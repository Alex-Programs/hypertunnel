use libtransit::{self, ClientMetaUpstreamTrafficStats, SocketID};
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use once_cell::sync::Lazy;

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

pub fn ms_since_epoch() -> u64 {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_the_epoch.as_secs() * 1000 + since_the_epoch.subsec_nanos() as u64 / 1_000_000
}

// Simple format: List of socket IDs to terminate reading from ASAP. Order doesn't really matter,
// the info should be sent as soon as possible. See diagram.
pub type YellowDataUpstreamQueue = Lazy<RwLock<Vec<SocketID>>>;

pub static YELLOW_DATA_UPSTREAM_QUEUE: YellowDataUpstreamQueue = Lazy::new(|| {
    RwLock::new(Vec::new())
});