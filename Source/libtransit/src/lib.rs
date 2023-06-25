use binrw;
use binrw::{BinReaderExt, BinWriterExt};
use binrw::io::{Cursor, Seek, SeekFrom};
use rand::Rng;

pub type Port = u16;
pub type IPV4 = u32;

// We want to encode multiple messages, which could be
// either a stream, create, or close, into a single buffer.
// We need to be able to decode the buffer into the list of
// messages, with the correct data. We should not waste data
// on padding. We will begin with the length of the offsets list,
// and then the offsets list, and then the data for each message at
// the specified offsets from the end of the offsets list.

#[binrw::binrw]
#[derive(Debug, PartialEq)]
#[brw(big)]
pub struct MultipleMessages {
    #[bw(try_calc(u8::try_from(sizes.len())))]
    pub sizes_length: u8,
    #[br(count = sizes_length)]
    pub sizes: Vec<u32>,

    #[bw(try_calc(u32::try_from(data.len())))]
    pub data_length: u32,
    #[br(count = data_length)]
    pub data: Vec<u8>,
}

impl MultipleMessages {
    pub fn from_messages(messages: Vec<Message>) -> Self {
        if messages.len() > u8::MAX as usize {
            panic!("Cannot encode more than 255 messages into a single buffer.");
        }
    
        let mut sizes: Vec<u32> = Vec::with_capacity(messages.len());
        let mut data: Vec<u8> = Vec::new();
    
        for message in messages {
            let message_data = match message {
                Message::StreamMessage(stream_message) => {
                    let mut writer = Cursor::new(Vec::new());
                    writer.write_be(&stream_message).unwrap();
                    writer.into_inner()
                },
                Message::CreateSocketMessage(create_socket_message) => {
                    let mut writer = Cursor::new(Vec::new());
                    writer.write_be(&create_socket_message).unwrap();
                    writer.into_inner()
                },
                Message::CloseSocketMessage(close_socket_message) => {
                    let mut writer = Cursor::new(Vec::new());
                    writer.write_be(&close_socket_message).unwrap();
                    writer.into_inner()
                },
            };
    
            sizes.push(message_data.len() as u32);
            data.extend(message_data);
        }
    
        Self {
            sizes,
            data
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // Write the data to a multiple messages buffer
        let buffer = Vec::with_capacity(1 + self.sizes.len() * 4 + self.data.len());
        let mut writer = Cursor::new(buffer);

        writer.write_be(&self).unwrap();

        writer.into_inner()
    }

    pub fn from_bytes(buffer: Vec<u8>) -> Self {
        let mut reader = Cursor::new(buffer);
        reader.read_be::<MultipleMessages>().unwrap()
    }

    pub fn pull_messages(&self) -> Vec<Message> {
        let mut messages: Vec<Message> = Vec::with_capacity(self.sizes.len());

        let mut i = 0;
        let mut base_offset = 0;

        println!("Self.data: {:?} (Length {})", self.data, self.data.len());

        loop {
            let data_start = base_offset;
            let data_end = base_offset + self.sizes[i] as usize;

            println!("Data start: {}", data_start);
            println!("Data end: {}", data_end);
            println!("Sizes: {:?}", self.sizes);
            println!("Current offset: {}", i);

            // Get those bytes
            let message_data = self.data[data_start..data_end].to_vec();

            // Read to a message struct
            println!("Message data: {:?}", message_data);
            let mut data_reader = Cursor::new(message_data);

            let messagetype_reader = self.data[data_start];

            let message_type = match messagetype_reader {
                0 => MessageType::CreateSocket,
                1 => MessageType::CloseSocket,
                2 => MessageType::SendStreamPacket,
                3 => MessageType::ReceiveStreamPacket,
                _ => panic!("Invalid message type"),
            };

            let message = match message_type {
                MessageType::CreateSocket => {
                    Message::CreateSocketMessage(data_reader.read_be::<CreateSocketMessage>().unwrap())
                },
                MessageType::CloseSocket => {
                    Message::CloseSocketMessage(data_reader.read_be::<CloseSocketMessage>().unwrap())
                },
                MessageType::SendStreamPacket => {
                    Message::StreamMessage(data_reader.read_be::<StreamMessage>().unwrap())
                },
                MessageType::ReceiveStreamPacket => {
                    Message::StreamMessage(data_reader.read_be::<StreamMessage>().unwrap())
                },
            };

            // Push to messages
            messages.push(message);

            // Check if we should stop
            if i > self.sizes.len() - 2 {
                break;
            }

            i = i + 1;
            base_offset = data_end;
        }

        messages
    }
}

#[test]
fn test_multiple_message_writing_reading() {
    let mut messages: Vec<Message> = Vec::new();

    messages.push(Message::CreateSocketMessage(CreateSocketMessage {
        message_type: MessageType::CreateSocket,
        socket_id: 5,
        forwarding_address: 43,
        forwarding_port: 80,
    }));

    // Write message to bytes
    let mut writer = Cursor::new(Vec::new());
    writer.write_be(&CreateSocketMessage {
        message_type: MessageType::CreateSocket,
        socket_id: 5,
        forwarding_address: 43,
        forwarding_port: 80,
    }).unwrap();
    let message_bytes = writer.into_inner();
    println!("Message bytes: {:?}", message_bytes);

    messages.push(Message::StreamMessage(StreamMessage {
        message_type: MessageType::SendStreamPacket,
        socket_id: 5,
        send_buffer_size: 0,
        receive_buffer_size: 0,
        packet_data: vec![1, 2, 3, 4, 5],
    }));

    messages.push(Message::CloseSocketMessage(CloseSocketMessage {
        message_type: MessageType::CloseSocket,
        socket_id: 5,
    }));

    let multiple_messages = MultipleMessages::from_messages(messages.clone());

    let buffer = multiple_messages.to_bytes();

    let multiple_messages = MultipleMessages::from_bytes(buffer);

    let messages = multiple_messages.pull_messages();

    assert_eq!(messages, messages);
}

#[derive(Debug, PartialEq, Clone)]
pub enum Message {
    StreamMessage(StreamMessage),
    CreateSocketMessage(CreateSocketMessage),
    CloseSocketMessage(CloseSocketMessage),
}

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq, Clone)]
pub enum MessageType {
    CreateSocket = 0,
    CloseSocket = 1,
    SendStreamPacket = 2,
    ReceiveStreamPacket = 3,
}

#[binrw::binrw]
#[derive(Debug, PartialEq, Clone)]
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
#[derive(Debug, PartialEq, Clone)]
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
#[derive(Debug, PartialEq, Clone)]
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