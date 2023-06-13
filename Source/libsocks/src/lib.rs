use binrw;
use binrw::{BinReaderExt, BinWriterExt};
use binrw::io::{Cursor, Seek, SeekFrom};

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq)]
enum Socks4Command {
    Connect = 1,
    Bind = 2,
}

#[binrw::binrw]
#[brw(repr(u8))]
#[derive(Debug, PartialEq)]
enum Socks4Status {
    Granted = 90,
    Rejected = 91,
    RejectedNoIdentd = 92,
    RejectedIdentdMismatch = 93,
}

#[binrw::binrw]
struct Socks4ConnectRequest {
    version: u8,
    command: Socks4Command,
    dstport: u16,
    dstip: u32,
    userid: binrw::NullString,
}

#[binrw::binrw]
struct Socks4ConnectReply {
    version: u8,
    status: Socks4Status,
    dstport: u16,
    dstip: u32,
}

#[binrw::binrw]
struct Socks4BindRequest {
    version: u8,
    command: Socks4Command,
    dstport: u16,
    dstip: u32,
    userid: binrw::NullString,
}

#[binrw::binrw]
struct Socks4BindReply {
    version: u8,
    status: Socks4Status,
    dstport: u16,
    dstip: u32,
}


#[cfg(test)]
mod tests {
    use super::*;

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

    // TODO add tests for Socks4ConnectReply (read, write)

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
