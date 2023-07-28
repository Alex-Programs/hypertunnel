use rand::Rng; // Not unused - ignore vscode
use borsh::{BorshSerialize, BorshDeserialize};

pub type Port = u16;
pub type IPV4 = u32;
pub type SocketID = u32;

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

#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct MultipleMessagesDownstream {
    pub stream_messages: Vec<DownStreamMessage>,
}

impl MultipleMessagesDownstream {
    pub fn encoded(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let data = self.try_to_vec()?;
        Ok(data)
    }

    pub fn decode_from_bytes(data: &mut Vec<u8>) -> Result<Self, std::io::Error> { // Borsh returns std::io::Error
        Self::try_from_slice(data)
    }
}

#[test]
fn test_multiple_messages_downstream() {
    let large_payload = vec![0; 100000];

    let mut stream_messages = Vec::new();

    for i in 0..50 {
        let message = DownStreamMessage {
            socket_id: i,
            message_sequence_number: i,
            payload: large_payload.clone(),
        };

        stream_messages.push(message);
    }

    let multiple_messages = MultipleMessagesDownstream {
        stream_messages,
    };

    let mut buffer = multiple_messages.encoded().unwrap();

    let out = MultipleMessagesDownstream::decode_from_bytes(&mut buffer).unwrap();

    assert_eq!(multiple_messages, out);
}