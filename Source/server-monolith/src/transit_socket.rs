use libsecrets::EncryptionKey;
use libtransit;
use std::sync::RwLock;

pub struct ClientStatistics {
    pub to_send: usize,
    pub to_reply: usize,
}

pub struct TransitSocket {
    pub key: EncryptionKey,
    pub client_statistics: ClientStatistics,
    // TODO information for communicating with SOCKS threads, returning
    // requests, etc. Need to figure out how to do this.
}

impl TransitSocket {
    pub fn new(key: EncryptionKey) -> Self {
        Self {
            key,
            client_statistics: ClientStatistics {
                to_send: 0,
                to_reply: 0,
            },
        }
    }
}