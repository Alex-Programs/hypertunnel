use libsocks;
use client_transit;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Sender, Receiver};

struct ClientArguments {
    listen_address: String,
    listen_port: u16,
    target_host: String,
    password: String,
}

// Core of the client program for now. See Obsidian
async fn listen_and_relay(arguments: ClientArguments) {
    let transit_socket = client_transit::TransitSocketBuilder::new()
        .with_target(arguments.target_host)
        .with_password(arguments.password)
        .with_client_name("Client-Core".to_string())
        .build();

    let status = transit_socket.connect().await;

    match status {
        Ok(_) => {
            println!("Connected to server!");
        },
        Err(error) => {
            println!("Failed to connect to server: {:?}", error);
            return;
        }
    }


}