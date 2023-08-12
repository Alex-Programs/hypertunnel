use client_transit;
use libsocks;
use libtransit::UpStreamMessage;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::task;

struct ClientArguments {
    listen_address: String,
    listen_port: u16,
    target_host: String,
    password: String,
}

// Core of the client program for now. See Obsidian
async fn begin_core_client(arguments: ClientArguments) {
    let (upstreamPasserSend, upstreamPasserReceive): (
        Sender<UpStreamMessage>,
        Receiver<UpStreamMessage>,
    ) = mpsc::channel(100);

    let (messagePasserPasserSend, messagePasserPasserReceive): (
        Sender<client_transit::DownstreamBackpasser>,
        Receiver<client_transit::DownstreamBackpasser>,
    ) = mpsc::channel(100_00);

    // Cannot transfer threads
    let mut transit_socket = client_transit::TransitSocketBuilder::new()
        .with_target(arguments.target_host)
        .with_password(arguments.password)
        .with_client_name("Client-Core".to_string())
        .build();

    let status = transit_socket.connect().await;

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
    task::spawn(async move {
        let listener = TcpListener::bind((arguments.listen_address, arguments.listen_port)).await.expect("Failed to start TCP listener");

        loop {
            let (socket, _) = listener.accept().await.expect("Failed to accept connection");
        }
    });

    transit_socket
        .handle_transit(upstreamPasserReceive, messagePasserPasserReceive)
        .await;
}

async fn tcp_listener(stream: TcpStream, upstreamPasserSend: Sender<UpStreamMessage>, messagePasserPasserSend: Sender<client_transit::DownstreamBackpasser>) {
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
            println!("Failed to read from socket: {:?}", error);
            return;
        }
    }

    // Try to parse the packet
    let packet = libsocks::decode_socks_request(&buf);

    let mut dstip: Option<libsocks::IPV4> = None;
    let mut dstport: Option<libsocks::Port> = None;

    let rejection = libsocks::Socks4ConnectReply {
        version: 4,
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

    // If we've gotten to this point the data's good. Let's compose a reply
    let reply = libsocks::Socks4ConnectReply {
        version: 4,
        status: libsocks::Socks4Status::Granted,
        dstport: dstport.unwrap(),
        dstip: dstip.unwrap()
    };

    stream.writable().await.unwrap();

    match stream.try_write(&reply.to_binary()) {
        Ok(_) => {
            println!("Sent reply to client");
        },
        Err(error) => {
            println!("Failed to send reply to client: {:?}", error);
            return;
        }
    }

    // It seems initialisation was a success. Let's get our socket ID
    let socket_id = allocate_socket_id();

    // Now we need to let transit know how to reply to this socket. First we create a message passer
    let (downstreamPasserSend, downstreamPasserReceive): (Sender<libtransit::DownStreamMessage>, Receiver<libtransit::DownStreamMessage>) = mpsc::channel(100);

    // Now we send the message passer to transit
    let message = client_transit::DownstreamBackpasser {
        socket_id,
        sender: downstreamPasserSend
    };

    messagePasserPasserSend.send(message).await.expect("Failed to send message passer to transit");

    // Now go into the constant loop of message passing
    // TODO
}

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_socket_id() -> libtransit::SocketID {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}