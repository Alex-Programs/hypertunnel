use libsecrets::{self, EncryptionKey};
use reqwest::Client;
use crate::TransitSocket;
use crate::ServerMetaDownstream;
use rand;
use reqwest::header::{HeaderMap, HeaderValue};
use hex;
use std::collections::HashMap;

pub struct TransitSocketBuilder {
    target: Option<String>,
    key: Option<EncryptionKey>,
    password: Option<String>,
    client_identifier: Option<[u8; 16]>,
    push_client_count: Option<usize>,
    pull_client_count: Option<usize>,
    timeout_time: Option<usize>,
    client_name: Option<String>,
}

impl TransitSocketBuilder {
    pub fn new() -> Self {
        Self {
            target: None,
            key: None,
            password: None,
            client_identifier: None,
            push_client_count: None,
            pull_client_count: None,
            timeout_time: None,
            client_name: None,
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

    pub fn with_push_client_count(mut self, push_client_count: usize) -> Self {
        self.push_client_count = Some(push_client_count);
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

    pub fn with_client_name(mut self, client_name: String) -> Self {
        self.client_name = Some(client_name);
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

        let push_client_count = match self.push_client_count {
            Some(push_client_count) => push_client_count,
            None => 6,
        };

        let pull_client_count = match self.pull_client_count {
            Some(pull_client_count) => pull_client_count,
            None => 6,
        };

        let timeout_time = match self.timeout_time {
            Some(timeout_time) => timeout_time,
            None => 10,
        };

        let control_client = Client::new();

        let server_meta = ServerMetaDownstream {
            bytes_to_reply_to_client: 0,
            bytes_to_send_to_remote: 0,
            messages_to_reply_to_client: 0,
            messages_to_send_to_remote: 0,
            cpu_usage: 0.0,
            memory_usage_kb: 0,
            num_open_sockets: 0,
            streams: Vec::new(),
        };

        let client_id_string = hex::encode(client_identifier);
        let headers = generate_headers(client_id_string, target.clone());

        let client_name = match self.client_name {
            Some(client_name) => client_name,
            None => panic!("Client name not set"),
        };

        TransitSocket {
            target,
            key,
            control_client,
            server_meta,
            client_identifier,
            push_client_count,
            pull_client_count,
            timeout_time,
            headers,
            is_initialized: false,
            client_name,
            tcp_return_passers: HashMap::new(),
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