use std::collections::HashMap;
use crossbeam::deque::{Injector, Stealer, Worker};
use libsocks;
use std::net::TcpListener;
use binrw::io::{Cursor};

const BUFFSIZE: usize = 1024;

#[derive(Debug)]
struct SendStreamBuffer {
    socksconnid: libsocks::ConnID,
    data: [u8; BUFFSIZE],
}

#[derive(Debug)]
struct SendConnRequest {
    socksconnid: libsocks::ConnID,
    command: libsocks::Socks4Command,
    dstport: libsocks::Port,
    dstip: libsocks::IPV4,
}

#[derive(Debug)]
enum ToSendBuffer {
    SendStreamBuffer(SendStreamBuffer),
    SendConnRequest(SendConnRequest),
}

#[derive(Debug)]
struct ReturnStreamBuffer {
    socksconnid: libsocks::ConnID,
    data: [u8; BUFFSIZE],
}

type ReturnInjectorVec = Vec<ReturnStreamBuffer>;

pub fn entrypoint() {
    // We will have a shared to-send queue and a shared received-to-be-returned queue per SOCKS connection using crossbeam. This is given to the TCP SOCKS handler and transit.
    let to_send: Injector<ToSendBuffer> = Injector::new();
    let return_injector_map: ReturnInjectorVec = Vec::new();

    // Initialise the TCP SOCKS handler

}

fn socks_listener(to_send_buffer: &Injector<ToSendBuffer>, return_injector_vec: &ReturnInjectorVec) {
    let mut next_socks_id: libsocks::ConnID = 0;
    
    // Listen for incoming TCP connections on port 1080
    let listener = TcpListener::bind("0.0.0.0:1080").unwrap();

    // Accept connections and process them, spawning a new thread for each one
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // Parse the SOCKS request
                let mut cursor = Cursor::new(&stream);
                let request: libsocks::Socks4ConnectRequest = cursor.read_be().unwrap();

                // Check the version is 4
                if request.version != 4 {
                    println!("Error: SOCKS version is not 4");
                    continue;
                }

                // Check the command is 1 (CONNECT)
                if request.command != libsocks::Socks4Command::Connect {
                    println!("Error: SOCKS command is not CONNECT");
                    continue;
                }

                let socks_id = next_socks_id;
                next_socks_id += 1;

                let return_injector = Injector<ReturnStreamBuffer>::new();
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}