use libtransit::{
    DeclarationToken, DownStreamMessage, SocketID,
    SocksSocketDownstream, SocksSocketUpstream,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
use tokio::net::TcpStream;

use libsecrets::EncryptionKey;

use log::{debug, error, info};

use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use std::collections::HashMap;

type SequenceNumber = u32;

pub struct SessionActorsStorage {
    pub to_bundler_stream: mpsc::UnboundedSender<SocksSocketUpstream>,
    pub from_bundler_stream: flume::Receiver<(Vec<SocksSocketDownstream>, SequenceNumber)>,
    pub key: EncryptionKey,
    pub next_seq_num_up: AtomicU32,
    pub declaration_token: DeclarationToken,
    pub to_coordinator_yellow: mpsc::UnboundedSender<SocketID>,
}

pub async fn create_actor(key: &EncryptionKey, token: DeclarationToken) -> SessionActorsStorage {
    // Create the channel
    let (to_bundler_stream, from_http_stream) = mpsc::unbounded_channel();
    let (to_http_stream, from_bundler_stream) = flume::unbounded();
    let (to_coordinator_yellow, from_coordinator_yellow) = mpsc::unbounded_channel();

    // Create the storage
    let storage = SessionActorsStorage {
        to_bundler_stream,
        from_bundler_stream,
        key: key.clone(),
        next_seq_num_up: AtomicU32::new(0),
        declaration_token: token,
        to_coordinator_yellow,
    };

    // Start the handler
    tokio::spawn(handle_session(
        from_http_stream,
        to_http_stream,
        from_coordinator_yellow,
        AtomicU32::new(0),
    ));

    // Return the storage
    storage
}

pub async fn handle_session(
    mut from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    to_http_stream: flume::Sender<(Vec<SocksSocketDownstream>, SequenceNumber)>,
    mut from_coordinator_yellow: mpsc::UnboundedReceiver<SocketID>,
    seq_num_down: AtomicU32,
) {
    let mut stream_to_tcp_handlers: HashMap<SocketID, mpsc::UnboundedSender<SocksSocketUpstream>> =
        HashMap::new();
    let mut stream_from_tcp_handlers: HashMap<
        SocketID,
        mpsc::UnboundedReceiver<DownStreamMessage>,
    > = HashMap::new();

    let (blue_termination_sender, mut blue_termination_receiver) = mpsc::unbounded_channel();

    let mut yellow_informers: HashMap<SocketID, Arc<AtomicBool>> = HashMap::new();

    let mut last_return_time: Instant = Instant::now();
    let mut buffer_size = 0;
    let mut downstream_sockets: Vec<SocksSocketDownstream> = Vec::new();

    let mut last_iteration_time: Instant = Instant::now();

    loop {
        // Check how long the loop took
        let loop_time = last_iteration_time.elapsed();
        let loop_time_ms = loop_time.as_millis();
        if loop_time_ms > 15 {
            // TODO handle this better
            info!("Coordinator took {}ms", loop_time_ms);
        }
        last_iteration_time = Instant::now();

        let mut no_received_http = true;
        let mut no_received_tcp = false;

        // Handle inbound yellow messages
        loop {
            match from_coordinator_yellow.try_recv() {
                Ok(socket_id) => {
                    // Remove from the map
                    stream_to_tcp_handlers.remove(&socket_id);

                    // Inform it to close ASAP
                    let informer = yellow_informers.get(&socket_id).unwrap();

                    informer.store(true, Ordering::Relaxed);

                    // Remove from the map
                    yellow_informers.remove(&socket_id);
                }
                Err(e) => {
                    // Check if the channel is empty
                    if e == mpsc::error::TryRecvError::Empty {
                        break;
                    } else {
                        // Panic
                        panic!("Error receiving yellow messages from HTTP stream: {:?}", e);
                    }
                }
            }
        }

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

                            if message.do_green_terminate {
                                downstream_socket.do_green_terminate = true; // It'll be removed later when we send the buffer back
                            } else {
                                downstream_socket.payload.extend(message.payload);
                            }
                            
                            no_received_http = false;
                        }
                        Err(e) => {
                            if e == tokio::sync::mpsc::error::TryRecvError::Empty {
                                return;
                            } else if e == tokio::sync::mpsc::error::TryRecvError::Disconnected {
                                // Terminate the socket
                                downstream_socket.do_green_terminate = true;
                            }
                        }
                    }
                });
        }

        // Set do_blue_terminate for each socket based on ingress from the blue channel
        loop {
            match blue_termination_receiver.try_recv() {
                Ok(socket_id) => {
                    // Get the downstream socket
                    let downstream_socket = downstream_sockets
                        .iter_mut()
                        .find(|socket| socket.socket_id == socket_id)
                        .unwrap();

                    downstream_socket.do_blue_terminate = true;

                    // Remove from tcp_to_http handler
                    stream_from_tcp_handlers.remove(&socket_id);
                }
                Err(e) => {
                    // Check if the channel is empty
                    if e == mpsc::error::TryRecvError::Empty {
                        break;
                    } else {
                        // Panic
                        panic!("Error receiving blue messages from TCP handler: {:?}", e);
                    }
                }
            }
        }

        // Check if we should return the buffer
        let should_return = if buffer_size > 128 * 1024 {
            debug!("Returning; Buffer size is {}", buffer_size);
            true
        } else if last_return_time.elapsed() > Duration::from_millis(10) {
            if buffer_size > 0 {
                debug!("Returning; Time since last return is {:?}", last_return_time.elapsed());

                last_return_time = Instant::now();

                true
            } else {
                last_return_time = Instant::now();

                false
            }
        } else {
            false
        };

        if should_return {
            to_http_stream.send((downstream_sockets.clone(), seq_num_down.fetch_add(1, Ordering::SeqCst))).unwrap();

            downstream_sockets.retain(|socket| !socket.do_green_terminate);

            // Reset the buffer
            for socket in &mut downstream_sockets { // NOTE: TODO: This will memory leak in a long enough period unless we have a timeout for unused sockets
                socket.payload.clear();
            }

            buffer_size = 0;

            // Update the last return time
            last_return_time = Instant::now();
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
                        do_green_terminate: false,
                        do_blue_terminate: false,
                    };

                    downstream_sockets.push(downstream_socket);

                    let yellow_informer = Arc::new(AtomicBool::new(false));

                    // Add the informer to the map
                    yellow_informers.insert(socket_id, yellow_informer.clone());

                    // Spawn the TCP handler
                    tokio::spawn(handle_tcp(
                        from_http_stream,
                        to_http_stream,
                        message.dest_ip,
                        message.dest_port,
                        message.socket_id,
                        yellow_informer,
                        blue_termination_sender.clone()
                    ));

                    // Get the TCP handler
                    let tcp_handler = stream_to_tcp_handlers.get(&socket_id).unwrap();
                    tcp_handler
                } else {
                    tcp_handler.unwrap()
                };

                let do_red_terminate = message.red_terminate;

                // Send the payload to the TCP handler
                match tcp_handler.send(message) {
                    Ok(_) => {
                        // Just let it pass through
                    }
                    Err(e) => {
                        info!("Error sending to TCP handler: {:?}", e);

                        // There isn't really much we can do - the uploader is gone and should have already sent down a terminate message

                        // Just continue
                    }
                }

                // Check if the message says to terminate
                if do_red_terminate {
                    // Remove the HTTP-to-TCP for this socket
                    stream_to_tcp_handlers.remove(&socket_id);

                    // Everything else should be handled in the sent-on-message having the terminate flag in the handler
                }
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

// TODO
fn send_down_blue_channel(blue_message_sender: mpsc::UnboundedSender<SocketID>, socket_id: SocketID) {
    debug!("Socket {} is closing on blue route", socket_id);

    // Send the message
    blue_message_sender.send(socket_id).unwrap();
}

async fn handle_tcp_up(
    mut from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    mut tcp_write_half: tokio::net::tcp::OwnedWriteHalf,
    socket_id: SocketID,
    blue_message_sender: mpsc::UnboundedSender<SocketID>,
) {
    loop {
        let ready = tcp_write_half.ready(Interest::WRITABLE).await;

        if ready.is_err() {
            panic!("Error getting ready state: {:?}", ready);
        }

        let ready = ready.unwrap();

        let go_ahead = if ready.is_writable() {
            true
        } else if ready.is_empty() {
            false
        } else if ready.is_write_closed() {
            // Send and close
            send_down_blue_channel(blue_message_sender, socket_id);
            return;
        } else if ready.is_readable() {
            panic!("Asking if something is writable returned 'readable' ready state!?!?");
        } else if ready.is_read_closed() {
            panic!("Asking if something is writable returned 'read closed' ready state!?!?");
        } else {
            panic!("Unknown ready state: {:?}", ready);
        };

        if go_ahead {
            // We're using an asynchronous wait here to avoid the old hot-loop system
            let message = match from_http_stream.recv().await {
                Some(message) => message,
                None => {
                    // Qe need to close the socket
                    send_down_blue_channel(blue_message_sender, socket_id);
                    return;
                }
            };

            // Write payload
            tcp_write_half.write_all(&message.payload).await.unwrap();

            debug!("Written to the stream");

            // Check if we should terminate
            if message.red_terminate {
                // Simply return
                return;
            }
        }
    }
}

fn send_green_terminate(to_http_stream: mpsc::UnboundedSender<DownStreamMessage>, socket_id: SocketID) {
    debug!("Socket {} is closing on green route", socket_id);

    let downstream_msg = DownStreamMessage {
        socket_id,
        payload: Vec::with_capacity(0),
        do_green_terminate: true,
    };

    // Send the message
    match to_http_stream.send(downstream_msg) {
        Ok(_) => {}
        Err(e) => {
            // TODO handle this better
            error!("Error sending green terminate: {:?}", e);
        }
    }
}

async fn handle_tcp_down(
    to_http_stream: mpsc::UnboundedSender<DownStreamMessage>,
    mut tcp_read_half: tokio::net::tcp::OwnedReadHalf,
    socket_id: SocketID,
    yellow_informer: Arc<AtomicBool>,
) {
    loop {
        if yellow_informer.load(Ordering::Relaxed) {
            debug!("Socket {} is yellow, closing", socket_id);
            return;
        }

        let ready = tcp_read_half.ready(Interest::READABLE).await;

        if yellow_informer.load(Ordering::Relaxed) {
            debug!("Socket {} is yellow, closing", socket_id);
            return;
        }

        if ready.is_err() {
            // TODO handle this better
            panic!("Error getting ready state: {:?}", ready);
        }

        let ready = ready.unwrap();

        let go_ahead = if ready.is_readable() {
            true
        } else if ready.is_empty() {
            false
        } else if ready.is_read_closed() {
            // Send down green channel, then return
            send_green_terminate(to_http_stream, socket_id);
            return;
        } else if ready.is_writable() {
            panic!("Asking if something is readable returned 'writable' ready state!?!?");
        } else if ready.is_write_closed() {
            panic!("Asking if something is readable returned 'write closed' ready state!?!?");
        } else {
            panic!("Unknown ready state: {:?}", ready);
        };

        if go_ahead {
            let mut downstream_msg = DownStreamMessage {
                socket_id,
                payload: vec![0; 512], // TODO make this configurable
                do_green_terminate: false,
            };

            // Write in directly for efficiency
            // We're using an asynchronous wait here to avoid the old hot-loop system
            let bytes_read = match tcp_read_half.read(&mut downstream_msg.payload).await {
                Ok(bytes_read) => bytes_read,
                Err(e) => {
                    debug!("Sending green terminate due to error: {:?}", e);
                    send_green_terminate(to_http_stream, socket_id);
                    return;
                }
            };

            if yellow_informer.load(Ordering::Relaxed) {
                debug!("Socket {} is yellow, closing", socket_id);
                return;
            }

            if bytes_read == 0 {
                // Socket is closing. Send back a terminate message then close
                send_green_terminate(to_http_stream, socket_id);
                return;
            }

            // Trim array down to size
            downstream_msg.payload.truncate(bytes_read);

            // Send back
            to_http_stream.send(downstream_msg).unwrap();
        }
    }
}

pub async fn handle_tcp(
    from_http_stream: mpsc::UnboundedReceiver<SocksSocketUpstream>,
    to_http_stream: mpsc::UnboundedSender<DownStreamMessage>,
    ip: u32,
    port: u16,
    socket_id: SocketID,
    yellow_informer: Arc<AtomicBool>,
    handle_blue_sender: mpsc::UnboundedSender<SocketID>,
) {
    let stream = TcpStream::connect(format!("{}:{}", ip, port)).await;
    let stream = match stream {
        Ok(stream) => stream,
        Err(e) => {
            // Just close
            debug!("Error connecting to {}:{} (Error: {:?})", ip, port, e);
            send_green_terminate(to_http_stream, socket_id);

            return;
        }
    };

    // Split the stream
    let (tcp_read_half, tcp_write_half) = stream.into_split();

    // Spawn the TCP handlers
    tokio::spawn(handle_tcp_up(
        from_http_stream,
        tcp_write_half,
        socket_id,
        handle_blue_sender
    ));

    tokio::spawn(handle_tcp_down(
        to_http_stream,
        tcp_read_half,
        socket_id,
        yellow_informer,
    ));
}
