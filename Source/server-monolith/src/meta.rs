use std::sync::atomic::{AtomicU32, Ordering};
use libtransit::ServerMetaDownstreamTrafficStats;

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