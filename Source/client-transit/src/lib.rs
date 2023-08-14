use libsecrets::{self, EncryptionKey};
use libtransit;
use rand::Rng;
use reqwest::Client;
use libtransit::{
    ClientMessageUpstream,
    ServerMessageDownstream,
    ClientMetaUpstream,
    ServerMetaDownstream,
    UpStreamMessage,
    DownStreamMessage,
    SocketID,
    DeclarationToken
};
use tokio::sync::mpsc::{self, Sender, Receiver};
use flume::{Sender as FlumeSender, Receiver as FlumeReceiver, self};

mod builder;
pub use self::builder::TransitSocketBuilder;
use reqwest::header::{HeaderMap, HeaderValue};
use tokio::task;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::{self, Sender as BroadcastSender, Receiver as BroadcastReceiver};

use std::collections::HashMap;

pub struct TransitSocket {
    target: String, // Base URL, incl. protocol
    key: EncryptionKey, // Encryption key for libsecrets
    control_client: Client, // Does the initial encryption agreement
    server_meta: ServerMetaDownstream, // Statistics about the server to inform client congestion control
    client_identifier: DeclarationToken, // Identifier for this client, used as a cookie
    push_client_count: usize, // Number of hybrid clients (Both push and pull)
    pull_client_count: usize, // Number of pull clients
    timeout_time: usize, // Time in seconds before a request is considered timed out
    headers: HeaderMap, // Headers to send with requests. Includes client identifier
    is_initialized: bool, // Whether the socket has been initialized by greeting the server
    client_name: String, // Name of the client
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

pub struct DownstreamBackpasser {
    pub socket_id: libtransit::SocketID,
    pub sender: Sender<DownStreamMessage>,
}

impl TransitSocket {
    pub async fn connect(&mut self) -> Result<(), TransitInitError> {
        self.greet_server().await?;
        self.is_initialized = true;
        Ok(())
    }
    
    // TODO this is unusual traffic, not very believable.
    // Change this to "Get a PNG image" incl. header etc, with the
    // encrypted data being some form of additional token in the cookies
    async fn greet_server(&self) -> Result<(), TransitInitError> {
        let mut data = format!("Hello. Protocol version: {}, client-transit version: {}, client-name: {}", "0", env!("CARGO_PKG_VERSION"), self.client_name);
        
        // Find minimum padding amount such that it's at least 512 bytes
        let length = data.len();
        let min_padding = 512 - length;

        // Find maximum padding amount such that it's at most 1024 bytes
        let max_padding = 1024 - length;

        // Add random padding up to 1024 bytes
        let mut rng = rand::thread_rng();
        let amount = rng.gen_range(min_padding..max_padding);
        data = data + &" ".repeat(amount);

        let encrypted = libsecrets::encrypt(data.as_bytes(), &self.key)?;

        let response = self.control_client.post(&format!("{}/submit", self.target))
            .body(encrypted)
            .headers(self.headers.clone())
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
                        let decrypted = libsecrets::decrypt(&encrypted, &self.key)?;
                        // Convert to string
                        let decrypted = String::from_utf8(decrypted);
                        match decrypted {
                            Ok(decrypted) => {
                                // Remove spaces
                                let decrypted = decrypted.trim_end_matches(' ');
                                if decrypted != "CONNECTION ACCEPTED" {
                                    return Err(TransitInitError::LiedResponse);
                                }
                            },
                            Err(_) => {
                                return Err(TransitInitError::LiedResponse);
                            }
                        }
                    },
                    Err(error) => {
                        return Err(TransitInitError::RequestError(error));
                    }
                }

            },
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

    async fn push_handler(target: String, to_send_passer: FlumeReceiver<Vec<UpStreamMessage>>) {
        // Create client
        let client = Client::new();

        loop {
            // Get the data to send
            let to_send = to_send_passer.recv_async().await;

            let to_send = match to_send {
                Ok(to_send) => to_send,
                Err(_) => {
                    // TODO handle this
                    continue;
                }
            };

            // Send the data
            // Encode and encrypt in a thread
            

            // Send the data
        }
    }

    async fn pull_handler(tcp_return_passers: Arc<RwLock<HashMap<SocketID, Sender<DownStreamMessage>>>>,
    messagePasserPasser: BroadcastReceiver<DownstreamBackpasser>
    ) {

    }

    pub async fn handle_transit(&mut self, mut upstreamPasserRcv: Receiver<UpStreamMessage>, messagePasserPasserSend: Arc<BroadcastSender<DownstreamBackpasser>>) {
        // Spawn requisite push and pull handlers
        // Create a channel for the push handler to send to so that a random not-in-use push handler
        // can grab the data to send.

        let tcp_return_passers: Arc<RwLock<HashMap<SocketID, Sender<DownStreamMessage>>>> = Arc::new(RwLock::new(HashMap::new()));

        // Create the push handlers as flume message passers of a vec of UpStreamMessages
        let (push_passer_send, push_passer_receive) = flume::unbounded::<Vec<UpStreamMessage>>();

        for _ in 0..self.push_client_count {
            task::spawn(Self::push_handler(push_passer_receive.clone()));
        }

        // Create the pull handlers
        for _ in 0..self.pull_client_count {
            let new_receiver = messagePasserPasserSend.subscribe();
            task::spawn(Self::pull_handler(tcp_return_passers.clone(), new_receiver));
        }

        // TODO make this variable
        let BUFF_SIZE = 1024 * 256;
        let MODETIME = 40; // In milliseconds

        let mut buffer: Vec<UpStreamMessage> = Vec::new();

        let mut last_upstream_time = Instant::now();
        let mut current_buffer_size = 0;

        loop {
            // Get data from downstream
            let data = upstreamPasserRcv.recv().await;
            let data = match data {
                Some(data) => data,
                None => {
                    // TODO handle this
                    continue;
                }
            };
            // Increment buffer size
            current_buffer_size += data.payload.len();
            // Add it to the buffer
            buffer.push(data);

            // If we're above buff size
            if current_buffer_size > BUFF_SIZE {
                // Send the buffer to the push handler
                push_passer_send.send_async(buffer).await;
                // Reset buffer
                buffer = Vec::new();
                // Reset buffer size
                current_buffer_size = 0;
            }

            // If we're above mode time and there's more than one piece of data in the buffer
            if last_upstream_time.elapsed().as_millis() > MODETIME && buffer.len() > 1 {
                // Send the buffer to the push handler
                push_passer_send.send_async(buffer).await;
                // Reset buffer
                buffer = Vec::new();
                // Reset buffer size
                current_buffer_size = 0;
                // Reset last upstream time
                last_upstream_time = Instant::now();
            }

            // If we're above mode time and this is the first piece of data, reset the last upstream time
            if last_upstream_time.elapsed().as_millis() > MODETIME && buffer.len() == 1 {
                last_upstream_time = Instant::now();
            }
        }
    }
}