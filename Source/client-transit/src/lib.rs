use flume::{self, Receiver as FlumeReceiver, Sender as FlumeSender};
use libsecrets::{self, EncryptionKey};
use libtransit::{self, CloseSocketMessage, MultipleMessagesUpstream};
use libtransit::{
    ClientMessageUpstream, ClientMetaUpstream, DeclarationToken, DownStreamMessage,
    ServerMessageDownstream, ServerMetaDownstream, SocketID, UpStreamMessage,
};
use rand::Rng;
use reqwest::Client;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::mpsc::error::TryRecvError;

mod builder;
pub use self::builder::TransitSocketBuilder;
use reqwest::header::{HeaderMap, HeaderValue};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::{self, Receiver as BroadcastReceiver, Sender as BroadcastSender};
use tokio::sync::RwLock;
use tokio::task;

use std::collections::HashMap;

pub struct TransitSocket {
    target: String,                      // Base URL, incl. protocol
    key: EncryptionKey,                  // Encryption key for libsecrets
    server_meta: ServerMetaDownstream, // Statistics about the server to inform client congestion control
    client_identifier: DeclarationToken, // Identifier for this client, used as a cookie
    push_client_count: usize,          // Number of hybrid clients (Both push and pull)
    pull_client_count: usize,          // Number of pull clients
    timeout_time: usize,               // Time in seconds before a request is considered timed out
    headers: HeaderMap,                // Headers to send with requests. Includes client identifier
    is_initialized: bool, // Whether the socket has been initialized by greeting the server
    client_name: String,  // Name of the client
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
    pub sender: Sender<DownStreamMessage>,
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
    to_send_passer: FlumeReceiver<MultipleMessagesUpstream>,
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

    println!("Push handler started");

    loop {
        // Get the data to send
        println!("Waiting for data");
        let to_send = to_send_passer.recv_async().await;
        println!("Got data into upstream handler: {:?}", to_send);

        let to_send = match to_send {
            Ok(to_send) => to_send,
            Err(_) => {
                // TODO handle this
                to_send.unwrap();
                continue;
            }
        };

        println!("Sending data: {:?}", to_send);

        // Send the data
        // Encode and encrypt in a thread
        let to_send = tokio::task::spawn_blocking(move || {
            let upstream_message = ClientMessageUpstream {
                messages: to_send,
                metadata: get_metadata(),
            };

            let data_bin = upstream_message.encoded().unwrap();

            let encrypted = libsecrets::encrypt(&data_bin, &key).unwrap();

            encrypted
        });

        println!("About to send to server");

        // Send the data
        let response = client
            .post(&format!("{}/upload", target))
            .body(to_send.await.unwrap())
            .headers(headers.clone())
            .timeout(std::time::Duration::from_secs(1)) // TODO RM
            .send()
            .await;

        let response = response.unwrap();

        println!("Sent! Response code {:?}, data: {:?}", response.status(), response.bytes().await);

        // TODO if things go wrong, prevent the arrival of new data until the send works. See working nicely section of todo about reconstruction and error handling.
    }
}

fn get_metadata() -> ClientMetaUpstream {
    ClientMetaUpstream {
        bytes_to_send_to_remote: 0,
        bytes_to_reply_to_client: 0,
        messages_to_send_to_remote: 0,
        messages_to_reply_to_client: 0,
    }
}

async fn pull_handler(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut messagePasserPasser: BroadcastReceiver<DownstreamBackpasser>,
) {
    // This will hold the return lookup and be populated by the messagePasserPassers
    let mut return_lookup: HashMap<SocketID, Sender<DownStreamMessage>> = HashMap::new();

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
        let to_send = {
            let upstream_message = ClientMessageUpstream {
                messages: MultipleMessagesUpstream {
                    stream_messages: Vec::with_capacity(0),
                    close_socket_messages: Vec::with_capacity(0),
                },
                metadata: get_metadata(),
            };

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

        println!("Response status: {}", response.status());

        // Get the data
        let encrypted = response.bytes().await.unwrap();

        // Decrypt the data
        let mut decrypted = libsecrets::decrypt(&encrypted, &key).unwrap();

        // Decode the data
        let decoded = ServerMessageDownstream::decode_from_bytes(&mut decrypted).unwrap();

        // TODO use the metadata to determine whether to kill the connection, as well as congestion control et cetera
        // for now just send the data to the appropriate socket

        for downstream_message in decoded.messages {
            if downstream_message.has_remote_closed {
                // TODO handle this by removing from the lookup
            }

            let socket_id = downstream_message.socket_id;

            // Get the sender
            let sender = return_lookup.get(&socket_id);

            match sender {
                Some(sender) => {
                    // Send the message
                    sender.send(downstream_message).await.unwrap();
                }
                None => {
                    // Populate the sender via iteration through the messagePasserPasser
                    for _ in 0..messagePasserPasser.len() {
                        let messagePasser = messagePasserPasser.recv().await.unwrap();
                        // Blindly populate
                        return_lookup.insert(messagePasser.socket_id, messagePasser.sender);
                    }

                    // Try again
                    let sender = return_lookup.get(&socket_id).unwrap();

                    // Send the message
                    sender.send(downstream_message).await.unwrap();
                }
            }
        }
    }
}

pub async fn handle_transit(
    transit_socket: Arc<RwLock<TransitSocket>>,
    mut upstreamPasserRcv: Receiver<UpStreamMessage>,
    mut closePasserRcv: Receiver<CloseSocketMessage>,
    messagePasserPasserSend: Arc<BroadcastSender<DownstreamBackpasser>>,
) {
    // Spawn requisite push and pull handlers
    // Create a channel for the push handler to send to so that a random not-in-use push handler
    // can grab the data to send.

    // Create the push handlers as flume message passers of MultipleMessagesUpstream
    let (push_passer_send, push_passer_receive) = flume::unbounded::<MultipleMessagesUpstream>();

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
        let new_receiver = messagePasserPasserSend.subscribe();
        task::spawn(pull_handler(transit_socket.clone(), new_receiver));
    }

    // TODO make this variable
    let BUFF_SIZE = 1024 * 256;
    let MODETIME = 40; // In milliseconds

    let mut buffer_stream: Vec<UpStreamMessage> = Vec::new(); // TODO make these (and ones in the loop) appropriately presized
    let mut buffer_close: Vec<CloseSocketMessage> = Vec::new();

    let mut last_upstream_time = Instant::now();
    let mut current_buffer_size = 0;

    loop {
        // Get data to send up
        let stream_data = upstreamPasserRcv.try_recv();
        match stream_data {
            Ok(upstream) => {
                // Increment buffer size
                current_buffer_size += upstream.payload.len();

                println!("Gotten data to send up: {:?}", upstream);
                
                // Add it to the buffer
                buffer_stream.push(upstream);
            }
            Err(error) => {
                match error {
                    TryRecvError::Empty => {}
                    TryRecvError::Disconnected => {
                        // TODO handle this
                        panic!("Upstream passer disconnected");
                    }
                }
            }
        }

        // Now do it to the close socket messages
        let close_data = closePasserRcv.try_recv();
        match close_data {
            Ok(close) => {
                // Increment buffer size
                current_buffer_size += 2048; // Approxish I guess
                                             // Add it to the buffer
                buffer_close.push(close);
            }
            Err(error) => {
                match error {
                    TryRecvError::Empty => {}
                    TryRecvError::Disconnected => {
                        // TODO handle this
                        panic!("Close socket passer disconnected");
                    }
                }
            }
        }

        // If we're above buff size
        if current_buffer_size > BUFF_SIZE {
            println!("Sending data on due to buffer size");
            // Send the buffer to the push handler
            let multiple_messages = MultipleMessagesUpstream {
                stream_messages: buffer_stream,
                close_socket_messages: buffer_close,
            };
            push_passer_send.send_async(multiple_messages).await.unwrap();
            // Reset buffer
            buffer_stream = Vec::new();
            buffer_close = Vec::new();
            // Reset buffer size
            current_buffer_size = 0;
        }

        // If we're above mode time and there's more than one piece of data in the buffer
        if last_upstream_time.elapsed().as_millis() > MODETIME
            && (buffer_close.len() > 1
            || buffer_stream.len() > 1)
        {
            // Send the buffer to the push handler
            let multiple_messages = MultipleMessagesUpstream {
                stream_messages: buffer_stream,
                close_socket_messages: buffer_close,
            };
            println!("Sending data on due to modetime");
            push_passer_send.send_async(multiple_messages).await.unwrap();
            // Reset buffer
            buffer_close = Vec::new();
            buffer_stream = Vec::new();
            // Reset buffer size
            current_buffer_size = 0;
            // Reset last upstream time
            last_upstream_time = Instant::now();
        }

        // If we're above mode time and this is the first piece of data, reset the last upstream time
        if last_upstream_time.elapsed().as_millis() > MODETIME
            && (buffer_close.len() > 1
            || buffer_stream.len() > 1)
        {
            last_upstream_time = Instant::now();
        }
    }
}
