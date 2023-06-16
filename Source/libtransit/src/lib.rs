use binrw;
use binrw::{BinReaderExt, BinWriterExt};
use binrw::io::{Cursor, Seek, SeekFrom};
use rand::Rng;

pub type Port = u16;
pub type IPV4 = u32;

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq)]
pub enum MessageType {
    CreateSocket = 0,
    CloseSocket = 1,
    SendStreamPacket = 2,
    ReceiveStreamPacket = 3,
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
#[brw(big)]
pub struct StreamMessage {
    pub message_type: MessageType,
    pub socket_id: u32,

    pub send_buffer_size: u32,
    pub receive_buffer_size: u32,

    #[bw(try_calc(u32::try_from(packet_data.len())))]
    pub packet_length: u32,
    #[br(count = packet_length)]
    pub packet_data: Vec<u8>,
}

impl StreamMessage {
    pub fn encoded(socket_id: u32, send_buffer_size: u32, receive_buffer_size: u32, packet_data: Vec<u8>) -> Vec<u8> {
        let data = Self {
            message_type: MessageType::SendStreamPacket,
            socket_id,
            send_buffer_size,
            receive_buffer_size,
            packet_data,
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&data).unwrap();
        
        return writer.into_inner();
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut reader = Cursor::new(bytes);
        return reader.read_be().unwrap();
    }
}

#[test]
fn test_stream_message() {
    let mut rng = rand::thread_rng();
    let packet_data: Vec<u8> = (0..rng.gen_range(0..100)).map(|_| rng.gen()).collect();

    let data = StreamMessage {
        message_type: MessageType::SendStreamPacket,
        socket_id: 0,
        send_buffer_size: 0,
        receive_buffer_size: 0,
        packet_data: packet_data,
    };

    let mut writer = Cursor::new(Vec::new());
    writer.write_be(&data).unwrap();
    let bytes = writer.into_inner();

    let mut reader = Cursor::new(bytes);
    let read_data = reader.read_be::<StreamMessage>().unwrap();

    assert_eq!(data, read_data);
    assert_eq!(data.packet_data, read_data.packet_data);
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
#[brw(big)]
pub struct CreateSocketMessage {
    pub message_type: MessageType,
    pub socket_id: u32,
    pub forwarding_address: IPV4,
    pub forwarding_port: Port,
}

impl CreateSocketMessage {
    pub fn encoded(socket_id: u32, forwarding_address: IPV4, forwarding_port: Port) -> Vec<u8> {
        let data = Self {
            message_type: MessageType::CreateSocket,
            socket_id,
            forwarding_address,
            forwarding_port,
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&data).unwrap();
        
        return writer.into_inner();
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut reader = Cursor::new(bytes);
        return reader.read_be().unwrap();
    }
}

#[test]
fn test_create_socket_message() {
    let data = CreateSocketMessage {
        message_type: MessageType::CreateSocket,
        socket_id: 0,
        forwarding_address: 5,
        forwarding_port: 0,
    };

    let mut writer = Cursor::new(Vec::new());
    writer.write_be(&data).unwrap();
    let bytes = writer.into_inner();

    let mut reader = Cursor::new(bytes);
    let read_data = reader.read_be::<CreateSocketMessage>().unwrap();

    assert_eq!(data, read_data);
    assert_eq!(data.forwarding_address, read_data.forwarding_address);
    assert_eq!(data.forwarding_port, read_data.forwarding_port);
    assert_eq!(data.socket_id, read_data.socket_id);
    assert_eq!(data.message_type, read_data.message_type);
    assert_eq!(read_data.message_type, MessageType::CreateSocket);
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
#[brw(big)]
pub struct CloseSocketMessage {
    pub message_type: MessageType,
    pub socket_id: u32,
}

impl CloseSocketMessage {
    pub fn encoded(socket_id: u32) -> Vec<u8> {
        let data = Self {
            message_type: MessageType::CloseSocket,
            socket_id,
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&data).unwrap();
        
        return writer.into_inner();
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut reader = Cursor::new(bytes);
        return reader.read_be().unwrap();
    }
}

#[test]
fn test_close_socket_message() {
    let data = CloseSocketMessage {
        message_type: MessageType::CloseSocket,
        socket_id: 5,
    };

    let mut writer = Cursor::new(Vec::new());
    writer.write_be(&data).unwrap();
    let bytes = writer.into_inner();

    let mut reader = Cursor::new(bytes);
    let read_data = reader.read_be::<CloseSocketMessage>().unwrap();

    assert_eq!(data, read_data);
    assert_eq!(data.socket_id, read_data.socket_id);
    assert_eq!(data.message_type, read_data.message_type);
    assert_eq!(read_data.message_type, MessageType::CloseSocket);
}