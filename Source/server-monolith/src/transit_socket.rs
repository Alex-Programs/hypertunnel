use libsecrets::EncryptionKey;
use libtransit::{CloseSocketMessage, SocketID, IPV4, Port, UpStreamMessage, DownStreamMessage};
use tokio::sync::{mpsc, broadcast};
use dashmap::DashMap;

pub struct ClientStatistics {
    pub to_send: usize,
    pub to_reply: usize,
}

#[derive(Debug)]
pub enum SocksStreamInboundMessage {
    UpStreamMessage(UpStreamMessage),
    CloseSocketMessage(CloseSocketMessage),
}

pub struct TransitSocket {
    pub key: EncryptionKey,
    pub client_statistics: ClientStatistics,
    // TODO information for communicating with SOCKS threads, returning
    // requests, etc. Need to figure out how to do this.
    socks_streams: DashMap<SocketID, mpsc::Sender<SocksStreamInboundMessage>>,
    tcp_data_return: broadcast::Receiver<DownStreamMessage>,
    tcp_data_assign: broadcast::Sender<DownStreamMessage>,
}

impl TransitSocket {
    pub fn new(key: EncryptionKey) -> Self {
        let (tcp_data_assign, tcp_data_return) = broadcast::channel(10000);

        Self {
            key,
            client_statistics: ClientStatistics {
                to_send: 0,
                to_reply: 0,
            },
            socks_streams: DashMap::new(),
            tcp_data_return,
            tcp_data_assign,
        }
    }

    pub async fn process_socks_message(&mut self, message: SocksStreamInboundMessage) {
        match message {
            SocksStreamInboundMessage::UpStreamMessage(message) => {
                let socket_id = message.socket_id;
                
                // Check if the socket is already in the map
                if let Some(sender) = self.socks_streams.get(&socket_id) {
                    // Send the message to the task
                    sender.send(SocksStreamInboundMessage::UpStreamMessage(message)).await.unwrap();
                } else {
                    // Spawn a new task to handle this socket
                    // First create a channel to communicate with the task
                    // The reason this section can be slightly "implicit" is because we send dest ip and dest port in
                    let (sender, receiver) = mpsc::channel(10000);

                    // Insert the sender into the map
                    self.socks_streams.insert(socket_id, sender);

                    // Construct the arguments for the task
                    let arguments = TCPHandlerArguments {
                        receiver,
                        sender: self.tcp_data_assign.clone(),
                        address: message.dest_ip,
                        port: message.dest_port,
                    };

                    // Spawn the task
                    tokio::spawn(tcp_handler_task(arguments));
                }
            },
            SocksStreamInboundMessage::CloseSocketMessage(message) => {
                let socket_id = message.socket_id;
                
                // Re-form
                let message = SocksStreamInboundMessage::CloseSocketMessage(message);

                // Check if the socket is already in the map
                if let Some(sender) = self.socks_streams.get(&socket_id) {
                    // Send the message to the task
                    sender.send(message).await.unwrap();
                } else {
                    panic!("Unordered socket deletion: This should not happen because data should flow before close messages. Check process_sock_message feed order.")
                }
            },
        }
    }
}

struct TCPHandlerArguments {
    receiver: mpsc::Receiver<SocksStreamInboundMessage>,
    sender: broadcast::Sender<DownStreamMessage>,
    address: IPV4,
    port: Port,
}

// Constructed for each SOCKS TCP connection. Takes in a receiver for receiving upstream messages from clients, and a sender for sending downstream messages to clients.
// Remember: Incoming data may be out of order, so we need to check that it matches the sequence number.
// If it doesn't, we need to wait till it comes.
// For now, if data is lost this is going to stay open forever. In the future we should add client-side buffering of data and a "data-lost, please resend" message.
// However the lower level protocol should be reliable through request level retries so this should be rare-to-impossible.
// Though this does provide a DOS vector, so we should probably also add some sort of timeout. TODO
async fn tcp_handler_task(arguments: TCPHandlerArguments) {
    // TODO
}