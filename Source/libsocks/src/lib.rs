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
}
