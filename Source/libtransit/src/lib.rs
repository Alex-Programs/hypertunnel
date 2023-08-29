use rand::Rng; // Not unused - ignore vscode
use borsh::{BorshSerialize, BorshDeserialize};
use std::collections::HashMap;

pub type Port = u16;
pub type IPV4 = u32;
pub type SocketID = u32;
pub type DeclarationToken = [u8; 16];

static LAST_SOCKET_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn generate_socket_id() -> SocketID {
    LAST_SOCKET_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as SocketID
}

#[test]
fn test_generate_socket_id() {
    let socket_id = generate_socket_id();
    assert_eq!(socket_id, 0);
    let socket_id = generate_socket_id();
    assert_eq!(socket_id, 1);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct UpStreamMessage {
    pub socket_id: SocketID,
    pub dest_ip: IPV4,
    pub dest_port: Port,
    pub payload: Vec<u8>,
}

impl UpStreamMessage {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_upstream_msg() {
    let msg = UpStreamMessage {
        socket_id: 0,
        dest_ip: 0,
        dest_port: 0,
        payload: vec![0, 1, 2, 3],
    };
    let encoded = msg.encoded().unwrap();
    let decoded = UpStreamMessage::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct DownStreamMessage {
    pub socket_id: SocketID,
    pub payload: Vec<u8>,
}

impl DownStreamMessage {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_downstream_msg() {
    let msg = DownStreamMessage {
        socket_id: 0,
        payload: vec![0, 1, 2, 3],
    };
    let encoded = msg.encoded().unwrap();
    let decoded = DownStreamMessage::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

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
    pub termination_reason: Vec<UpStreamTerminationReason>,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub enum UpStreamTerminationReason {
    SocketClosed,
    // Todo add more as I implement them
}

impl ClientMessageUpstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_client_upstream() {
    let msg = ClientMessageUpstream {
        metadata: ClientMetaUpstream {
            packet_info: UnifiedPacketInfo {
                unix_ms: 0,
                seq_num: 0,
            },
            traffic_stats: ClientMetaUpstreamTrafficStats {
                socks_to_coordinator_bytes: 0,
                coordinator_to_request_buffer_bytes: 0,
                coordinator_to_request_channel_bytes: 0,
                up_request_in_progress_bytes: 0,
                response_to_socks_bytes: 0,
            },
            set: None,
        },
        socks_sockets: vec![SocksSocketUpstream {
            socket_id: 0,
            dest_ip: 0,
            dest_port: 0,
            payload: vec![0, 1, 2, 3],
            termination_reason: vec![UpStreamTerminationReason::SocketClosed],
        }],
        payload_size: 4,
    };
    let encoded = msg.encoded().unwrap();
    let decoded = ClientMessageUpstream::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
pub struct ServerMessageDownstream {
    pub metadata: ServerMetaDownstream,
    pub socks_sockets: Vec<SocksSocketDownstream>,
    pub payload_size: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstream {
    pub packet_info: UnifiedPacketInfo,
    pub traffic_stats: ServerMetaDownstreamTrafficStats,
    pub server_stats: ServerMetaDownstreamServerStats,
    pub logs: Vec<ServerMetaDownstreamLog>,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstreamTrafficStats {
    pub http_up_to_coordinator_bytes: u32,
    pub coordinator_up_to_socket_bytes: u32,
    pub socket_down_to_coordinator_bytes: u32,
    pub coordinator_down_to_http_message_passer_bytes: u32,
    pub coordinator_down_to_http_buffer_bytes: u32,
    pub congestion_ctrl_intake_throttle: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstreamServerStats {
    pub cpu_usage: f32,
    pub memory_usage_kb: u32,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstreamLog {
    timestamp: u64,
    severity: ServerMetaDownstreamLogSeverity,
    message: String,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct SocksSocketDownstream {
    pub socket_id: SocketID,
    pub dest_ip: IPV4,
    pub dest_port: Port,
    pub payload: Vec<u8>,
    pub termination_reasons: Vec<DownStreamTerminationReason>,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub enum DownStreamTerminationReason {
    SocketClosed,
    // Todo add more as I implement them
}

#[repr(u8)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub enum ServerMetaDownstreamLogSeverity {
    Info,
    Warning,
    Error,
}

impl ServerMessageDownstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_server_message_downstream() {
    let msg = ServerMessageDownstream {
        metadata: ServerMetaDownstream {
            packet_info: UnifiedPacketInfo {
                unix_ms: 0,
                seq_num: 0,
            },
            traffic_stats: ServerMetaDownstreamTrafficStats {
                http_up_to_coordinator_bytes: 0,
                coordinator_up_to_socket_bytes: 0,
                socket_down_to_coordinator_bytes: 0,
                coordinator_down_to_http_message_passer_bytes: 0,
                coordinator_down_to_http_buffer_bytes: 0,
                congestion_ctrl_intake_throttle: 0,
            },
            server_stats: ServerMetaDownstreamServerStats {
                cpu_usage: 0.0,
                memory_usage_kb: 0,
            },
            logs: vec![ServerMetaDownstreamLog {
                timestamp: 0,
                severity: ServerMetaDownstreamLogSeverity::Info,
                message: "test".to_string(),
            }],
        },
        socks_sockets: vec![SocksSocketDownstream {
            socket_id: 0,
            dest_ip: 0,
            dest_port: 0,
            payload: vec![0, 1, 2, 3],
            termination_reasons: vec![DownStreamTerminationReason::SocketClosed],
        }],
    };
    let encoded = msg.encoded().unwrap();
    let decoded = ServerMessageDownstream::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}