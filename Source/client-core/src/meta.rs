use libtransit::SocketID;
use tokio::sync::RwLock;
use once_cell::sync::Lazy;

// Simple format: List of socket IDs to terminate reading from ASAP. Order doesn't really matter,
// the info should be sent as soon as possible. See diagram.
pub type YellowDataUpstreamQueue = Lazy<RwLock<Vec<SocketID>>>;

pub static YELLOW_DATA_UPSTREAM_QUEUE: YellowDataUpstreamQueue = Lazy::new(|| {
    RwLock::new(Vec::new())
});