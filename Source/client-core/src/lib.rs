use libsocks;
use libtransit::{UpStreamMessage, SocketID, SocksSocketDownstream};
use tokio::net::{TcpListener, TcpStream};
#[allow(unused)] // Not unused - the warning is wrong
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
use meta::YELLOW_DATA_UPSTREAM_QUEUE;

#[allow(unused)]
use log::{debug, error, info, warn};

pub struct ClientArguments {
    pub listen_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub password: String,
    pub push_client_count: usize,
    pub pull_client_count: usize,
    pub timeout_time_s: usize,
    pub client_name: String,
}

// Core of the client program for now. See Obsidian
pub async fn begin_core_client(arguments: ClientArguments) {
    let (upstream_passer_send, upstream_passer_receive): (
        Sender<UpStreamMessage>,
        Receiver<UpStreamMessage>,
    ) = mpsc::channel(10_000);

    let (message_passer_passer_send, _): (BroadcastSender<transit::DownstreamBackpasser>, BroadcastReceiver<transit::DownstreamBackpasser>) = broadcast::channel(100_000);
    
    let message_passer_passer_send = Arc::new(message_passer_passer_send);

    let (blue_terminate_send, _): (BroadcastSender<SocketID>, BroadcastReceiver<SocketID>) = broadcast::channel(100_000);

    // Cannot transfer threads
    let transit_socket = TransitSocketBuilder::new()
        .with_target(arguments.target_host)
        .with_password(arguments.password)
        .with_client_name(arguments.client_name)
        .with_timeout_time(arguments.timeout_time_s)
        .with_pull_client_count(arguments.pull_client_count)
        .with_push_client_count(arguments.push_client_count)
        .build();

    let transit_socket = Arc::new(RwLock::new(transit_socket));

    let status = transit::connect(transit_socket.clone()).await;

    match status {
        Ok(_) => {
            info!("Connected to server!");
        }
        Err(error) => {
            error!("Failed to connect to server: {:?}", error);
            return;
        }
    }

    // Now start the TCP listener in a task
    let moved_passer_passer = message_passer_passer_send.clone();
    
    let blue_terminate_for_listener = blue_terminate_send.clone();
    task::spawn(async move {
        let listener = TcpListener::bind((arguments.listen_address, arguments.listen_port)).await.expect("Failed to start TCP listener");

        loop {
            let (socket, _) = listener.accept().await.expect("Failed to accept connection");

            // Spawn a task to handle the connection
            task::spawn(tcp_listener(socket, upstream_passer_send.clone(), moved_passer_passer.clone(), blue_terminate_for_listener.clone()));
        }
    });

    transit::handle_transit(transit_socket, upstream_passer_receive, message_passer_passer_send.clone(), blue_terminate_send).await;
}

#[allow(unused)]
async fn tcp_listener(mut stream: TcpStream, upstream_passer_send: Sender<UpStreamMessage>, message_passer_passer_send: Arc<BroadcastSender<transit::DownstreamBackpasser>>, blue_terminate_send: BroadcastSender<SocketID>) {
    const MAX_SOCKS_REQUEST_LENGTH: usize = 4096;

    // Read the first packet
    // Wait for the socket to be readable
    let mut buf = Vec::with_capacity(4096);
    let mut fixed_header = [0_u8; 8];

    if let Err(error) = stream.read_exact(&mut fixed_header).await {
        info!("Failed to read SOCKS request header: {:?}", error);
        return;
    }
    buf.extend_from_slice(&fixed_header);

    loop {
        if buf.len() >= MAX_SOCKS_REQUEST_LENGTH {
            warn!("SOCKS request exceeded {} bytes", MAX_SOCKS_REQUEST_LENGTH);
            return;
        }

        let byte = match stream.read_u8().await {
            Ok(byte) => byte,
            Err(error) => {
                info!("Failed to read SOCKS user ID: {:?}", error);
                return;
            }
        };

        buf.push(byte);
        if byte == 0 {
            break;
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
                        warn!("Incorrect socks version ({}) from program {}", connect_req.version, connect_req.userid);
                        if let Err(error) = stream.write_all(&rejection_bytes).await {
                            error!("Failed to send SOCKS rejection: {:?}", error);
                        }
                        return;
                    }

                    dstip = Some(connect_req.dstip);
                    dstport = Some(connect_req.dstport);
                },
                libsocks::Socks4Request::Bind(bind_req) => {
                    // We don't support bind yet
                    let user_id = bind_req.userid;
                    error!("Received bind request (unsupported) from user ID {}. Killing connection", user_id);
                    if let Err(error) = stream.write_all(&rejection_bytes).await {
                        error!("Failed to send SOCKS rejection: {:?}", error);
                    }
                    return;
                }
            }
        },
        Err(error) => {
            error!("Failed to parse SOCKS request: {:?}", error);
            if let Err(error) = stream.write_all(&rejection_bytes).await {
                error!("Failed to send SOCKS rejection: {:?}", error);
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

    if let Err(error) = stream.write_all(&reply.to_binary()).await {
        error!("Failed to send SOCKS acceptance: {:?}", error);
        return;
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
    task::spawn(tcp_handler_up(read_half, socket_id, dstip, dstport, upstream_passer_send.clone(), blue_terminate_send.subscribe()));
    task::spawn(tcp_handler_down(write_half, downstream_passer_receive, socket_id));
}

async fn tcp_handler_down(mut write_half: tokio::net::tcp::OwnedWriteHalf,
    mut downstream_passer_receive: UnboundedReceiver<SocksSocketDownstream>,
    socket_id: SocketID,
) {
    while let Some(data) = downstream_passer_receive.recv().await {
        if !data.payload.is_empty() {
            if let Err(error) = write_half.write_all(&data.payload).await {
                if !data.do_green_terminate {
                    yellow_route_record_error(socket_id, error.to_string()).await;
                }
                return;
            }
            debug!("Sent {} bytes to client", data.payload.len());
        }

        if data.do_green_terminate {
            debug!("Closing writer for id {} due to green terminate", socket_id);
            return;
        }
    }

    debug!("Closing writer for id {} because its downstream channel closed", socket_id);
    if let Err(error) = write_half.shutdown().await {
        debug!("Failed to shut down writer for id {}: {:?}", socket_id, error);
    }
}

async fn yellow_route_record_error(socket_id: SocketID, reason: String) {
    debug!("Yellow route error on socket id {} : {}", socket_id, reason);

    YELLOW_DATA_UPSTREAM_QUEUE.write().await.push(socket_id);
}

async fn tcp_handler_up(mut read_half: tokio::net::tcp::OwnedReadHalf,
    socket_id: SocketID,
    dest_ip: libsocks::IPV4,
    dest_port: libsocks::Port,
    upstream_passer_send: Sender<UpStreamMessage>,
    mut blue_terminate_receive: BroadcastReceiver<SocketID>
) {
    // Send the first msg in order to get the socket open
    let upstream_msg = UpStreamMessage {
        socket_id,
        dest_ip,
        dest_port,
        payload: Vec::with_capacity(0),
        red_terminate: false,
    };

    upstream_passer_send.send(upstream_msg).await.expect("Failed to send initialising socket open data to transit");

    loop {
        let ready = read_half.ready(Interest::READABLE).await.expect("Failed to wait for socket to be ready");

        while let Ok(term_socket_id) = blue_terminate_receive.try_recv() {
            if term_socket_id == socket_id {
                // We've been told to close
                debug!("Closing reader for id {} due to blue terminate", socket_id);
                return
            }
        }        

        if ready.is_readable() {
            let mut upstream_msg = UpStreamMessage {
                socket_id,
                dest_ip,
                dest_port,
                payload: vec![0; 512], // TODO changeable, try to reuse buffers
                red_terminate: false,
            };

            let bytes_read = match read_half.read(&mut upstream_msg.payload).await {
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    upstream_msg.red_terminate = true;
                    upstream_msg.payload = Vec::with_capacity(0);
                    upstream_passer_send.send(upstream_msg).await.expect("Failed to send data to transit");
                    debug!("Failed to read from socket: {:?}", error);
                    return;
                }
            };

            if bytes_read == 0 {
                // The socket was closed
                upstream_msg.red_terminate = true;
                upstream_msg.payload = Vec::with_capacity(0);
                upstream_passer_send.send(upstream_msg).await.expect("Failed to send data to transit");
                return
            }

            // Trim array to actual size
            upstream_msg.payload.truncate(bytes_read);

            upstream_passer_send.send(upstream_msg).await.expect("Failed to send data to transit");
        }
    }
}

static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_socket_id() -> libtransit::SocketID {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst)
}
