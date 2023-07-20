use libsecrets::EncryptionKey;

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