use std::sync::atomic::{AtomicU32, Ordering};
use libtransit::ServerMetaDownstreamTrafficStats;
use std::time::{UNIX_EPOCH, SystemTime};

#[derive(Debug)]
pub struct ServerMetaDownstreamTrafficStatsSynced {
    pub http_up_to_coordinator_bytes: AtomicU32,
    pub coordinator_up_to_socket_bytes: AtomicU32,
    pub socket_down_to_coordinator_bytes: AtomicU32,
    pub coordinator_down_to_http_message_passer_bytes: AtomicU32,
    pub coordinator_down_to_http_buffer_bytes: AtomicU32,
    pub congestion_ctrl_intake_throttle: AtomicU32,
}

impl ServerMetaDownstreamTrafficStatsSynced {
    // Convert into a ServerMetaDownstreamTrafficStats by turning AtomicU32 into u32
    pub fn into_server_meta_downstream_traffic_stats(&self) -> ServerMetaDownstreamTrafficStats {
        ServerMetaDownstreamTrafficStats {
            http_up_to_coordinator_bytes: self.http_up_to_coordinator_bytes.load(Ordering::Relaxed),
            coordinator_up_to_socket_bytes: self.coordinator_up_to_socket_bytes.load(Ordering::Relaxed),
            socket_down_to_coordinator_bytes: self.socket_down_to_coordinator_bytes.load(Ordering::Relaxed),
            coordinator_down_to_http_message_passer_bytes: self.coordinator_down_to_http_message_passer_bytes.load(Ordering::Relaxed),
            coordinator_down_to_http_buffer_bytes: self.coordinator_down_to_http_buffer_bytes.load(Ordering::Relaxed),
            congestion_ctrl_intake_throttle: self.congestion_ctrl_intake_throttle.load(Ordering::Relaxed),
        }
    }
}

pub fn ms_since_epoch() -> u64 {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_the_epoch.as_secs() * 1000 + since_the_epoch.subsec_nanos() as u64 / 1_000_000
}