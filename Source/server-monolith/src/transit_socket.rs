use libsecrets::EncryptionKey;
use libtransit::{CloseSocketMessage, SocketID, IPV4, Port, UpStreamMessage, DownStreamMessage};
use tokio::sync::{mpsc, broadcast};
use dashmap::DashMap;

use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;
use tokio::io::AsyncReadExt;

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

                    let address = message.dest_ip;
                    let port = message.dest_port;

                    // Send the message to the task
                    sender.send(SocksStreamInboundMessage::UpStreamMessage(message)).await.unwrap();

                    // Insert the sender into the map
                    self.socks_streams.insert(socket_id, sender);

                    // Construct the arguments for the task
                    let arguments = TCPHandlerArguments {
                        receiver,
                        sender: self.tcp_data_assign.clone(),
                        address,
                        port,
                        socket_id,
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
    socket_id: SocketID,
}

// Constructed for each SOCKS TCP connection. Takes in a receiver for receiving upstream messages from clients, and a sender for sending downstream messages to clients.
// Remember: Incoming data may be out of order, so we need to check that it matches the sequence number.
// If it doesn't, we need to wait till it comes.
// For now, if data is lost this is going to stay open forever. In the future we should add client-side buffering of data and a "data-lost, please resend" message.
// However the lower level protocol should be reliable through request level retries so this should be rare-to-impossible.
// Though this does provide a DOS vector, so we should probably also add some sort of timeout. TODO
async fn tcp_handler_task(mut arguments: TCPHandlerArguments) {
    // Connect to the server
    let mut stream = match TcpStream::connect(format!("{}:{}", arguments.address, arguments.port)).await {
        Ok(stream) => stream,
        Err(_) => {
            // TODO handle this better - have an error and a way to send it back to the client
            panic!("Could not connect to server from TCP handler task to address {}:{}", arguments.address, arguments.port);
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
                },
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
                },
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
                continue
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
            },
            SocksStreamInboundMessage::CloseSocketMessage(_) => {
                // Close the socket
                stream.shutdown().await.unwrap();

                // TODO remove the socket from the map

                // Break out of the loop
                break
            }
        }

        // Read buffer
        let mut buffer = vec![0; 2048]; // TODO variable buffer size
        let bytes_read = match stream.read(&mut buffer).await {
            Ok(bytes_read) => bytes_read,
            Err(_) => {
                // TODO handle this better - have an error and a way to send it back to the client
                panic!("Could not read from server from TCP handler task to address {}:{}", arguments.address, arguments.port);
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
        SocksStreamInboundMessage::UpStreamMessage(interior) => {
            interior.message_sequence_number
        },
        SocksStreamInboundMessage::CloseSocketMessage(interior) => {
            interior.message_sequence_number
        }
    }
}