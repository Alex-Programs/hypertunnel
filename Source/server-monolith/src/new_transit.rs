use flume;
use libtransit::{
    ClientMetaUpstream, DeclarationToken, DownStreamMessage, ServerMetaDownstream, SocketID,
    SocksSocketDownstream, SocksSocketUpstream, UpStreamMessage,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
use tokio::net::TcpStream;

use libsecrets::EncryptionKey;

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

use crate::meta;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct SessionActorsStorage {
    pub to_bundler_stream: mpsc::UnboundedSender<SocksSocketUpstream>,
    pub from_bundler_stream: flume::Receiver<Vec<SocksSocketDownstream>>,
    pub key: EncryptionKey,
    pub seq_num_down: AtomicU32,
    pub next_seq_num_up: AtomicU32,
    pub declaration_token: DeclarationToken,
    pub traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
}

pub async fn create_actor(key: &EncryptionKey, token: DeclarationToken) -> SessionActorsStorage {
    // Create the channel
    let (to_bundler_stream, from_http_stream) = mpsc::unbounded_channel();
    let (to_http_stream, from_bundler_stream) = flume::unbounded();

    // Create the storage
    let storage = SessionActorsStorage {
        to_bundler_stream,
        from_bundler_stream,
        key: key.clone(),
        seq_num_down: AtomicU32::new(0),
        next_seq_num_up: AtomicU32::new(0),
        declaration_token: token,
        traffic_stats: Arc::new(meta::ServerMetaDownstreamTrafficStatsSynced {
            http_up_to_coordinator_bytes: AtomicU32::new(0),
            coordinator_up_to_socket_bytes: AtomicU32::new(0),
            socket_down_to_coordinator_bytes: AtomicU32::new(0),
            coordinator_down_to_http_message_passer_bytes: AtomicU32::new(0),
            coordinator_down_to_http_buffer_bytes: AtomicU32::new(0),
            congestion_ctrl_intake_throttle: AtomicU32::new(0),
        }),
    };

    // Start the handler
    tokio::spawn(handle_session(
        from_http_stream,
        to_http_stream,
        storage.traffic_stats.clone(),
    ));

    // Return the storage
    storage
}

pub async fn handle_session(
    mut from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    to_http_stream: flume::Sender<Vec<SocksSocketDownstream>>,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
) {
    let mut stream_to_tcp_handlers: HashMap<SocketID, mpsc::UnboundedSender<SocksSocketUpstream>> =
        HashMap::new();
    let mut stream_from_tcp_handlers: HashMap<
        SocketID,
        mpsc::UnboundedReceiver<DownStreamMessage>,
    > = HashMap::new();

    let mut last_return_time: Instant = Instant::now();
    let mut buffer_size = 0;
    let mut downstream_sockets: Vec<SocksSocketDownstream> = Vec::new();

    let mut last_iteration_time: Instant = Instant::now();

    loop {
        // Check how long the loop took
        let loop_time = last_iteration_time.elapsed();
        let loop_time_ms = loop_time.as_millis();
        if loop_time_ms > 5 {
            // TODO handle this better
            println!("Coordinator took {}ms", loop_time_ms);
        }
        last_iteration_time = Instant::now();

        let mut no_received_http = true;
        let mut no_received_tcp = false;

        // TCP to HTTP messages
        for downstream_socket in &mut downstream_sockets {
            // Get the TCP handler
            stream_from_tcp_handlers
                .entry(downstream_socket.socket_id)
                .and_modify(|tcp_handler| {
                    // Try to receive from the TCP handler
                    match tcp_handler.try_recv() {
                        Ok(message) => {
                            // Add to the buffer
                            let payload_length = message.payload.len();
                            buffer_size += payload_length;

                            let payload_length_u32 = payload_length as u32;

                            traffic_stats
                                .coordinator_down_to_http_buffer_bytes
                                .fetch_add(payload_length_u32, Ordering::Relaxed);
                            traffic_stats
                                .socket_down_to_coordinator_bytes
                                .fetch_sub(payload_length_u32, Ordering::Relaxed);

                            downstream_socket.payload.extend(message.payload);

                            no_received_http = false;
                        }
                        Err(e) => {
                            if e == tokio::sync::mpsc::error::TryRecvError::Empty {
                                return;
                            } else if e == tokio::sync::mpsc::error::TryRecvError::Disconnected {
                                dprintln!("TCP handler closed");
                                // TODO handle this - remove from managed sockets etc
                            }
                        }
                    }
                });
        }

        // Check if we should return the buffer
        if buffer_size > 128 * 1024 {
            // Send the buffer
            dprintln!("Sending buffer back due to size!");
            to_http_stream.send(downstream_sockets.clone()).unwrap();

            traffic_stats
                .coordinator_down_to_http_message_passer_bytes
                .fetch_add(buffer_size as u32, Ordering::Relaxed);
            traffic_stats
                .coordinator_down_to_http_buffer_bytes
                .fetch_sub(buffer_size as u32, Ordering::Relaxed);

            // Reset the buffer
            for socket in &mut downstream_sockets { // NOTE: TODO: This will memory leak in a long enough period unless we have a timeout for unused sockets
                socket.payload.clear();
            }

            buffer_size = 0;

            // Update the last return time
            last_return_time = Instant::now();
        }

        // Check if it exceeds modetime
        if last_return_time.elapsed() > Duration::from_millis(10) {
            if buffer_size > 0 {
                dprintln!("Sending buffer back due to modetime!");

                // Send the buffer
                to_http_stream.send(downstream_sockets.clone()).unwrap();

                traffic_stats
                    .coordinator_down_to_http_message_passer_bytes
                    .fetch_add(buffer_size as u32, Ordering::Relaxed);
                traffic_stats
                    .coordinator_down_to_http_buffer_bytes
                    .fetch_sub(buffer_size as u32, Ordering::Relaxed);

                // Reset the buffer
                for socket in &mut downstream_sockets { // NOTE: TODO: This will memory leak in a long enough period unless we have a timeout for unused sockets
                    socket.payload.clear();
                }

                buffer_size = 0;

                // Update the last return time
                last_return_time = Instant::now();
            } else {
                // Update the last return time
                last_return_time = Instant::now();
            }
        }

        // HTTP to TCP messages
        match from_http_stream.try_recv() {
            Ok(message) => {
                // Get the socket ID
                let socket_id = message.socket_id;

                // Get the TCP handler
                let tcp_handler = stream_to_tcp_handlers.get(&socket_id);

                let tcp_handler = if tcp_handler.is_none() {
                    // Create a new TCP handler
                    // For sending upstreams to the handle_tcp
                    let (to_tcp_handler, from_http_stream) = mpsc::unbounded_channel();
                    // For receiving downstreams from the handle_tcp
                    let (to_http_stream, from_tcp_handler) = mpsc::unbounded_channel();

                    // Add the TCP handler to the map
                    stream_to_tcp_handlers.insert(socket_id, to_tcp_handler);
                    stream_from_tcp_handlers.insert(socket_id, from_tcp_handler);

                    // Add downstream socket
                    let downstream_socket = SocksSocketDownstream {
                        socket_id,
                        dest_ip: message.dest_ip,
                        dest_port: message.dest_port,
                        payload: Vec::with_capacity(512),
                        termination_reasons: Vec::with_capacity(1),
                    };

                    downstream_sockets.push(downstream_socket);

                    // Spawn the TCP handler
                    tokio::spawn(handle_tcp(
                        from_http_stream,
                        to_http_stream,
                        message.dest_ip,
                        message.dest_port,
                        message.socket_id,
                        traffic_stats.clone(),
                    ));

                    // Get the TCP handler
                    let tcp_handler = stream_to_tcp_handlers.get(&socket_id).unwrap();
                    tcp_handler
                } else {
                    tcp_handler.unwrap()
                };

                // Send the payload to the TCP handler
                let msg_size = message.payload.len() as u32;
                tcp_handler.send(message).unwrap();

                traffic_stats
                    .coordinator_up_to_socket_bytes
                    .fetch_add(msg_size, Ordering::Relaxed);
                traffic_stats
                    .http_up_to_coordinator_bytes
                    .fetch_sub(msg_size, Ordering::Relaxed);
            }
            Err(e) => {
                // Check if the channel is empty
                if e == mpsc::error::TryRecvError::Empty {
                    no_received_tcp = true;
                } else {
                    // Panic
                    panic!("Error receiving from HTTP stream: {:?}", e);
                }
            }
        }

        if no_received_http && no_received_tcp {
            // Sleep for 1ms
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

async fn handle_tcp_up(
    mut from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    mut tcp_write_half: tokio::net::tcp::OwnedWriteHalf,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
    socket_id: SocketID,
) {
    let mut return_sequence_number = 0;

    loop {
        let ready = tcp_write_half.ready(Interest::WRITABLE).await;

        if ready.is_err() {
            // TODO handle this better
            panic!("Error getting ready state: {:?}", ready);
        }

        let ready = ready.unwrap();

        if ready.is_writable() {
            // We're using an asynchronous wait here to avoid the old hot-loop system
            let mut message = match from_http_stream.recv().await {
                Some(message) => message,
                None => {
                    // TODO handle this better
                    panic!("Error receiving from HTTP stream: {:?}", from_http_stream);
                }
            };

            let payload_size = message.payload.len() as u32;
            traffic_stats
                .coordinator_up_to_socket_bytes
                .fetch_sub(payload_size, Ordering::Relaxed);

            // Write payload
            tcp_write_half.write_all(&message.payload).await.unwrap();

            dprintln!("Written to the stream");
        }
    }
}

async fn handle_tcp_down(
    to_http_stream: mpsc::UnboundedSender<DownStreamMessage>,
    mut tcp_read_half: tokio::net::tcp::OwnedReadHalf,
    socket_id: SocketID,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
) {
    loop {
        let ready = tcp_read_half.ready(Interest::READABLE).await;

        if ready.is_err() {
            // TODO handle this better
            panic!("Error getting ready state: {:?}", ready);
        }

        let ready = ready.unwrap();

        if ready.is_readable() {
            let mut downstream_msg = DownStreamMessage {
                socket_id,
                payload: vec![0; 512], // TODO make this configurable
            };

            // Write in directly for efficiency
            // We're using an asynchronous wait here to avoid the old hot-loop system
            let bytes_read = match tcp_read_half.read(&mut downstream_msg.payload).await {
                Ok(bytes_read) => bytes_read,
                Err(e) => {
                    panic!("Error reading from TCP stream: {:?}", e);
                }
            };

            if bytes_read == 0 {
                // TODO handle socket close
                return;
            }

            // Trim array down to size
            downstream_msg.payload.truncate(bytes_read);

            dprintln!("Read data: {:?}", downstream_msg.payload);

            // Send the message
            let payload_size = downstream_msg.payload.len() as u32;

            // Send back
            to_http_stream.send(downstream_msg).unwrap();

            traffic_stats
                .socket_down_to_coordinator_bytes
                .fetch_add(payload_size, Ordering::Relaxed);
        }
    }
}

pub async fn handle_tcp(
    from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    to_http_stream: mpsc::UnboundedSender<DownStreamMessage>,
    ip: u32,
    port: u16,
    socket_id: SocketID,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
) {
    let stream = TcpStream::connect(format!("{}:{}", ip, port)).await;
    let stream = match stream {
        Ok(stream) => stream,
        Err(e) => {
            // TODO handle this better
            panic!("Error connecting to TCP server: {:?}", e);
        }
    };

    // Split the stream
    let (tcp_read_half, tcp_write_half) = stream.into_split();

    // Spawn the TCP handlers
    tokio::spawn(handle_tcp_up(
        from_http_stream,
        tcp_write_half,
        traffic_stats.clone(),
        socket_id,
    ));

    tokio::spawn(handle_tcp_down(
        to_http_stream,
        tcp_read_half,
        socket_id,
        traffic_stats.clone(),
    ));
}
