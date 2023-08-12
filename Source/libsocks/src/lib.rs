use binrw;
use binrw::{BinReaderExt, BinWriterExt};
use binrw::io::{Cursor, Seek, SeekFrom};

pub type IPV4 = u32;
pub type Port = u16;
pub type ConnID = u32;

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq)]
pub enum Socks4Command {
    Connect = 1,
    Bind = 2,
}

#[derive(Debug, PartialEq)]
pub enum Socks4Request {
    Connect(Socks4ConnectRequest),
    Bind(Socks4BindRequest),
}

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq)]
pub enum Socks4Status {
    Granted = 90,
    Rejected = 91,
    RejectedNoIdentd = 92,
    RejectedIdentdMismatch = 93,
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
pub struct Socks4ConnectRequest {
    pub version: u8,
    pub command: Socks4Command,
    pub dstport: Port,
    pub dstip: IPV4,
    pub userid: binrw::NullString,
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
pub struct Socks4ConnectReply {
    pub version: u8,
    pub status: Socks4Status,
    pub dstport: Port,
    pub dstip: IPV4,
}

impl Socks4ConnectReply {
    pub fn to_binary(&self) -> Vec<u8> {
        let mut writer = Cursor::new(Vec::new());
        writer.write_be(self).unwrap();
        writer.into_inner()
    }
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
pub struct Socks4BindRequest {
    pub version: u8,
    pub command: Socks4Command,
    pub dstport: Port,
    pub dstip: IPV4,
    pub userid: binrw::NullString,
}

#[binrw::binrw]
#[derive(Debug, PartialEq)]
pub struct Socks4BindReply {
    pub version: u8,
    pub status: Socks4Status,
    pub dstport: Port,
    pub dstip: IPV4,
}

#[derive(Debug)]
pub enum DecodeCommandError {
    InvalidCommand,
    BinrwError(binrw::Error),
}

pub fn decode_socks_request(data: &[u8]) -> Result<Socks4Request, DecodeCommandError> {
    // Get type based on second byte
    let command_type = match data[1] {
        0x01 => Socks4Command::Connect,
        0x02 => Socks4Command::Bind,
        _ => {
            return Err(DecodeCommandError::InvalidCommand)
        },
    };

    match command_type {
        Socks4Command::Connect => {
            let mut cursor = Cursor::new(data);
            let request: Socks4ConnectRequest = cursor.read_be().map_err(|e| DecodeCommandError::BinrwError(e))?;
            Ok(Socks4Request::Connect(request))
        },
        Socks4Command::Bind => {
            let mut cursor = Cursor::new(data);
            let request: Socks4BindRequest = cursor.read_be().map_err(|e| DecodeCommandError::BinrwError(e))?;
            Ok(Socks4Request::Bind(request))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_decode_request() {
        let sample_data = [
            0x04, 0x01, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x61, 0x62, 0x63, 0x64, 0x00
        ];

        let request = decode_socks_request(&sample_data).unwrap();
        assert_eq!(request, Socks4Request::Connect(Socks4ConnectRequest {
            version: 4,
            command: Socks4Command::Connect,
            dstport: 80,
            dstip: 1,
            userid: binrw::NullString::from("abcd"),
        }));
    }

    #[test]
    fn check_parse_connect() {
        let sample_data = [
            0x04, 0x01, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x61, 0x62, 0x63, 0x64, 0x00
        ];
        
        let mut cursor = Cursor::new(&sample_data);
        let request: Socks4ConnectRequest = cursor.read_be().unwrap();
        assert_eq!(request.version, 4);
        assert_eq!(request.command, Socks4Command::Connect);
        assert_eq!(request.dstport, 80);
        assert_eq!(request.dstip, 1);
        assert_eq!(request.userid.to_string(), "abcd");
    }

    #[test]
    fn check_write_connect() {
        let request = Socks4ConnectRequest {
            version: 4,
            command: Socks4Command::Connect,
            dstport: 80,
            dstip: 1,
            userid: binrw::NullString::from("abcd"),
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&request).unwrap();

        let expected_data = [
            0x04, 0x01, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x61, 0x62, 0x63, 0x64, 0x00
        ];

        assert_eq!(writer.into_inner(), expected_data);
    }

    #[test]
    fn check_write_reply() {
        let reply = Socks4ConnectReply {
            version: 4,
            status: Socks4Status::Granted,
            dstport: 80,
            dstip: 1,
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&reply).unwrap();

        let expected_data = [
            0x04, 0x5a, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01
        ];

        assert_eq!(writer.into_inner(), expected_data);
    }

    #[test]
    fn check_parse_reply() {
        let data = [
            0x04, 0x5a, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01
        ];

        let mut cursor = Cursor::new(&data);
        let reply: Socks4ConnectReply = cursor.read_be().unwrap();
        assert_eq!(reply.version, 4);
        assert_eq!(reply.status, Socks4Status::Granted);
        assert_eq!(reply.dstport, 80);
        assert_eq!(reply.dstip, 1);
    }

    #[test]
    fn check_parse_bind() {
        let data = [
            0x04, 0x02, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x61, 0x62, 0x63, 0x64, 0x00
        ];

        let mut cursor = Cursor::new(&data);
        let request: Socks4BindRequest = cursor.read_be().unwrap();
        assert_eq!(request.version, 4);
        assert_eq!(request.command, Socks4Command::Bind);
        assert_eq!(request.dstport, 80);
        assert_eq!(request.dstip, 1);
        assert_eq!(request.userid.to_string(), "abcd");
    }

    #[test]
    fn check_write_bind() {
        let request = Socks4BindRequest {
            version: 4,
            command: Socks4Command::Bind,
            dstport: 80,
            dstip: 1,
            userid: binrw::NullString::from("abcd"),
        };

        let mut writer = Cursor::new(Vec::new());
        writer.write_be(&request).unwrap();

        let expected_data = [
            0x04, 0x02, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x61, 0x62, 0x63, 0x64, 0x00
        ];

        assert_eq!(writer.into_inner(), expected_data);
    }
}
