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
use tokio::io::Interest;
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
    awaiting_downstreams: Vec<mpsc::Sender<Vec<DownStreamMessage>>>
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
            awaiting_downstreams: vec![]
        }
    }

    pub fn register_get_data_listener(&mut self) -> mpsc::Receiver<Vec<DownStreamMessage>> {
        let (send, recv) = mpsc::channel(1);

        // Add the sender to the downstream waiters
        self.awaiting_downstreams.push(send);

        recv
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
                )
                .await;
                return;
            }
        };

    // Begin loop of transferring TCP data back and forth
    let mut return_sequence_number = 0;
    let mut last_seq_number_sanity = 0;

    loop {
        let ready = stream.ready(Interest::READABLE | Interest::WRITABLE).await;

        if ready.is_err() {
            eprintln!("Ready error: {}", ready.err().unwrap());
            // TODO handle properly
            return;
        }

        let ready = ready.unwrap();

        if ready.is_readable() {
            let mut downstream_msg = DownStreamMessage {
                socket_id: arguments.socket_id,
                message_sequence_number: return_sequence_number,
                payload: Vec::with_capacity(0), // So it doesn't do a small allocation only to do a huge one later
                has_remote_closed: false,
            };

            // Write in directly for efficiency
            let bytes_read = match stream.try_read(&mut downstream_msg.payload) {
                Ok(bytes_read) => bytes_read,
                Err(_) => {
                    // TODO handle this properly
                    return_error(
                        format!(
                            "Could not read from server from TCP handler task to address {}:{}",
                            arguments.address, arguments.port
                        ),
                        &arguments.meta_return_data,
                        &arguments.our_declaration_token,
                    )
                    .await;
                    return;
                }
            };
            if bytes_read == 0 {
                // The remote has closed the connection
                // TODO handle this properly
                return;
            }

            // Send on the message
            arguments.sender.send(downstream_msg).unwrap();

            return_sequence_number += 1;
        }

        if ready.is_writable() {
            let message = arguments.receiver.recv().await.unwrap();
            
            let seq_num = get_seq_num(&message);
            debug_assert_eq!(seq_num, last_seq_number_sanity + 1);

            match message {
                SocksStreamInboundMessage::UpStreamMessage(message) => {
                    stream.write_all(&message.payload).await.unwrap();
                    last_seq_number_sanity = seq_num;
                }
                SocksStreamInboundMessage::CloseSocketMessage(message) => {
                    // TODO handle - but ensure all data is read first
                    println!("TODO close now");
                }
            }
        }
    }
}

fn get_seq_num(msg: &SocksStreamInboundMessage) -> u32 {
    match msg {
        SocksStreamInboundMessage::UpStreamMessage(interior) => interior.message_sequence_number,
        SocksStreamInboundMessage::CloseSocketMessage(interior) => interior.message_sequence_number,
    }
}
