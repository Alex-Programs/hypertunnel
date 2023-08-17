use libtransit::{self, ClientMetaUpstreamTrafficStats};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct ClientMetaUpstreamTrafficStatsAtomic {
    pub socks_to_coordinator_bytes: AtomicU32,
    pub coordinator_to_request_buffer_bytes: AtomicU32,
    pub coordinator_to_request_channel_bytes: AtomicU32,
    pub up_request_in_progress_bytes: AtomicU32,
    pub response_to_socks_bytes: AtomicU32,
}

pub static CLIENT_META_UPSTREAM: ClientMetaUpstreamTrafficStatsAtomic =
    ClientMetaUpstreamTrafficStatsAtomic {
        socks_to_coordinator_bytes: AtomicU32::new(0), // IO
        coordinator_to_request_buffer_bytes: AtomicU32::new(0), // IO
        coordinator_to_request_channel_bytes: AtomicU32::new(0), // IO
        up_request_in_progress_bytes: AtomicU32::new(0), // IO
        response_to_socks_bytes: AtomicU32::new(0), // IO
    };

impl ClientMetaUpstreamTrafficStatsAtomic {
    pub fn as_base(&self) -> ClientMetaUpstreamTrafficStats {
        ClientMetaUpstreamTrafficStats {
            socks_to_coordinator_bytes: self.socks_to_coordinator_bytes.load(Ordering::Relaxed),
            coordinator_to_request_buffer_bytes: self
                .coordinator_to_request_buffer_bytes
                .load(Ordering::Relaxed),
            coordinator_to_request_channel_bytes: self
                .coordinator_to_request_channel_bytes
                .load(Ordering::Relaxed),
            up_request_in_progress_bytes: self.up_request_in_progress_bytes.load(Ordering::Relaxed),
            response_to_socks_bytes: self.response_to_socks_bytes.load(Ordering::Relaxed),
        }
    }
}
