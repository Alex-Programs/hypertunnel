use libsecrets::{self, EncryptionKey};
use libtransit;
use rand::Rng;
use reqwest::blocking::Client;
use std::sync::{RwLock, Condvar};


mod builder;
pub use self::builder::TransitSocketBuilder;
use reqwest::header::{HeaderMap, HeaderValue};

pub struct ServerStatistics {
    pub to_reply: usize, // Bytes
    pub to_send: usize, // Bytes
}

pub struct TransitSocket {
    target: String, // Base URL, incl. protocol
    key: EncryptionKey, // Encryption key for libsecrets
    control_client: Client, // Does the initial encryption agreement
    send_buffer: RwLock<Vec<libtransit::Message>>, // Messages to send
    recv_buffer: RwLock<Vec<libtransit::Message>>, // Messages received, but not yet sent to user of this socket
    server_statistics: ServerStatistics, // Statistics about the server to inform client congestion control
    client_identifier: String, // Identifier for this client, used as a cookie
    hybrid_client_count: usize, // Number of hybrid clients (Both push and pull)
    pull_client_count: usize, // Number of pull clients
    timeout_time: usize, // Time in seconds before a request is considered timed out
    headers: HeaderMap, // Headers to send with requests. Includes client identifier
}

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
    // TODO this is unusual traffic, not very believable.
    // Change this to "Get a PNG image" incl. header etc, with the
    // encrypted data being some form of additional token in the cookies
    
    fn greet_server(&self) -> Result<(), TransitInitError> {
        let mut data = "Hello!".to_string();
        // Pad with random amount of spaces
        let mut rng = rand::thread_rng();
        let amount = rng.gen_range(0..100);
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