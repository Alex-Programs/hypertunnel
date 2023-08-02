use libsecrets::{self, EncryptionKey};
use libtransit;
use rand::Rng;
use reqwest::blocking::Client;
use std::{sync::{RwLock, Condvar}, env};
use libtransit::ServerStatistics;


mod builder;
pub use self::builder::TransitSocketBuilder;
use reqwest::header::{HeaderMap, HeaderValue};

pub struct TransitSocket {
    target: String, // Base URL, incl. protocol
    key: EncryptionKey, // Encryption key for libsecrets
    control_client: Client, // Does the initial encryption agreement
    send_buffer: RwLock<Vec<libtransit::Message>>, // Messages to send
    recv_buffer: RwLock<Vec<libtransit::DownStreamMessage>>, // Messages received, but not yet sent to user of this socket
    server_statistics: ServerStatistics, // Statistics about the server to inform client congestion control
    client_identifier: [u8; 16], // Identifier for this client, used as a cookie
    hybrid_client_count: usize, // Number of hybrid clients (Both push and pull)
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

impl TransitSocket {
    pub fn connect(&mut self) -> Result<(), TransitInitError> {
        self.greet_server()?;
        self.is_initialized = true;
        Ok(())
    }
    
    // TODO this is unusual traffic, not very believable.
    // Change this to "Get a PNG image" incl. header etc, with the
    // encrypted data being some form of additional token in the cookies
    fn greet_server(&self) -> Result<(), TransitInitError> {
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
            .send();

        match response {
            Ok(response) => {
                if response.status() != 200 {
                    return Err(TransitInitError::ConnectionDenied);
                }
                // Try to decrypt the response
                let encrypted = response.bytes();
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
}