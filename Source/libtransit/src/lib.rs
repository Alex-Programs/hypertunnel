use rand::Rng; // Not unused - ignore vscode
use borsh::{BorshSerialize, BorshDeserialize};
use std::collections::HashMap;

pub type Port = u16;
pub type IPV4 = u32;
pub type SocketID = u32;
pub type DeclarationToken = [u8; 16];

static LAST_SOCKET_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// TODO look at using slices for performance

#[derive(Debug, PartialEq, Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    UpStreamMessage(UpStreamMessage),
    DownStreamMessage(DownStreamMessage),
    CloseSocketMessage(CloseSocketMessage),
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct CloseSocketMessage {
    pub socket_id: SocketID,
    pub message_sequence_number: u32,
}

impl CloseSocketMessage {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_close_socket_message() {
    let message = CloseSocketMessage {
        socket_id: 0,
        message_sequence_number: 0,
    };

    let mut encoded = message.encoded().unwrap();

    let decoded = CloseSocketMessage::decode_from_bytes(&mut encoded).unwrap();

    assert_eq!(message, decoded);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct UpStreamMessage {
    pub socket_id: SocketID,
    pub message_sequence_number: u32,
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
fn test_upstream_message() {
    let large_payload = vec![0; 100000];

    let message = UpStreamMessage {
        socket_id: 0,
        message_sequence_number: 0,
        dest_ip: 0,
        dest_port: 0,
        payload: large_payload,
    };

    let mut encoded = message.encoded().unwrap();

    let decoded = UpStreamMessage::decode_from_bytes(&mut encoded).unwrap();

    assert_eq!(message, decoded);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct DownStreamMessage {
    pub socket_id: SocketID,
    pub message_sequence_number: u32,
    pub has_remote_closed: bool,
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
fn test_downstream_message() {
    let large_payload = vec![0; 100000];

    let message = DownStreamMessage {
        socket_id: 0,
        message_sequence_number: 0,
        has_remote_closed: false,
        payload: large_payload,
    };

    let mut encoded = message.encoded().unwrap();

    let decoded = DownStreamMessage::decode_from_bytes(&mut encoded).unwrap();

    assert_eq!(message, decoded);
}

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
pub struct MultipleMessagesUpstream {
    pub stream_messages: Vec<UpStreamMessage>,
    pub close_socket_messages: Vec<CloseSocketMessage>,
}

impl MultipleMessagesUpstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_multiple_messages_upstream() {
    let large_payload = vec![0; 100000];

    let mut stream_messages = Vec::new();
    let mut close_socket_messages = Vec::new();

    for i in 0..50 {
        let message = UpStreamMessage {
            socket_id: i,
            message_sequence_number: i,
            dest_ip: 0,
            dest_port: 0,
            payload: large_payload.clone(),
        };

        stream_messages.push(message);

        let message = CloseSocketMessage {
            socket_id: i,
            message_sequence_number: i,
        };

        close_socket_messages.push(message);
    }

    let multiple_messages = MultipleMessagesUpstream {
        stream_messages,
        close_socket_messages,
    };

    let mut buffer = multiple_messages.encoded().unwrap();

    let out = MultipleMessagesUpstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(multiple_messages, out);
}

// ================================================= //

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerMetaDownstream {
    pub bytes_to_reply_to_client: usize,
    pub bytes_to_send_to_remote: usize,
    pub messages_to_reply_to_client: usize,
    pub messages_to_send_to_remote: usize,
    pub cpu_usage: f32,
    pub memory_usage_kb: usize,
    pub num_open_sockets: usize,
    pub streams: Vec<ServerStreamInfo>,
}

impl ServerMetaDownstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ServerStreamInfo {
    pub has_terminated: bool,
    pub errors: Vec<String>,
    pub logs: Vec<String>,
    pub declaration_token: DeclarationToken,
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct ClientMetaUpstream {
    pub bytes_to_send_to_remote: usize,
    pub bytes_to_reply_to_client: usize,
    pub messages_to_send_to_remote: usize,
    pub messages_to_reply_to_client: usize,
}

impl ClientMetaUpstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_server_downstream_meta() {
    let mut streams = Vec::new();

    for i in 1..100 {
        let stream_info = ServerStreamInfo {
            has_terminated: i % 2 == 0,
            errors: vec![format!("error {}", i)],
            logs: vec![format!("log {}", i), format!("another log {}", i)],
            declaration_token: [i; 16]
        };
    
        streams.push(stream_info);
    }

    let server_meta = ServerMetaDownstream {
        bytes_to_reply_to_client: 50,
        bytes_to_send_to_remote: 32423,
        messages_to_reply_to_client: 4283,
        messages_to_send_to_remote: 237482,
        cpu_usage: 4.2,
        memory_usage_kb: 2091,
        num_open_sockets: 42,
        streams,
    };

    let mut buffer = server_meta.encoded().unwrap();

    let out = ServerMetaDownstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(server_meta, out);
}

#[test]
fn test_client_upstream_meta() {
    let client_meta = ClientMetaUpstream {
        bytes_to_send_to_remote: 32423,
        bytes_to_reply_to_client: 50,
        messages_to_send_to_remote: 237482,
        messages_to_reply_to_client: 4283,
    };

    let mut buffer = client_meta.encoded().unwrap();

    let out = ClientMetaUpstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(client_meta, out);
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
pub struct ServerMessageDownstream {
    pub metadata: ServerMetaDownstream,
    pub messages: Vec<DownStreamMessage>,
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

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
pub struct ClientMessageUpstream {
    metadata: ClientMetaUpstream,
    messages: MultipleMessagesUpstream,
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
fn test_server_downstream_message() {
    let mut streams = Vec::new();

    for i in 1..100 {
        let stream_info = ServerStreamInfo {
            has_terminated: i % 2 == 0,
            errors: vec![format!("error {}", i)],
            logs: vec![format!("log {}", i), format!("another log {}", i)],
            declaration_token: [i; 16],
        };
    
        streams.push(stream_info);
    }

    let server_meta = ServerMetaDownstream {
        bytes_to_reply_to_client: 50,
        bytes_to_send_to_remote: 32423,
        messages_to_reply_to_client: 4283,
        messages_to_send_to_remote: 237482,
        cpu_usage: 4.2,
        memory_usage_kb: 2091,
        num_open_sockets: 42,
        streams,
    };

    let mut stream_messages = Vec::new();

    for i in 0..50 {
        let message = DownStreamMessage {
            socket_id: i,
            message_sequence_number: i,
            payload: vec![0; 100000],
            has_remote_closed: false,
        };

        stream_messages.push(message);
    }

    let server_message = ServerMessageDownstream {
        metadata: server_meta,
        messages: stream_messages,
    };

    let mut buffer = server_message.encoded().unwrap();

    let out = ServerMessageDownstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(server_message, out);
}

#[test]
fn test_client_upstream_message() {
    let client_meta = ClientMetaUpstream {
        bytes_to_send_to_remote: 32423,
        bytes_to_reply_to_client: 50,
        messages_to_send_to_remote: 237482,
        messages_to_reply_to_client: 4283,
    };

    let mut stream_messages = Vec::new();

    for i in 0..50 {
        let message = UpStreamMessage {
            socket_id: i,
            message_sequence_number: i,
            payload: vec![0; 100000],
            dest_ip: 03402,
            dest_port: 80,
        };

        stream_messages.push(message);
    }

    let mut close_socket_messages = Vec::new();

    for i in 0..50 {
        let message = CloseSocketMessage {
            socket_id: i,
            message_sequence_number: i,
        };

        close_socket_messages.push(message);
    }

    let multiple_messages = MultipleMessagesUpstream {
        stream_messages,
        close_socket_messages,
    };

    let client_message = ClientMessageUpstream {
        metadata: client_meta,
        messages: multiple_messages,
    };

    let mut buffer = client_message.encoded().unwrap();

    let out = ClientMessageUpstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(client_message, out);
}