use dashmap::DashMap;
use libtransit::{
    ClientMetaUpstream, DownStreamMessage, ServerMetaDownstream, SocketID, UpStreamMessage, DeclarationToken,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use flume;

use tokio::io::{Interest, AsyncWriteExt};
use tokio::net::TcpStream;

use libsecrets::EncryptionKey;

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

use crate::meta;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct SessionActorsStorage {
    pub to_bundler_stream: mpsc::UnboundedSender<UpStreamMessage>,
    pub from_bundler_stream: flume::Receiver<Vec<DownStreamMessage>>,
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
    tokio::spawn(handle_session(from_http_stream, to_http_stream, storage.traffic_stats.clone()));

    // Return the storage
    storage
}

pub async fn handle_session(
    mut from_http_stream: mpsc::UnboundedReceiver<UpStreamMessage>,
    to_http_stream: flume::Sender<Vec<DownStreamMessage>>,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
) {
    let mut stream_to_tcp_handlers: HashMap<SocketID, mpsc::UnboundedSender<UpStreamMessage>> =
        HashMap::new();
    let mut stream_from_tcp_handlers: HashMap<
        SocketID,
        mpsc::UnboundedReceiver<DownStreamMessage>,
    > = HashMap::new();

    let mut managed_sockets: Vec<SocketID> = Vec::new();

    let mut last_return_time: Instant = Instant::now();
    let mut buffer_size = 0;
    let mut buffer: Vec<DownStreamMessage> = Vec::with_capacity(1024);

    loop {
        let mut no_received_http = true;
        let mut no_received_tcp = false;

        // TCP to HTTP messages
        for managed_socket in &managed_sockets {
            // Get the TCP handler
            stream_from_tcp_handlers
                .entry(*managed_socket)
                .and_modify(|tcp_handler| {
                    // Try to receive from the TCP handler
                    match tcp_handler.try_recv() {
                        Ok(message) => {
                            // Add to the buffer
                            let payload_length = message.payload.len();
                            buffer_size += payload_length; 

                            let payload_length_u32 = payload_length as u32;

                            traffic_stats.coordinator_down_to_http_buffer_bytes.fetch_add(payload_length_u32, Ordering::Relaxed);
                            traffic_stats.socket_down_to_coordinator_bytes.fetch_sub(payload_length_u32, Ordering::Relaxed);

                            buffer.push(message);
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
            to_http_stream.send(buffer.clone()).unwrap();
            traffic_stats.coordinator_down_to_http_message_passer_bytes.fetch_add(buffer_size as u32, Ordering::Relaxed);
            traffic_stats.coordinator_down_to_http_buffer_bytes.fetch_sub(buffer_size as u32, Ordering::Relaxed);

            // Reset the buffer
            buffer.clear();
            buffer_size = 0;

            // Update the last return time
            last_return_time = Instant::now();
        }

        // Check if it exceeds modetime, but there's nothing in the buffer
        if last_return_time.elapsed() > Duration::from_millis(10) {
            if buffer_size > 0 {
                dprintln!("Sending buffer back due to modetime!");

                // Send the buffer
                to_http_stream.send(buffer.clone()).unwrap();
                traffic_stats.coordinator_down_to_http_message_passer_bytes.fetch_add(buffer_size as u32, Ordering::Relaxed);
                traffic_stats.coordinator_down_to_http_buffer_bytes.fetch_sub(buffer_size as u32, Ordering::Relaxed);

                // Reset the buffer
                buffer.clear();
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
            Ok(mut message) => {
                // Set ingress time
                message.time_at_server_coordinator_ms = meta::ms_since_epoch();

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

                    // Add to the managed sockets
                    managed_sockets.push(socket_id);

                    // Spawn the TCP handler
                    tokio::spawn(handle_tcp(from_http_stream, to_http_stream, message.dest_ip, message.dest_port, message.socket_id, traffic_stats.clone()));

                    // Get the TCP handler
                    let tcp_handler = stream_to_tcp_handlers.get(&socket_id).unwrap();
                    tcp_handler
                } else {
                    tcp_handler.unwrap()
                };

                // Send the message to the TCP handler
                dprintln!("Sending message sequence number {} to TCP handler (socket ID {})", message.message_sequence_number, message.socket_id);
                let msg_size = message.payload.len() as u32;
                tcp_handler.send(message).unwrap();

                traffic_stats.coordinator_up_to_socket_bytes.fetch_add(msg_size, Ordering::Relaxed);
                traffic_stats.http_up_to_coordinator_bytes.fetch_sub(msg_size, Ordering::Relaxed);
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

pub async fn handle_tcp(
    mut from_http_stream: mpsc::UnboundedReceiver<UpStreamMessage>,
    to_http_stream: mpsc::UnboundedSender<DownStreamMessage>,
    ip: u32,
    port: u16,
    socket_id: SocketID,
    traffic_stats: Arc<meta::ServerMetaDownstreamTrafficStatsSynced>,
) {
    let stream = TcpStream::connect(format!("{}:{}", ip, port)).await;
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(e) => {
            // TODO handle this better
            panic!("Error connecting to TCP server: {:?}", e);
        }
    };

    let mut return_sequence_number = 0;
    let mut last_sent_seq_number_sanity: i64 = -1;

    loop {
        let ready = stream.ready(Interest::READABLE | Interest::WRITABLE).await;

        if ready.is_err() {
            // TODO handle this better
            panic!("Error getting ready state: {:?}", ready);
        }

        let ready = ready.unwrap();

        if ready.is_readable() {
            let mut downstream_msg = DownStreamMessage {
                socket_id,
                message_sequence_number: return_sequence_number,
                payload: Vec::with_capacity(0),
                has_remote_closed: false,
            };

            // Write in directly for efficiency
            let bytes_read = match stream.try_read_buf(&mut downstream_msg.payload) {
                Ok(bytes_read) => bytes_read,
                Err(e) => {
                    // TODO handle this better
                    // Check if it's because it's blocking
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        // Wait a moment
                        tokio::time::sleep(Duration::from_millis(1)).await;

                        continue;
                    } else {
                        panic!("Error reading from TCP stream: {:?}", e);
                    }
                }
            };

            if bytes_read == 0 {
                // Remote has closed
                // TODO handle this properly
            }

            // Send the message
            dprintln!("Sending message sequence number {} to HTTP stream (socket ID {})", downstream_msg.message_sequence_number, downstream_msg.socket_id);
            let payload_size = downstream_msg.payload.len() as u32;

            to_http_stream.send(downstream_msg).unwrap();

            traffic_stats.socket_down_to_coordinator_bytes.fetch_add(payload_size, Ordering::Relaxed);

            // Increment the return sequence number
            return_sequence_number += 1;
        }

        if ready.is_writable() {
            let mut message = match from_http_stream.try_recv() {
                Ok(message) => message,
                Err(e) => {
                    // Check if the channel is empty
                    if e == mpsc::error::TryRecvError::Empty {
                        // Wait a moment
                        tokio::time::sleep(Duration::from_millis(1)).await;

                        continue;
                    } else {
                        // Panic
                        panic!("Error receiving from HTTP stream: {:?}", e);
                    }
                }
            };
            message.time_at_server_socket_ms = meta::ms_since_epoch();

            let seq_num = message.message_sequence_number;

            debug_assert_eq!(last_sent_seq_number_sanity + 1, seq_num as i64);

            last_sent_seq_number_sanity = seq_num as i64;

            let payload_size = message.payload.len() as u32;
            traffic_stats.coordinator_up_to_socket_bytes.fetch_sub(payload_size, Ordering::Relaxed);
            
            // Write payload
            stream.write_all(&message.payload).await.unwrap();

            message.time_client_write_finished_ms = meta::ms_since_epoch();

            dprintln!("Written to the stream");

            println!("Message timing info: {}", message.render_latency_information());
        }
    }
}