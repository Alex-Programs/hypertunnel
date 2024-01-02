use flume::{self, Receiver as FlumeReceiver, Sender as FlumeSender};
use libsecrets::{self, EncryptionKey};
use libtransit::{self, UnifiedPacketInfo, SocksSocketUpstream, SocksSocketDownstream};
use libtransit::{
    ClientMessageUpstream, ClientMetaUpstream, DeclarationToken, DownStreamMessage,
    ServerMessageDownstream, ServerMetaDownstream, SocketID, UpStreamMessage,
};
use rand::Rng;
use reqwest::Client;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::mpsc::error::TryRecvError;

use reqwest::header::{HeaderMap, HeaderValue};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast::{self, Receiver as BroadcastReceiver, Sender as BroadcastSender};
use tokio::sync::RwLock;
use tokio::task;

use std::collections::HashMap;

static RECEIVED_SEQ_NUM: AtomicU32 = AtomicU32::new(0);
static SENT_SEQ_NUM: AtomicU32 = AtomicU32::new(0);

use crate::meta::{CLIENT_META_UPSTREAM, ms_since_epoch, YELLOW_DATA_UPSTREAM_QUEUE};

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

pub struct TransitSocket {
    pub target: String,                      // Base URL, incl. protocol
    pub key: EncryptionKey,                  // Encryption key for libsecrets
    pub client_identifier: DeclarationToken, // Identifier for this client, used as a cookie
    pub push_client_count: usize,            // Number of hybrid clients (Both push and pull)
    pub pull_client_count: usize,            // Number of pull clients
    pub timeout_time: usize,                 // Time in seconds before a request is considered timed out
    pub headers: HeaderMap,                  // Headers to send with requests. Includes client identifier
    pub is_initialized: bool,                // Whether the socket has been initialized by greeting the server
    pub client_name: String,                 // Name of the client
}

#[derive(Debug)]
pub enum TransitInitError {
    EncryptionError(libsecrets::EncryptionError),
    RequestError(reqwest::Error),
    ConnectionFailed,
    ConnectionDenied,
    LiedResponse,
}

impl From<libsecrets::EncryptionError> for TransitInitError {
    fn from(error: libsecrets::EncryptionError) -> Self {
        Self::EncryptionError(error)
    }
}

#[derive(Clone, Debug)]
pub struct DownstreamBackpasser {
    pub socket_id: libtransit::SocketID,
    pub sender: UnboundedSender<SocksSocketDownstream>,
}

pub async fn connect(transit_socket: Arc<RwLock<TransitSocket>>) -> Result<(), TransitInitError> {
    greet_server(transit_socket.clone()).await?;
    transit_socket.write().await.is_initialized = true;
    Ok(())
}

// TODO this is unusual traffic, not very believable.
// Change this to "Get a PNG image" incl. header etc, with the
// encrypted data being some form of additional token in the cookies
async fn greet_server(transit_socket: Arc<RwLock<TransitSocket>>) -> Result<(), TransitInitError> {
    let mut data = format!(
        "Hello. Protocol version: {}, client-transit version: {}, client-name: {}",
        "0",
        env!("CARGO_PKG_VERSION"),
        transit_socket.read().await.client_name
    );

    // Find minimum padding amount such that it's at least 512 bytes
    let length = data.len();
    let min_padding = 512 - length;

    // Find maximum padding amount such that it's at most 1024 bytes
    let max_padding = 1024 - length;

    // Add random padding up to 1024 bytes
    let mut rng = rand::thread_rng();
    let amount = rng.gen_range(min_padding..max_padding);
    data = data + &" ".repeat(amount);

    let encrypted = libsecrets::encrypt(data.as_bytes(), &transit_socket.read().await.key)?;

    let (target, headers, key, timeout_time) = {
        let transit_socket = transit_socket.read().await;
        (
            transit_socket.target.clone(),
            transit_socket.headers.clone(),
            transit_socket.key.clone(),
            transit_socket.timeout_time,
        )
    };

    let response = Client::new()
        .post(&format!("{}/submit", target))
        .body(encrypted)
        .headers(headers)
        .timeout(Duration::from_secs(timeout_time as u64))
        .send()
        .await;

    match response {
        Ok(response) => {
            if response.status() != 200 {
                return Err(TransitInitError::ConnectionDenied);
            }
            // Try to decrypt the response
            let encrypted = response.bytes().await;
            match encrypted {
                Ok(encrypted) => {
                    let decrypted = libsecrets::decrypt(&encrypted, &key)?;
                    // Convert to string
                    let decrypted = String::from_utf8(decrypted);
                    match decrypted {
                        Ok(decrypted) => {
                            // Remove spaces
                            let decrypted = decrypted.trim_end_matches(' ');
                            if decrypted != "CONNECTION ACCEPTED" {
                                return Err(TransitInitError::LiedResponse);
                            }
                        }
                        Err(_) => {
                            return Err(TransitInitError::LiedResponse);
                        }
                    }
                }
                Err(error) => {
                    return Err(TransitInitError::RequestError(error));
                }
            }
        }
        Err(error) => {
            if error.is_timeout() || error.is_connect() {
                return Err(TransitInitError::ConnectionFailed);
            } else {
                return Err(TransitInitError::RequestError(error));
            }
        }
    }

    // If we've gotten here, the connection was successful
    // We should be ready to talk to the server now

    Ok(())
}

async fn push_handler(
    transit_socket: Arc<RwLock<TransitSocket>>,
    to_send_passer: FlumeReceiver<Vec<SocksSocketUpstream>>,
) {
    // Create client
    let client = Client::new();

    let (key, headers, target, timeout_time) = {
        let transit_socket = transit_socket.read().await;
        (
            transit_socket.key.clone(),
            transit_socket.headers.clone(),
            transit_socket.target.clone(),
            transit_socket.timeout_time,
        )
    };

    let timeout_duration = Duration::from_secs(timeout_time as u64);

    dprintln!("Push handler started");

    loop {
        // Get the data to send
        let to_send = to_send_passer.recv_async().await;

        let to_send = match to_send {
            Ok(to_send) => to_send,
            Err(_) => {
                // TODO handle this
                to_send.unwrap();
                continue;
            }
        };

        // Get the size of the multiple messages
        let payload_size = to_send.iter().map(|x| x.payload.len() as u32).sum::<u32>();

        // Decrement the counter
        CLIENT_META_UPSTREAM.coordinator_to_request_channel_bytes.fetch_sub(payload_size, Ordering::Relaxed);

        // Increment RIP
        CLIENT_META_UPSTREAM.up_request_in_progress_bytes.fetch_add(payload_size, Ordering::Relaxed);

        // Send the data
        // Encode and encrypt in a thread
        let upstream_message = ClientMessageUpstream {
            socks_sockets: to_send,
            metadata: get_metadata(SENT_SEQ_NUM.fetch_add(1, Ordering::SeqCst)).await,
            payload_size,
        };

        let to_send = tokio::task::spawn_blocking(move || {
            let data_bin = upstream_message.encoded().unwrap();

            let encrypted = libsecrets::encrypt(&data_bin, &key).unwrap();

            encrypted
        });

        // Send the data
        let response = client
            .post(&format!("{}/upload", target))
            .body(to_send.await.unwrap())
            .headers(headers.clone())
            .timeout(timeout_duration) // TODO RM
            .send()
            .await;

        // Decrement RIP
        CLIENT_META_UPSTREAM.up_request_in_progress_bytes.fetch_sub(payload_size, Ordering::Relaxed);

        let response = response.unwrap();

        dprintln!("Sent! Response code {:?}, data: {:?}", response.status(), response.bytes().await);

        // TODO if things go wrong, prevent the arrival of new data until the send works. See working nicely section of todo about reconstruction and error handling.
    }
}

async fn get_metadata(seq_num: u32) -> ClientMetaUpstream {
    let yellow_to_stop_reading_from = {
        let mut data = YELLOW_DATA_UPSTREAM_QUEUE.write().await;

        let to_return = data.clone();

        // Clear the queue
        data.clear();

        to_return
    };

    let data = ClientMetaUpstream {
        packet_info: UnifiedPacketInfo {
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            seq_num,
        },
        set: None,
        traffic_stats: CLIENT_META_UPSTREAM.as_base(),
        yellow_to_stop_reading_from,
    };

    dprintln!("Sending metadata: {:?}", data);

    data
}

async fn pull_handler(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut message_passer_passer: BroadcastReceiver<DownstreamBackpasser>,
) {
    // This will hold the return lookup and be populated by the messagePasserPassers
    let mut return_lookup: HashMap<SocketID, UnboundedSender<SocksSocketDownstream>> = HashMap::new();

    // Create client
    let client = Client::new();

    let (key, headers, target, timeout_time) = {
        let transit_socket = transit_socket.read().await;
        (
            transit_socket.key.clone(),
            transit_socket.headers.clone(),
            transit_socket.target.clone(),
            transit_socket.timeout_time,
        )
    };

    let timeout_duration = Duration::from_secs(timeout_time as u64);

    loop {
        // Could be made spawn_blocking if this turns out to take too long
        let upstream_message = ClientMessageUpstream {
            socks_sockets: Vec::with_capacity(0),
            metadata: get_metadata(0).await,
            payload_size: 0,
        };

        let to_send = {
            let data_bin = upstream_message.encoded().unwrap();

            let encrypted = libsecrets::encrypt(&data_bin, &key).unwrap();

            encrypted
        };

        // Send the data
        let response = client
            .get(&format!("{}/download", target))
            .body(to_send)
            .headers(headers.clone())
            .timeout(timeout_duration)
            .send()
            .await;

        // Read the response
        let response = response.unwrap();

        dprintln!("Pull response status: {}", response.status());

        // Get the data
        let encrypted = response.bytes().await.unwrap();

        // Decrypt the data
        let mut decrypted = libsecrets::decrypt(&encrypted, &key).unwrap();

        // Decode the data
        let decoded = ServerMessageDownstream::decode_from_bytes(&mut decrypted).unwrap();

        let seq_num = decoded.metadata.packet_info.seq_num;

        while seq_num != RECEIVED_SEQ_NUM.load(Ordering::SeqCst) {
            // Wait until the seq_num is the one we're expecting
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        // Set the seq_num to the next one
        RECEIVED_SEQ_NUM.fetch_add(1, Ordering::SeqCst);

        for mut socket in decoded.socks_sockets {
            let socket_id = socket.socket_id;

            // Get the sender
            let sender = return_lookup.get(&socket_id);

            match sender {
                Some(sender) => {
                    let size = socket.payload.len() as u32;
                    // Increment the counter
                    CLIENT_META_UPSTREAM.response_to_socks_bytes.fetch_add(size, Ordering::SeqCst);

                    // Send the message
                    sender.send(socket).unwrap();
                }
                None => {
                    // Populate the sender via iteration through the messagePasserPasser
                    for _ in 0..message_passer_passer.len() {
                        let message_passer = message_passer_passer.recv().await.unwrap();
                        // Blindly populate
                        return_lookup.insert(message_passer.socket_id, message_passer.sender);
                    }

                    // Try again
                    let sender = return_lookup.get(&socket_id).unwrap();

                    // Increment the counter
                    let size = socket.payload.len() as u32;
                    CLIENT_META_UPSTREAM.response_to_socks_bytes.fetch_add(size, Ordering::SeqCst);

                    // Send the message
                    sender.send(socket).unwrap();
                }
            }
        }

        // Print stats
        dprintln!("Meta stats: {:?}", CLIENT_META_UPSTREAM.as_base());
    }
}

pub async fn handle_transit(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut upstream_passer_rcv: Receiver<UpStreamMessage>,
    message_passer_passer_send: Arc<BroadcastSender<DownstreamBackpasser>>,
) {
    // Spawn requisite push and pull handlers
    // Create a channel for the push handler to send to so that a random not-in-use push handler
    // can grab the data to send.

    // Create the push handlers as flume message passers of MultipleMessagesUpstream
    let (push_passer_send, push_passer_receive) = flume::unbounded::<Vec<libtransit::SocksSocketUpstream>>();

    let (push_client_count, pull_client_count) = {
        let transit_socket = transit_socket.read().await;
        (
            transit_socket.push_client_count,
            transit_socket.pull_client_count,
        )
    };

    for _ in 0..push_client_count {
        task::spawn(push_handler(
            transit_socket.clone(),
            push_passer_receive.clone(),
        ));
    }

    // Create the pull handlers
    for _ in 0..pull_client_count {
        let new_receiver = message_passer_passer_send.subscribe();
        task::spawn(pull_handler(transit_socket.clone(), new_receiver));
    }

    // TODO make this variable
    let FORCE_SEND_BUFF_SIZE = 1024 * 256;
    let MODETIME = 10; // In milliseconds

    let mut last_upstream_time = Instant::now();
    let mut current_buffer_size = 0;
    let mut last_loop_time = Instant::now();

    let mut socks_sockets: Vec<libtransit::SocksSocketUpstream> = Vec::with_capacity(8);

    loop {
        let delta  = Instant::now() - last_loop_time;
        if delta > Duration::from_millis(3) {
            println!("Transit took an unusual amount of time ({}ms) to iterate", delta.as_millis());
        }
        last_loop_time = Instant::now();

        // Get data to send up
        let stream_data = upstream_passer_rcv.try_recv();
        match stream_data {
            Ok(mut upstream) => {
                // Increment buffer size
                let size = upstream.payload.len() as u32;
                current_buffer_size += size;

                // Take away from socks_to_coordinator
                CLIENT_META_UPSTREAM.socks_to_coordinator_bytes.fetch_sub(size, Ordering::Relaxed);

                // Add in timing info
                let now_ms = ms_since_epoch();

                dprintln!("Gotten data to send up: {:?}", upstream);
                
                // Find the relevant socks socket to insert into
                let mut found = false;
                // Iterate over socks_sockets without taking ownership
                for socks_socket in socks_sockets.iter_mut() {
                    if socks_socket.socket_id == upstream.socket_id {
                        // Is it a terminate message?
                        if upstream.red_terminate {
                            // Set terminate flag to true on the socks socket
                            socks_socket.red_terminate = true;
                        } else {
                            // Insert into the socks socket
                            socks_socket.payload.append(&mut upstream.payload);
                        }
                        found = true;
                        break;
                    }
                }

                if !found {
                    // Create a new socks socket
                    let socks_socket = libtransit::SocksSocketUpstream {
                        socket_id: upstream.socket_id,
                        dest_ip: upstream.dest_ip,
                        dest_port: upstream.dest_port,
                        payload: upstream.payload,
                        red_terminate: false,
                    };

                    // Insert into the socks sockets
                    socks_sockets.push(socks_socket);
                }

                // Increase the buffer size recorded
                CLIENT_META_UPSTREAM.coordinator_to_request_buffer_bytes.fetch_add(size, Ordering::Relaxed);
            }
            Err(error) => {
                match error {
                    TryRecvError::Empty => {
                        // Wait 1ms
                        let start_time = Instant::now();
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        let end_time = Instant::now();
                        let delta = end_time - start_time;
                        let delta_ms = delta.as_millis();
                        if delta_ms > 10 {
                            eprintln!("Took {}ms to sleep targeted 1ms", delta_ms);
                        }
                    }
                    TryRecvError::Disconnected => {
                        // TODO handle this
                        panic!("Upstream passer disconnected");
                    }
                }
            }
        }

        let mut do_send = false;
        if current_buffer_size > FORCE_SEND_BUFF_SIZE {
            println!("Sending on due to buffer size {} being greater than {}", current_buffer_size, FORCE_SEND_BUFF_SIZE);
            do_send = true;
        }

        if last_upstream_time.elapsed().as_millis() > MODETIME
            && current_buffer_size > 0
        {
            println!("Sending on due to modetime");
            do_send = true;
        }

        // If we're above buff size
        if do_send {
            // Send the buffer to the push handler
            push_passer_send.send_async(socks_sockets.clone()).await.unwrap();
            // Set recorded buffer size to 0
            CLIENT_META_UPSTREAM.coordinator_to_request_buffer_bytes.store(0, Ordering::Relaxed);

            // Increment egress buffer
            CLIENT_META_UPSTREAM.coordinator_to_request_channel_bytes.fetch_add(current_buffer_size, Ordering::Relaxed);

            // Eliminate socks sockets with terminate flag set
            socks_sockets.retain(|x| !x.red_terminate); // This is *elegant*

            // Reset buffer
            for socks_socket in socks_sockets.iter_mut() {
                socks_socket.payload.clear(); // Avoids reallocations this way, but causes unnecessary memory usage
            }

            // Reset buffer size
            current_buffer_size = 0;

            // Reset last upstream time
            last_upstream_time = Instant::now();
        }
    }
}
