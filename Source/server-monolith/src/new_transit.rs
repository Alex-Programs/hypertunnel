use dashmap::DashMap;
use libtransit::{
    ClientMetaUpstream, DownStreamMessage, ServerMetaDownstream, SocketID, UpStreamMessage,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use flume;

use tokio::io::{Interest, AsyncWriteExt};
use tokio::net::TcpStream;

use libsecrets::EncryptionKey;

pub struct SessionActorsStorage {
    pub to_bundler_stream: mpsc::UnboundedSender<UpStreamMessage>,
    pub from_bundler_stream: flume::Receiver<Vec<DownStreamMessage>>,
    pub key: EncryptionKey,
}

pub async fn create_actor(key: &EncryptionKey) -> SessionActorsStorage {
    // Create the channel
    let (to_bundler_stream, from_http_stream) = mpsc::unbounded_channel();
    let (to_http_stream, from_bundler_stream) = flume::unbounded();

    // Create the storage
    let storage = SessionActorsStorage {
        to_bundler_stream,
        from_bundler_stream,
        key: key.clone(),
    };

    // Start the handler
    tokio::spawn(handle_session(from_http_stream, to_http_stream));

    // Return the storage
    storage
}

pub async fn handle_session(
    mut from_http_stream: mpsc::UnboundedReceiver<UpStreamMessage>,
    to_http_stream: flume::Sender<Vec<DownStreamMessage>>,
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
                            buffer_size += message.payload.len();
                            buffer.push(message);
                            no_received_tcp = false;
                        }
                        Err(e) => {
                            // Check if the channel is empty
                            if e == mpsc::error::TryRecvError::Empty {
                                return;
                            } else {
                                // Panic
                                panic!("Error receiving from TCP handler: {:?}", e);
                            }
                        }
                    }
                });
        }

        // Check if we should return the buffer
        if buffer_size > 128 * 1024 {
            // Send the buffer
            to_http_stream.send(buffer.clone()).unwrap();

            // Reset the buffer
            buffer.clear();
            buffer_size = 0;

            // Update the last return time
            last_return_time = Instant::now();
        }

        // Check if it exceeds modetime, but there's nothing in the buffer
        if last_return_time.elapsed() > Duration::from_millis(50) && buffer_size == 0 {
            // Update the last return time
            last_return_time = Instant::now();
        }

        // Check if last_return_time exceeds modetime
        if last_return_time.elapsed() > Duration::from_millis(50) {
            // Send the buffer
            to_http_stream.send(buffer.clone()).unwrap();

            // Reset the buffer
            buffer.clear();
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

                    // Add to the managed sockets
                    managed_sockets.push(socket_id);

                    // Spawn the TCP handler
                    tokio::spawn(handle_tcp(from_http_stream, to_http_stream, message.dest_ip, message.dest_port, message.socket_id));

                    // Get the TCP handler
                    let tcp_handler = stream_to_tcp_handlers.get(&socket_id).unwrap();
                    tcp_handler
                } else {
                    tcp_handler.unwrap()
                };

                // Send the message to the TCP handler
                tcp_handler.send(message).unwrap();
            }
            Err(e) => {
                // Check if the channel is empty
                if e == mpsc::error::TryRecvError::Empty {
                    no_received_http = true;
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
                    panic!("Error reading from TCP stream: {:?}", e);
                }
            };

            if bytes_read == 0 {
                // Remote has closed
                // TODO handle this properly
            }

            // Send the message
            to_http_stream.send(downstream_msg).unwrap();

            // Increment the return sequence number
            return_sequence_number += 1;
        }

        if ready.is_writable() {
            let message = match from_http_stream.try_recv() {
                Ok(message) => message,
                Err(e) => {
                    // Check if the channel is empty
                    if e == mpsc::error::TryRecvError::Empty {
                        continue;
                    } else {
                        // Panic
                        panic!("Error receiving from HTTP stream: {:?}", e);
                    }
                }
            };

            let seq_num = message.message_sequence_number;

            debug_assert_eq!(last_sent_seq_number_sanity + 1, seq_num as i64);

            last_sent_seq_number_sanity = seq_num as i64;

            // Write payload
            stream.write_all(&message.payload).await.unwrap();
        }
    }
}