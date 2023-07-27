use libsecrets::EncryptionKey;
use libtransit::{StreamMessage, CloseSocketMessage, SocketID, IPV4, Port};
use tokio::sync::{mpsc, broadcast};
use dashmap::DashMap;

pub struct ClientStatistics {
    pub to_send: usize,
    pub to_reply: usize,
}

#[derive(Debug)]
enum SocksStreamInboundMessage {
    StreamMessage(StreamMessage),
    CloseSocketMessage(CloseSocketMessage),
}

pub struct TransitSocket {
    pub key: EncryptionKey,
    pub client_statistics: ClientStatistics,
    // TODO information for communicating with SOCKS threads, returning
    // requests, etc. Need to figure out how to do this.
    socks_streams: DashMap<SocketID, mpsc::Sender<SocksStreamInboundMessage>>,
    tcp_data_return: broadcast::Receiver<StreamMessage>,
    tcp_data_assign: broadcast::Sender<StreamMessage>,
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
        // TODO
    }
}

struct TCPHandlerArguments {
    receiver: mpsc::Receiver<SocksStreamInboundMessage>,
    sender: broadcast::Sender<StreamMessage>,
    address: IPV4,
    port: Port,
}

async fn tcp_handler_task(arguments: TCPHandlerArguments) {
    // TODO
}