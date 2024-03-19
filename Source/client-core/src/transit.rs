use libtransit::SerialMessage;

use flume::{self, Receiver as FlumeReceiver, Sender as FlumeSender};
use libsecrets::{self, EncryptionKey};
use libtransit::{self, UnifiedPacketInfo, SocksSocketUpstream, SocksSocketDownstream};
use libtransit::{
    ClientMessageUpstream, ClientMetaUpstream, DeclarationToken,
    ServerMessageDownstream, SocketID, UpStreamMessage,
};
use rand::Rng;
use reqwest::Client;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::mpsc::error::TryRecvError;
use std::collections::HashMap;

use reqwest::header::HeaderMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast::{Receiver as BroadcastReceiver, Sender as BroadcastSender};
use tokio::sync::RwLock;
use tokio::task;

static RECEIVED_SEQ_NUM: AtomicU32 = AtomicU32::new(0);
static SENT_SEQ_NUM: AtomicU32 = AtomicU32::new(0);

use crate::meta::YELLOW_DATA_UPSTREAM_QUEUE;

use log::{debug, error, info, warn};

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

fn generate_nonsense_data() -> String {
    let dataset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let count = 64;

    let mut rng = rand::thread_rng();
    let mut data = String::with_capacity(count);
    for _ in 0..count {
        let index = rng.gen_range(0..dataset.len());
        data.push(dataset.chars().nth(index).unwrap());
    }

    data
}

// TODO this is unusual traffic, not very believable.
// Change this to "Get a PNG image" incl. header etc, with the
// encrypted data being some form of additional token in the cookies
async fn greet_server(transit_socket: Arc<RwLock<TransitSocket>>) -> Result<(), TransitInitError> {
    info!("Greeting server");

    let mut data = format!(
        "Hello. Protocol version: {}, client-transit version: {}, client-name: {}",
        "2",
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

    let (target, headers, key, _) = {
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
        .timeout(Duration::from_secs(2 as u64))
        .send()
        .await;

    info!("Received response from server");

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

    info!("Processed response from server");

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

    debug!("Push handler started");

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

        let payload_size = to_send.iter().fold(0, |acc, x| acc + x.payload.len() as u32);

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
        }).await.unwrap();

        let url = format!("{}/upload/segment/{}", target, generate_nonsense_data());

        // Send the data
        let response = client
            .post(url)
            .body(to_send)
            .headers(headers.clone())
            .timeout(timeout_duration) // TODO RM
            .send()
            .await;

        let response = response.unwrap();

        debug!("Sent! Response code {:?}, data: {:?}", response.status(), response.bytes().await);

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
        yellow_to_stop_reading_from,
    };

    debug!("Sending metadata: {:?}", data);

    data
}

async fn pull_handler(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut message_passer_passer: BroadcastReceiver<DownstreamBackpasser>,
    blue_termination_send: FlumeSender<SocketID>,
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

    let default_timeout = 300;
    let mut cumulative_timeout = default_timeout;

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

        let url = format!("{}/video/segment", target);

        // Send the data
        let response = client
            .get(url)
            .body(to_send)
            .headers(headers.clone())
            .timeout(timeout_duration)
            .send()
            .await;

        let response = match response {
            Ok(response) => {
                if response.status() != 200 {
                    warn!("Request completed but with status {}", response.status());

                    cumulative_timeout = default_timeout;

                    continue
                }
                cumulative_timeout = default_timeout;

                response
            }
            Err(error) => {
                if error.is_timeout() {
                    // All if fine
                    info!("Request timeout; continue");
                    continue
                }
                warn!("Error on request. Error: {:?}", error);

                tokio::time::sleep(Duration::from_millis(cumulative_timeout)).await;

                cumulative_timeout *= 2;

                continue
            }
        };

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

        for socket in decoded.socks_sockets {
            // Do we need to send on a blue termination signal?
            if socket.do_blue_terminate {
                // Inform both sockets and coordinator to either terminate or remove data
                blue_termination_send.send(socket.socket_id).unwrap();

                // Do not remove from lookup etc: There may still be data to send back, this is only one side of the connection
            }
            let socket_id = socket.socket_id;
            let do_term = socket.do_green_terminate;

            // Get the sender
            let sender = return_lookup.get(&socket_id);

            match sender {
                Some(sender) => {
                    // Send the message
                    match sender.send(socket) {
                        Ok(_) => {}
                        Err(_) => {
                            // Remove the sender from the lookup
                            return_lookup.remove(&socket_id);
                        }
                    };
                }
                None => {
                    // Populate the sender via iteration through the messagePasserPasser
                    for _ in 0..message_passer_passer.len() {
                        let message_passer = message_passer_passer.recv().await.unwrap();
                        // Blindly populate
                        return_lookup.insert(message_passer.socket_id, message_passer.sender);
                    }

                    // Try again
                    let sender = match return_lookup.get(&socket_id) {
                        Some(sender) => sender,
                        None => {
                            // If we still don't have a sender, give up
                            continue;
                        }
                    };

                    // Send the message
                    match sender.send(socket) {
                        Ok(_) => {}
                        Err(_) => {
                            // Remove the sender from the lookup
                            return_lookup.remove(&socket_id);
                        }
                    };
                }
            }

            // Is it set to terminate?
            if do_term {
                // Remove the sender from the lookup. Yes this can sometimes add then immediately remove from the hashmap if they terminate on their first transit window; I don't care, we have bigger
                // perf issues.
                return_lookup.remove(&socket_id);

                debug!("Terminated socket from return lookup: {}", socket_id);
            }
        }
    }
}

pub async fn handle_transit(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut upstream_passer_rcv: Receiver<UpStreamMessage>,
    message_passer_passer_send: Arc<BroadcastSender<DownstreamBackpasser>>,
    blue_termination_send: FlumeSender<SocketID>,
    blue_termination_receive: FlumeReceiver<SocketID>
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
        task::spawn(pull_handler(transit_socket.clone(), new_receiver, blue_termination_send.clone()));
    }

    // TODO make this variable
    let force_send_buff_size = 1024 * 32768;
    let mode_time = 10; // In milliseconds

    let mut last_upstream_time = Instant::now();
    let mut current_buffer_size = 0;
    let mut last_loop_time = Instant::now();

    let mut socks_sockets: Vec<libtransit::SocksSocketUpstream> = Vec::with_capacity(8);

    loop {
        let delta  = Instant::now() - last_loop_time;
        if delta > Duration::from_millis(12) {
            warn!("Transit took an unusual amount of time ({}ms) to iterate", delta.as_millis());
        }
        last_loop_time = Instant::now();

        // Get data to send up
        let stream_data = upstream_passer_rcv.try_recv();
        match stream_data {
            Ok(mut upstream) => {
                // Increment buffer size
                let size = upstream.payload.len() as u32;
                current_buffer_size += size;

                debug!("Gotten data to send up: {:?}", upstream);
                
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
                            warn!("Took {}ms to sleep targeted 1ms", delta_ms);
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
        if current_buffer_size > force_send_buff_size {
            debug!("Sending on due to buffer size {} being greater than {}", current_buffer_size, force_send_buff_size);
            do_send = true;
        }

        if last_upstream_time.elapsed().as_millis() > mode_time
            && current_buffer_size > 0
        {
            debug!("Sending on due to modetime");
            do_send = true;
        }

        // If we're above buff size
        if do_send {
            // Wipe everything in the blue channel from the data - it can't be written so no point sending
            loop {
                let socket_id = match blue_termination_receive.try_recv() {
                    Ok(socket_id) => socket_id,
                    Err(_) => break,
                };

                // Do not remove from socks sockets: We still might want to receive from it

                // Instead, just remove it from the send buffer. It's ok if we remove a red terminate - the writer's already dead
                socks_sockets.retain(|x| x.socket_id != socket_id);
            }

            // Send the buffer to the push handler
            push_passer_send.send_async(socks_sockets.clone()).await.unwrap();

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
