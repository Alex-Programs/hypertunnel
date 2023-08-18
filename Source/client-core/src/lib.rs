use libsocks;
use libtransit::{UpStreamMessage, CloseSocketMessage};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::mpsc::error::TryRecvError;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::task;
use tokio::sync::broadcast::{self, Sender as BroadcastSender, Receiver as BroadcastReceiver};
use std::sync::Arc;
use tokio::io::Interest;
use tokio::sync::RwLock;

mod transit_builder;
use transit_builder::TransitSocketBuilder;
mod transit;
mod meta;
use meta::CLIENT_META_UPSTREAM;

#[allow(unused)]
use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

pub struct ClientArguments {
    pub listen_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub password: String,
}

// Core of the client program for now. See Obsidian
pub async fn begin_core_client(arguments: ClientArguments) {
    let (upstream_passer_send, upstream_passer_receive): (
        Sender<UpStreamMessage>,
        Receiver<UpStreamMessage>,
    ) = mpsc::channel(10_000);

    let (close_passer_send, close_passer_receive): (Sender<CloseSocketMessage>, Receiver<CloseSocketMessage>) = mpsc::channel(10_000);

    let (message_passer_passer_send, _): (BroadcastSender<transit::DownstreamBackpasser>, BroadcastReceiver<transit::DownstreamBackpasser>) = broadcast::channel(100_000);
    
    let message_passer_passer_send = Arc::new(message_passer_passer_send);

    // Cannot transfer threads
    let transit_socket = TransitSocketBuilder::new()
        .with_target(arguments.target_host)
        .with_password(arguments.password)
        .with_client_name("Client-Core".to_string())
        .with_timeout_time(5 * 60) // Temporary till streaming is in - TODO
        .with_pull_client_count(8)
        .with_push_client_count(8)
        .build();

    let transit_socket = Arc::new(RwLock::new(transit_socket));

    let status = transit::connect(transit_socket.clone()).await;

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
    let moved_passer_passer = message_passer_passer_send.clone();
    task::spawn(async move {
        let listener = TcpListener::bind((arguments.listen_address, arguments.listen_port)).await.expect("Failed to start TCP listener");

        loop {
            let (socket, _) = listener.accept().await.expect("Failed to accept connection");

            // Spawn a task to handle the connection
            task::spawn(tcp_listener(socket, upstream_passer_send.clone(), close_passer_send.clone(), moved_passer_passer.clone()));
        }
    });

    transit::handle_transit(transit_socket, upstream_passer_receive, close_passer_receive, message_passer_passer_send.clone()).await;
}

#[allow(unused)]
async fn tcp_listener(stream: TcpStream, upstream_passer_send: Sender<UpStreamMessage>, close_passer_send: Sender<CloseSocketMessage>, message_passer_passer_send: Arc<BroadcastSender<transit::DownstreamBackpasser>>) {
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
                        stream.writable().await.unwrap();
                        stream.try_write(&rejection_bytes).unwrap();
                        return;
                    }

                    dstip = Some(connect_req.dstip);
                    dstport = Some(connect_req.dstport);
                },
                libsocks::Socks4Request::Bind(bind_req) => {
                    // We don't support bind yet
                    let user_id = bind_req.userid;
                    println!("Received bind request (unsupported) from user ID {}. Killing connection", user_id);
                    stream.writable().await.unwrap();
                    stream.try_write(&rejection_bytes).unwrap();
                    return;
                }
            }
        },
        Err(error) => {
            println!("Failed to parse SOCKS request: {:?}", error);
            match stream.writable().await {
                Ok(_) => {
                    stream.try_write(&rejection_bytes).unwrap();
                },
                Err(error) => {
                    println!("Failed to send rejection to client: {:?}", error);
                }
            };
            match stream.try_write(&rejection_bytes) {
                Ok(_) => {
                    println!("Sent rejection to client");
                },
                Err(error) => {
                    println!("Failed to send rejection to client: {:?}", error);
                }
            }
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
    let (downstream_passer_send, mut downstream_passer_receive): (UnboundedSender<libtransit::DownStreamMessage>, UnboundedReceiver<libtransit::DownStreamMessage>) = mpsc::unbounded_channel();

    // Now we send the message passer to transit
    let message = transit::DownstreamBackpasser {
        socket_id,
        sender: downstream_passer_send
    };

    message_passer_passer_send.send(message).expect("Failed to send message passer to transit");

    let mut send_seq_num = 0;

    loop {
        let mut is_writeable = false;
        let mut is_readable = false;

        let ready = stream.ready(Interest::READABLE | Interest::WRITABLE).await.expect("Failed to wait for socket to be ready");

        if ready.is_writable() {
            // Check if transit has sent us any data
            dprintln!("Checking if we can send data as reported writeable");
            match downstream_passer_receive.try_recv() {
                Ok(data) => {
                    dprintln!("Sending data to client as reported writeable and have gotten data");

                    // Transit has sent us data
                    // Send it to the client
                    let bytes = data.payload;
                    let length = bytes.len();

                    // Decrement counter
                    CLIENT_META_UPSTREAM.response_to_socks_bytes.fetch_sub(length as u32, Ordering::SeqCst);

                    is_writeable = true;

                    match stream.try_write(&bytes) {
                        Ok(_) => {
                            // All is fine
                            dprintln!("Sent {} bytes to client", length);
                        },
                        Err(error) => {
                            // TODO handle properly
                            eprintln!("Failed to send data to client: {:?}", error);
                            continue
                        }
                    }
                },
                Err(error) => {
                    dprintln!("Wanted to send data to client as reported writeable but failed to receive data from transit: {:?}", error);
                    match error {
                        TryRecvError::Empty => {
                            // No data from transit
                            // Wait a moment because async
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            // DO NOT continue, we still need to read
                        },
                        TryRecvError::Disconnected => {
                            // Transit has disconnected
                            // TODO handle properly
                            eprintln!("Transit has disconnected; socket cannot be sustained");
                            return
                        }
                        _ => {
                            // TODO handle properly
                            eprintln!("Failed to receive data from transit: {:?}", error);
                            continue
                        }
                    };
                }
            };
        } else {
            dprintln!("Not checking if we can send data as not reported writeable")
        }

        if ready.is_readable() {
            let mut upstream_packet = UpStreamMessage {
                socket_id,
                message_sequence_number: send_seq_num,
                dest_ip: dstip,
                dest_port: dstport,
                payload: Vec::with_capacity(0),
                time_ingress_client_ms: meta::ms_since_epoch(),
                time_at_coordinator_ms: 0,
                time_at_client_egress_ms: 0,
                time_at_server_ingress_ms: 0,
                time_at_server_coordinator_ms: 0,
                time_at_server_socket_ms: 0,
                time_client_write_finished_ms: 0,
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

            is_readable = true;

            // Check if the socket was closed
            if bytes_read == 0 {
                // The socket was closed
                // TODO handle properly
                return
            }

            dprintln!("Read {} bytes from socket", bytes_read);

            // Send the data to transit
            CLIENT_META_UPSTREAM.socks_to_coordinator_bytes.fetch_add(bytes_read as u32, Ordering::Relaxed);
            upstream_passer_send.send(upstream_packet).await.expect("Failed to send data to transit");

            send_seq_num += 1;
        }

        if !is_readable && !is_writeable {
            // Wait a moment because async
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }
}

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_socket_id() -> libtransit::SocketID {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}