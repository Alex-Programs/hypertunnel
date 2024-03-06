use rand::Rng; // Not unused - ignore vscode
use borsh::{BorshSerialize, BorshDeserialize, from_slice, to_vec};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub type Port = u16;
pub type IPV4 = u32;
pub type SocketID = u32;
pub type DeclarationToken = [u8; 16];

static LAST_SOCKET_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub trait SerialMessage: BorshSerialize + BorshDeserialize {
    fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = to_vec(self)?;
        Ok(data)
    }

    fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        Self::try_from_slice(data)
    }
}

pub fn generate_socket_id() -> SocketID {
    LAST_SOCKET_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as SocketID
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct UpStreamMessage {
    pub socket_id: SocketID,
    pub dest_ip: IPV4,
    pub dest_port: Port,
    pub payload: Vec<u8>,
    pub red_terminate: bool,
}

impl SerialMessage for UpStreamMessage {}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct DownStreamMessage {
    pub socket_id: SocketID,
    pub payload: Vec<u8>,
    pub do_green_terminate: bool,
}

impl SerialMessage for DownStreamMessage {}

// ================================================= //

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
pub struct ClientMessageUpstream {
    pub metadata: ClientMetaUpstream,
    pub socks_sockets: Vec<SocksSocketUpstream>,
    pub payload_size: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ClientMetaUpstream {
    pub packet_info: UnifiedPacketInfo,
    pub traffic_stats: ClientMetaUpstreamTrafficStats,
    pub set: Option<ClientMetaUpstreamSet>,
    pub yellow_to_stop_reading_from: Vec<SocketID>,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct UnifiedPacketInfo {
    pub unix_ms: u64, // As u32 it would only last 49 days
    pub seq_num: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ClientMetaUpstreamTrafficStats {
    pub socks_to_coordinator_bytes: u32,
    pub coordinator_to_request_buffer_bytes: u32,
    pub coordinator_to_request_channel_bytes: u32,
    pub up_request_in_progress_bytes: u32,
    pub response_to_socks_bytes: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ClientMetaUpstreamSet {
    pub buffer_size_to_return: u32,
    pub modetime_to_return: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct SocksSocketUpstream {
    pub socket_id: SocketID,
    pub dest_ip: IPV4,
    pub dest_port: Port,
    pub payload: Vec<u8>,
    pub red_terminate: bool,
}

impl SerialMessage for ClientMessageUpstream {}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
pub struct ServerMessageDownstream {
    pub metadata: ServerMetaDownstream,
    pub socks_sockets: Vec<SocksSocketDownstream>,
    pub payload_size: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstream {
    pub packet_info: UnifiedPacketInfo,
}


#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct SocksSocketDownstream {
    pub socket_id: SocketID,
    pub dest_ip: IPV4,
    pub dest_port: Port,
    pub payload: Vec<u8>,
    pub do_green_terminate: bool,
    pub do_blue_terminate: bool,
}

#[repr(u8)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub enum ServerMetaDownstreamLogSeverity {
    Info,
    Warning,
    Error,
}

impl SerialMessage for ServerMessageDownstream {}