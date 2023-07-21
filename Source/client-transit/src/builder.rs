use libsecrets::{self, EncryptionKey};
use reqwest::blocking::Client;
use crate::TransitSocket;
use crate::ServerStatistics;
use std::sync::{RwLock, Condvar};
use rand;
use reqwest::header::{HeaderMap, HeaderValue};
use hex;

pub struct TransitSocketBuilder {
    target: Option<String>,
    key: Option<EncryptionKey>,
    password: Option<String>,
    client_identifier: Option<[u8; 16]>,
    hybrid_client_count: Option<usize>,
    pull_client_count: Option<usize>,
    timeout_time: Option<usize>,
}

impl TransitSocketBuilder {
    pub fn new() -> Self {
        Self {
            target: None,
            key: None,
            password: None,
            client_identifier: None,
            hybrid_client_count: None,
            pull_client_count: None,
            timeout_time: None,
        }
    }

    pub fn with_target(mut self, target: String) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_key(mut self, key: EncryptionKey) -> Self {
        if self.password.is_some() {
            panic!("Cannot set both key and password");
        }
        self.key = Some(key);
        self
    }

    pub fn with_password(mut self, password: String) -> Self {
        if self.key.is_some() {
            panic!("Cannot set both key and password");
        }
        self.password = Some(password);
        self
    }

    pub fn with_client_identifier(mut self, client_identifier: [u8; 16]) -> Self {
        self.client_identifier = Some(client_identifier);
        self
    }

    pub fn with_hybrid_client_count(mut self, hybrid_client_count: usize) -> Self {
        self.hybrid_client_count = Some(hybrid_client_count);
        self
    }

    pub fn with_pull_client_count(mut self, pull_client_count: usize) -> Self {
        self.pull_client_count = Some(pull_client_count);
        self
    }

    pub fn with_timeout_time(mut self, timeout_time: usize) -> Self {
        self.timeout_time = Some(timeout_time);
        self
    }

    pub fn build(self) -> TransitSocket {
        let target = self.target.expect("Target not set");

        let key = if let Some(key) = self.key {
            key
        } else if let Some(password) = self.password {
            libsecrets::form_key(password.as_bytes())
        } else {
            panic!("Neither key nor password set");
        };

        let client_identifier = match self.client_identifier {
            Some(client_identifier) => client_identifier,
            None => {
                // Create a random client identifier string
                let mut client_identifier = [0; 16];
                for i in 0..16 {
                    client_identifier[i] = rand::random::<u8>();
                }
                client_identifier
            }
        };

        let hybrid_client_count = match self.hybrid_client_count {
            Some(hybrid_client_count) => hybrid_client_count,
            None => 4,
        };

        let pull_client_count = match self.pull_client_count {
            Some(pull_client_count) => pull_client_count,
            None => 1,
        };

        let timeout_time = match self.timeout_time {
            Some(timeout_time) => timeout_time,
            None => 10,
        };

        let control_client = Client::new();
        let send_buffer = RwLock::new(Vec::new());
        let recv_buffer = RwLock::new(Vec::new());

        let server_statistics = ServerStatistics {
            to_reply: 0,
            to_send: 0,
        };

        let client_id_string = hex::encode(client_identifier);
        let headers = generate_headers(client_id_string, target.clone());

        TransitSocket {
            target,
            key,
            control_client,
            send_buffer,
            recv_buffer,
            server_statistics,
            client_identifier,
            hybrid_client_count,
            pull_client_count,
            timeout_time,
            headers,
            is_initialized: false,
        }
    }
}

fn generate_headers(client_identifier: String, referer: String) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Cookie", HeaderValue::from_str(&format!("token={}", client_identifier)).unwrap());
    headers.insert("Accept-Language", HeaderValue::from_str("en-GB,en;q=0.5").unwrap());
    headers.insert("Cache-Control", HeaderValue::from_str("no-cache").unwrap());
    headers.insert("Connection", HeaderValue::from_str("keep-alive").unwrap());
    headers.insert("Pragma", HeaderValue::from_str("no-cache").unwrap());
    headers.insert("Referer", HeaderValue::from_str(&referer).unwrap());
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_str("document").unwrap());
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_str("navigate").unwrap());
    headers.insert("Sec-Fetch-Site", HeaderValue::from_str("same-origin").unwrap());
    headers.insert("TE", HeaderValue::from_str("trailers").unwrap());
    headers.insert("User-Agent", HeaderValue::from_str("Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/115.0").unwrap());

    headers
}