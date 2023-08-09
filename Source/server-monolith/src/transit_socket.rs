use dashmap::DashMap;
use libsecrets::EncryptionKey;
use libtransit::{ClientMetaUpstream, ServerStreamInfo};
use libtransit::{
    CloseSocketMessage, DeclarationToken, DownStreamMessage, Port, SocketID, UpStreamMessage, IPV4,
};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::RwLock;

#[derive(Debug)]
pub enum SocksStreamInboundMessage {
    UpStreamMessage(UpStreamMessage),
    CloseSocketMessage(CloseSocketMessage),
}

pub struct TransitSocket {
    pub key: EncryptionKey,
    pub client_statistics: ClientMetaUpstream,
    // TODO information for communicating with SOCKS threads, returning
    // requests, etc. Need to figure out how to do this.
    socks_streams: DashMap<SocketID, mpsc::Sender<SocksStreamInboundMessage>>,
    tcp_data_return: broadcast::Receiver<DownStreamMessage>,
    tcp_data_assign: broadcast::Sender<DownStreamMessage>,
    meta_return_data: Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
    our_declaration_token: DeclarationToken,
}

impl TransitSocket {
    pub fn new(
        key: EncryptionKey,
        meta_return_data: Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
        our_declaration_token: DeclarationToken,
    ) -> Self {
        let (tcp_data_assign, tcp_data_return) = broadcast::channel(10000);

        meta_return_data.insert(
            our_declaration_token.clone(),
            RwLock::new(ServerStreamInfo {
                has_terminated: false,
                logs: vec![],
                errors: vec![],
                declaration_token: our_declaration_token.clone(),
            }),
        );

        Self {
            key,
            client_statistics: ClientMetaUpstream {
                bytes_to_send_to_remote: 0,
                bytes_to_reply_to_client: 0,
                messages_to_send_to_remote: 0,
                messages_to_reply_to_client: 0,
            },
            socks_streams: DashMap::new(),
            tcp_data_return,
            tcp_data_assign,
            meta_return_data,
            our_declaration_token,
        }
    }

    pub async fn get_data(&mut self, buffer_size: usize, modetime_ms: u32) -> Vec<DownStreamMessage> {
        // Continuously pull from tcp_data_return
        let mut start_time = std::time::Instant::now();

        /* Description from notes:
        Each TCP stream task is given a reference to the return-data message passer.
        They can feed data in to that. Returning can call `get_data(buffsize, modetime)` on the transit socket.
        That function pulls from the data message passer. If the buffer size is filled, it returns.
        After `modetime` has elapsed it will return data regardless of if it's filled the `buffsize`.
        If data appears *after* `modetime` where the buffer was *not filled at all* before `modetime` it will wait for the buffer to be filled *or* `modetime` to elapse again.
        */

        let mut messages = vec![];

        loop {
            // Check if the buffer is filled
            if messages.len() >= buffer_size {
                return messages;
            }

            // Check if the modetime has elapsed
            if start_time.elapsed().as_millis() >= modetime_ms as u128 {
                // Is there any data?
                if messages.len() > 0 {
                    // If there is data, return it
                    return messages;
                } else {
                    // If there is no data, wait for a bit and reset the timer
                    start_time = std::time::Instant::now();
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                }
            }

            // Check if there is data to read
            if let Ok(message) = self.tcp_data_return.recv().await { // TODO slightly worried about this locking up other requests to this function
                // Add the data to the buffer
                messages.push(message);
            } else {
                // If there is no data to read, wait for a bit
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        }
    }

    pub async fn process_close_socket_message(&mut self, message: CloseSocketMessage) {
        let socket_id = message.socket_id;

        // Re-form
        let message = SocksStreamInboundMessage::CloseSocketMessage(message);

        // Check if the socket is already in the map
        if let Some(sender) = self.socks_streams.get(&socket_id) {
            // Send the message to the task
            sender.send(message).await.unwrap();
        } else {
            // This genuinely just means a programming mistake, so no log returns
            panic!("Unordered socket deletion: This should not happen because data should flow before close messages. Check process_sock_message feed order.")
        }
    }

    pub async fn process_upstream_message(&mut self, message: UpStreamMessage) {
        let socket_id = message.socket_id;

        // Check if the socket is already in the map
        if let Some(sender) = self.socks_streams.get(&socket_id) {
            // Send the message to the task
            sender
                .send(SocksStreamInboundMessage::UpStreamMessage(message))
                .await
                .unwrap();
        } else {
            // Spawn a new task to handle this socket
            // First create a channel to communicate with the task
            // The reason this section can be slightly "implicit" is because we send dest ip and dest port in
            let (sender, receiver) = mpsc::channel(10000);

            let address = message.dest_ip;
            let port = message.dest_port;

            // Send the message to the task
            sender
                .send(SocksStreamInboundMessage::UpStreamMessage(message))
                .await
                .unwrap();

            // Insert the sender into the map
            self.socks_streams.insert(socket_id, sender);

            // Construct the arguments for the task
            let arguments = TCPHandlerArguments {
                receiver,
                sender: self.tcp_data_assign.clone(),
                address,
                port,
                socket_id,
                meta_return_data: self.meta_return_data.clone(),
                our_declaration_token: self.our_declaration_token.clone(),
            };

            // Spawn the task
            tokio::spawn(tcp_handler_task(arguments));
        }
    }
}

async fn return_error(
    error: String,
    meta_return_data: &Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
    our_declaration_token: &DeclarationToken,
) {
    eprintln!("Error: {}", error);
    let server_stream_info = meta_return_data.get(our_declaration_token).unwrap();
    let server_stream_info = server_stream_info.value();
    server_stream_info.write().await.errors.push(error);
}

async fn return_log(
    log: String,
    meta_return_data: &Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
    our_declaration_token: &DeclarationToken,
) {
    println!("{}", log);
    let server_stream_info = meta_return_data.get(our_declaration_token).unwrap();
    let server_stream_info = server_stream_info.value();
    server_stream_info.write().await.logs.push(log);
}

async fn terminate_transit_socket(
    meta_return_data: &Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
    our_declaration_token: &DeclarationToken,
) {
    let server_stream_info = meta_return_data.get(our_declaration_token).unwrap();
    let server_stream_info = server_stream_info.value();
    server_stream_info.write().await.has_terminated = true;

    // TODO: Send a message to the TCP handler threads to terminate under a specialised mspc, then use a shared mspc of things-to-terminate to let the
    // request handlers know to terminate this transit socket, and delete from current sessions
}

struct TCPHandlerArguments {
    receiver: mpsc::Receiver<SocksStreamInboundMessage>,
    sender: broadcast::Sender<DownStreamMessage>,
    address: IPV4,
    port: Port,
    socket_id: SocketID,
    meta_return_data: Arc<DashMap<DeclarationToken, RwLock<ServerStreamInfo>>>,
    our_declaration_token: DeclarationToken,
}

// Constructed for each SOCKS TCP connection. Takes in a receiver for receiving upstream messages from clients, and a sender for sending downstream messages to clients.
// Remember: Incoming data may be out of order, so we need to check that it matches the sequence number.
// If it doesn't, we need to wait till it comes.
// For now, if data is lost this is going to stay open forever. In the future we should add client-side buffering of data and a "data-lost, please resend" message.
// However the lower level protocol should be reliable through request level retries so this should be rare-to-impossible.
// Though this does provide a DOS vector, so we should probably also add some sort of timeout. TODO
async fn tcp_handler_task(mut arguments: TCPHandlerArguments) {
    // Connect to the server
    let mut stream =
        match TcpStream::connect(format!("{}:{}", arguments.address, arguments.port)).await {
            Ok(stream) => stream,
            Err(_) => {
                // TODO handle this better - have an error and a way to send it back to the client
                return_error(
                    format!(
                        "Could not connect to server from TCP handler task to address {}:{}",
                        arguments.address, arguments.port
                    ),
                    &arguments.meta_return_data,
                    &arguments.our_declaration_token,
                );
                return;
            }
        };

    // Begin loop of transferring TCP data back and forth
    let mut sequence_number = 0;
    let mut last_sequence_number_sanity = 0;
    let mut return_sequence_number = 0;

    let mut queue: Vec<SocksStreamInboundMessage> = Vec::with_capacity(16);

    loop {
        // Sort the queue by sequence number
        queue.sort_by(|a, b| {
            let seq_num_a = get_seq_num(a);
            let seq_num_b = get_seq_num(b);

            seq_num_a.cmp(&seq_num_b)
        });

        // Get data from the receiver OR the queue
        // Check if the queue's latest message is the next one in sequence
        let is_queue_next = if queue.len() > 0 {
            match &queue[0] {
                SocksStreamInboundMessage::UpStreamMessage(interior) => {
                    interior.message_sequence_number == sequence_number
                }
                SocksStreamInboundMessage::CloseSocketMessage(interior) => {
                    interior.message_sequence_number == sequence_number
                }
            }
        } else {
            false
        };

        // If the queue isn't next, get data from the receiver
        let message = if is_queue_next {
            // Pop the message off the queue
            queue.remove(0)
        } else {
            // Get data from the receiver
            let data = arguments.receiver.recv().await.unwrap();

            // Check if the message is the next one in sequence
            let sequence_number = match &data {
                SocksStreamInboundMessage::UpStreamMessage(interior) => {
                    interior.message_sequence_number
                }
                SocksStreamInboundMessage::CloseSocketMessage(interior) => {
                    interior.message_sequence_number
                }
            };

            if sequence_number == sequence_number {
                // If it is, return it
                data
            } else {
                // If it isn't, put it in the queue
                queue.push(data);

                // And loop - hopefully we'll get it later!
                continue;
            }
        };

        // Now we have the latest message!
        // Sanity check
        debug_assert!(get_seq_num(&message) == last_sequence_number_sanity + 1);

        // Now actually process it
        match message {
            SocksStreamInboundMessage::UpStreamMessage(message) => {
                // Send the data to the server
                stream.write_all(&message.payload).await.unwrap();
            }
            SocksStreamInboundMessage::CloseSocketMessage(_) => {
                // Close the socket
                stream.shutdown().await.unwrap();

                // TODO remove the socket from the map

                // Break out of the loop
                break;
            }
        }

        // Read buffer
        let mut buffer = vec![0; 2048]; // TODO variable buffer size
        let bytes_read = match stream.read(&mut buffer).await {
            Ok(bytes_read) => bytes_read,
            Err(_) => {
                return_error(
                    format!(
                        "Could not read from server from TCP handler task to address {}:{}",
                        arguments.address, arguments.port
                    ),
                    &arguments.meta_return_data,
                    &arguments.our_declaration_token,
                );
                return;
            }
        };

        // Sometimes less is sent than requested, so truncate the buffer. In the future we'll have a way to tune the max buffer size.
        buffer.truncate(bytes_read);

        // Create a downstream message
        let message = DownStreamMessage {
            socket_id: arguments.socket_id,
            message_sequence_number: return_sequence_number,
            payload: buffer,
            has_remote_closed: false,
        };

        // Send the message to the client
        arguments.sender.send(message).unwrap();

        // Increment the return sequence number
        return_sequence_number += 1;

        // Increment the sequence number
        last_sequence_number_sanity = sequence_number;
        sequence_number += 1;
    }
}

fn get_seq_num(msg: &SocksStreamInboundMessage) -> u32 {
    match msg {
        SocksStreamInboundMessage::UpStreamMessage(interior) => interior.message_sequence_number,
        SocksStreamInboundMessage::CloseSocketMessage(interior) => interior.message_sequence_number,
    }
}
