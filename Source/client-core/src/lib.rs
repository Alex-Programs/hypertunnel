use libsocks;
use libtransit::{UpStreamMessage, SocketID, SocksSocketDownstream};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::mpsc::error::TryRecvError;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::task;
use tokio::sync::broadcast::{self, Sender as BroadcastSender, Receiver as BroadcastReceiver};
use std::sync::Arc;
use tokio::io::{Interest, AsyncWriteExt, AsyncReadExt};
use tokio::sync::RwLock;

mod transit_builder;
use transit_builder::TransitSocketBuilder;
mod transit;
mod meta;
use meta::CLIENT_META_UPSTREAM;
use meta::YELLOW_DATA_UPSTREAM_QUEUE;

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
            task::spawn(tcp_listener(socket, upstream_passer_send.clone(), moved_passer_passer.clone()));
        }
    });

    transit::handle_transit(transit_socket, upstream_passer_receive, message_passer_passer_send.clone()).await;
}

#[allow(unused)]
async fn tcp_listener(stream: TcpStream, upstream_passer_send: Sender<UpStreamMessage>, message_passer_passer_send: Arc<BroadcastSender<transit::DownstreamBackpasser>>) {
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
    let (downstream_passer_send, mut downstream_passer_receive): (UnboundedSender<SocksSocketDownstream>, UnboundedReceiver<SocksSocketDownstream>) = mpsc::unbounded_channel();

    // Now we send the message passer to transit
    let message = transit::DownstreamBackpasser {
        socket_id,
        sender: downstream_passer_send
    };

    message_passer_passer_send.send(message).expect("Failed to send message passer to transit");

    let (mut read_half, mut write_half) = stream.into_split();

    // Spawn both tasks
    task::spawn(tcp_handler_up(read_half, socket_id, dstip, dstport, upstream_passer_send.clone()));
    task::spawn(tcp_handler_down(write_half, downstream_passer_receive, socket_id));
}

async fn tcp_handler_down(mut write_half: tokio::net::tcp::OwnedWriteHalf,
    mut downstream_passer_receive: UnboundedReceiver<SocksSocketDownstream>,
    socket_id: SocketID,
) {
    loop {
        let ready = write_half.ready(Interest::WRITABLE).await.expect("Failed to wait for socket to be ready");

        if ready.is_writable() {
            match downstream_passer_receive.recv().await {
                Some(data) => {
                    // Transit has sent us data
                    // Send it to the client
                    let bytes = &data.payload;
                    let length = bytes.len();

                    // Decrement counter
                    CLIENT_META_UPSTREAM.response_to_socks_bytes.fetch_sub(length as u32, Ordering::SeqCst);

                    match write_half.try_write(bytes) {
                        Ok(_) => {
                            // All is fine
                            dprintln!("Sent {} bytes to client", length);
                        },
                        Err(error) => {
                            // We can't write anymore. This should be propagated
                            // up the yellow route
                            yellow_route_record_error(socket_id, error.to_string()).await;
                        }
                    }
                },
                None => {
                    // Message passer has disconnected due to green route closing
                    // Simply close this half of the socket

                    write_half.shutdown().await.expect("Failed to shutdown socket at green route passer close");

                    // Now leave
                    return
                }
            }
        }
    }
}

async fn yellow_route_record_error(socket_id: SocketID, reason: String) {
    dprintln!("Yellow route error on socket id {} : {}", socket_id, reason);

    YELLOW_DATA_UPSTREAM_QUEUE.write().await.push(socket_id);
}

async fn tcp_handler_up(mut read_half: tokio::net::tcp::OwnedReadHalf,
    socket_id: SocketID,
    dest_ip: libsocks::IPV4,
    dest_port: libsocks::Port,
    upstream_passer_send: Sender<UpStreamMessage>,
) {
    let mut msg_seq_num = 0;

    loop {
        let ready = read_half.ready(Interest::READABLE).await.expect("Failed to wait for socket to be ready");

        if ready.is_readable() {
            let mut upstream_msg = UpStreamMessage {
                socket_id,
                dest_ip,
                dest_port,
                payload: vec![0; 512], // TODO changeable
                red_terminate: false,
            };

            let bytes_read = match read_half.read(&mut upstream_msg.payload).await {
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    // TODO handle properly
                    panic!("Failed to read from socket: {:?}", error);
                }
            };

            if bytes_read == 0 {
                // The socket was closed
                upstream_msg.red_terminate = true;
                upstream_passer_send.send(upstream_msg).await.expect("Failed to send data to transit");
                return
            }

            // Trim array to actual size
            upstream_msg.payload.truncate(bytes_read);

            // Send the data to transit
            CLIENT_META_UPSTREAM.socks_to_coordinator_bytes.fetch_add(bytes_read as u32, Ordering::Relaxed);
            upstream_passer_send.send(upstream_msg).await.expect("Failed to send data to transit");

            msg_seq_num += 1;
        }
    }
}

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_socket_id() -> libtransit::SocketID {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}