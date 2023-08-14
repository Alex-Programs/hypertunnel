use client_transit;
use libsocks;
use libtransit::{UpStreamMessage, CloseSocketMessage};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::mpsc::error::TryRecvError;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::task;
use flume::{Sender as FlumeSender, Receiver as FlumeReceiver, self};
use tokio::sync::broadcast::{self, Sender as BroadcastSender, Receiver as BroadcastReceiver};
use std::sync::Arc;
use tokio::io::Interest;
use tokio::sync::RwLock;

pub struct ClientArguments {
    pub listen_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub password: String,
}

// Core of the client program for now. See Obsidian
pub async fn begin_core_client(arguments: ClientArguments) {
    let (upstreamPasserSend, upstreamPasserReceive): (
        Sender<UpStreamMessage>,
        Receiver<UpStreamMessage>,
    ) = mpsc::channel(10_000);

    let (closePasserSend, closePasserReceive): (Sender<CloseSocketMessage>, Receiver<CloseSocketMessage>) = mpsc::channel(10_000);

    let (messagePasserPasserSend, messagePasserPasserReceive): (BroadcastSender<client_transit::DownstreamBackpasser>, BroadcastReceiver<client_transit::DownstreamBackpasser>) = broadcast::channel(100_000);
    
    let messagePasserPasserSend = Arc::new(messagePasserPasserSend);
    let mut messagePasserPasserReceive = Arc::new(messagePasserPasserReceive);

    // Cannot transfer threads
    let mut transit_socket = client_transit::TransitSocketBuilder::new()
        .with_target(arguments.target_host)
        .with_password(arguments.password)
        .with_client_name("Client-Core".to_string())
        .with_timeout_time(5 * 60) // Temporary till streaming is in - TODO
        .build();

    let mut transit_socket = Arc::new(RwLock::new(transit_socket));

    let status = client_transit::connect(transit_socket.clone()).await;

    match status {
        Ok(_) => {
            println!("Connected to server!");
        }
        Err(error) => {
            println!("Failed to connect to server: {:?}", error);
            return;
        }
    }

    // Now start the TCP listener in a task
    let movedPasserPasser = messagePasserPasserSend.clone();
    task::spawn(async move {
        let listener = TcpListener::bind((arguments.listen_address, arguments.listen_port)).await.expect("Failed to start TCP listener");

        loop {
            let (socket, _) = listener.accept().await.expect("Failed to accept connection");

            // Spawn a task to handle the connection
            task::spawn(tcp_listener(socket, upstreamPasserSend.clone(), closePasserSend.clone(), movedPasserPasser.clone()));
        }
    });

    client_transit::handle_transit(transit_socket, upstreamPasserReceive, closePasserReceive, messagePasserPasserSend.clone()).await;
}

async fn tcp_listener(stream: TcpStream, upstreamPasserSend: Sender<UpStreamMessage>, closePasserSend: Sender<CloseSocketMessage>, messagePasserPasserSend: Arc<BroadcastSender<client_transit::DownstreamBackpasser>>) {
    // Read the first packet
    // Wait for the socket to be readable
    let mut buf = Vec::with_capacity(4096);

    stream.readable().await.expect("Failed to wait for socket to be readable");

    match stream.try_read_buf(&mut buf) {
        Ok(bytes_read) => {
            if bytes_read == 0 {
                // The socket was closed
                return;
            }
        },
        Err(error) => {
            println!("Failed to read from socket in initial SOCKS discussion: {:?}", error);
            return;
        }
    }

    // Try to parse the packet
    let packet = libsocks::decode_socks_request(&buf);

    let mut dstip: Option<libsocks::IPV4> = None;
    let mut dstport: Option<libsocks::Port> = None;

    let rejection = libsocks::Socks4ConnectReply {
        version: 0, // yes, this is correct
        status: libsocks::Socks4Status::Rejected,
        dstport: 0,
        dstip: 0,
    };

    let rejection_bytes = rejection.to_binary();

    match packet {
        Ok(packet) => {
            // Check what kind of packet it is
            match packet {
                libsocks::Socks4Request::Connect(connect_req) => {
                    // Sanity check
                    if connect_req.version != 4 {
                        println!("Incorrect socks version ({}) from program {}", connect_req.version, connect_req.userid);
                        stream.writable().await;
                        stream.try_write(&rejection_bytes);
                        return;
                    }

                    dstip = Some(connect_req.dstip);
                    dstport = Some(connect_req.dstport);
                },
                libsocks::Socks4Request::Bind(bind_req) => {
                    // We don't support bind yet
                    let user_id = bind_req.userid;
                    println!("Received bind request (unsupported) from user ID {}. Killing connection", user_id);
                    stream.writable().await;
                    stream.try_write(&rejection_bytes);
                    return;
                }
            }
        },
        Err(error) => {
            println!("Failed to parse SOCKS request: {:?}", error);
            stream.writable().await;
            stream.try_write(&rejection_bytes);
            return;
        }
    }

    let (dstip, dstport) = (dstip.unwrap(), dstport.unwrap());

    // If we've gotten to this point the data's good. Let's compose a reply
    let reply = libsocks::Socks4ConnectReply {
        version: 0,
        status: libsocks::Socks4Status::Granted,
        dstport,
        dstip,
    };

    stream.writable().await.unwrap();

    match stream.try_write(&reply.to_binary()) {
        Ok(_) => {
            println!("Said hello to client");
        },
        Err(error) => {
            println!("Failed to send reply to client: {:?}", error);
            return;
        }
    }

    // It seems initialisation was a success. Let's get our socket ID
    let socket_id = allocate_socket_id();

    // Now we need to let transit know how to reply to this socket. First we create a message passer
    let (downstreamPasserSend, mut downstreamPasserReceive): (Sender<libtransit::DownStreamMessage>, Receiver<libtransit::DownStreamMessage>) = mpsc::channel(100);

    // Now we send the message passer to transit
    let message = client_transit::DownstreamBackpasser {
        socket_id,
        sender: downstreamPasserSend
    };

    messagePasserPasserSend.send(message).expect("Failed to send message passer to transit");

    let mut send_seq_num = 0;

    loop {
        let ready = stream.ready(Interest::READABLE | Interest::WRITABLE).await.expect("Failed to wait for socket to be ready");

        if ready.is_writable() {
            // Check if transit has sent us any data
            match downstreamPasserReceive.try_recv() {
                Ok(data) => {
                    // Transit has sent us data
                    // Send it to the client
                    let bytes = data.payload;

                    match stream.try_write(&bytes) {
                        Ok(_) => {
                            // Transit has sent us data
                            send_seq_num += 1;
                        },
                        Err(error) => {
                            // TODO handle properly
                            eprintln!("Failed to send data to client: {:?}", error);
                            continue
                        }
                    }
                },
                Err(error) => {
                    match error {
                        TryRecvError::Empty => {
                            // No data from transit
                            // Wait a moment because async
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            // DO NOT continue, we still need to read
                        },
                        _ => {
                            // TODO handle properly
                            eprintln!("Failed to receive data from transit: {:?}", error);
                            continue
                        }
                    };
                }
            };
        }

        if ready.is_readable() {
            let mut upstream_packet = UpStreamMessage {
                socket_id,
                message_sequence_number: send_seq_num,
                dest_ip: dstip,
                dest_port: dstport,
                payload: Vec::with_capacity(0)
            };

            // Read into the payload buffer
            let bytes_read = match stream.try_read_buf(&mut upstream_packet.payload) {
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    // TODO handle properly
                    eprintln!("Failed to read from socket: {:?}", error);
                    continue
                }
            };

            // Check if the socket was closed
            if bytes_read == 0 {
                // The socket was closed
                // TODO handle properly
                return
            }

            println!("Read {} bytes from socket", bytes_read);

            // Send the data to transit
            upstreamPasserSend.send(upstream_packet).await.expect("Failed to send data to transit");

            println!("Sent on to transit passer");
        }
    }
}

async fn send_socks_to_transit(stream: &mut TcpStream, upstream_passer: Sender<UpStreamMessage>, close_passer: Sender<CloseSocketMessage>,socket_id: libtransit::SocketID, ip: libsocks::IPV4, port: libsocks::Port) {
    let mut buf = Vec::with_capacity(4096);

    let mut seq_num = 0;
    loop {
        let bytes_read = stream.try_read_buf(&mut buf).expect("Failed to read from socket");

        // Check if the socket was closed
        if bytes_read == 0 {
            // The socket was closed
            close_passer.send(CloseSocketMessage {
                socket_id,
                message_sequence_number: seq_num
            }).await.expect("Failed to send close message to transit");
            return;
        }

        // Send the data to transit
        upstream_passer.send(UpStreamMessage {
            socket_id,
            message_sequence_number: seq_num,
            dest_ip: ip,
            dest_port: port,
            payload: buf.clone()
        }).await.expect("Failed to send data to transit");

        // Blank the buffer
        buf.clear();

        seq_num += 1;
    }
}

async fn send_transit_to_socks(stream: &mut TcpStream, mut downstream_passer_rcv: Receiver<libtransit::DownStreamMessage>) {
    let mut seq_num = 0;
    loop {
        let message = downstream_passer_rcv.recv().await.expect("Failed to receive message from transit");
        
        debug_assert!(message.message_sequence_number == seq_num, "Received message with incorrect sequence number (expected {}, got {})", seq_num, message.message_sequence_number);

        seq_num += 1;

        // Send the data to the socket
        stream.try_write(&message.payload).expect("Failed to write to socket");
    }
}

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_socket_id() -> libtransit::SocketID {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}