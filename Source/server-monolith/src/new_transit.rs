use dashmap::DashMap;
use tokio::sync::mpsc;
use libtransit::{
    UpStreamMessage, DownStreamMessage, ClientMetaUpstream, ServerMetaDownstream,
    SocketID
};

use std::sync::atomic::AtomicU32;

pub struct SessionActorsStorage {
    to_bundler_stream: mpsc::UnboundedSender<UpStreamMessage>,
    from_bundler_stream: mpsc::UnboundedReceiver<Vec<DownStreamMessage>>,
}

pub async fn handle_session(mut from_http_stream: mpsc::UnboundedReceiver<UpStreamMessage>,
to_http_stream: mpsc::UnboundedSender<Vec<DownStreamMessage>>) {
    let stream_to_tcp_handlers: DashMap<SocketID, mpsc::UnboundedSender<UpStreamMessage>> = DashMap::new();
    let stream_from_tcp_handlers: DashMap<SocketID, mpsc::UnboundedReceiver<DownStreamMessage>> = DashMap::new();

    loop {
        let mut no_received = false;

        match from_http_stream.try_recv() {
            Ok(message) => {
                // Get the socket ID
                let socket_id = message.socket_id;

                // Get the TCP handler
                let tcp_handler = stream_to_tcp_handlers.get(&socket_id);

                let tcp_handler = if tcp_handler.is_none() {
                    // Create a new TCP handler
                    // For sending upstreams to the handle_tcp
                    let (to_tcp_handler, from_http_stream) = mpsc::unbounded_channel();
                    // For receiving downstreams from the handle_tcp
                    let (to_http_stream, from_tcp_handler) = mpsc::unbounded_channel();

                    // Add the TCP handler to the map
                    stream_to_tcp_handlers.insert(socket_id, to_tcp_handler);
                    stream_from_tcp_handlers.insert(socket_id, from_tcp_handler);

                    // Spawn the TCP handler
                    tokio::spawn(handle_tcp(from_http_stream, to_http_stream));

                    // Get the TCP handler
                    let tcp_handler = stream_to_tcp_handlers.get(&socket_id).unwrap();
                    tcp_handler
                } else {
                    tcp_handler.unwrap()
                };

                // Send the message to the TCP handler
                tcp_handler.send(message).unwrap();
            }
            Err(e) => {
                // Check if the channel is empty
                if e == mpsc::error::TryRecvError::Empty {
                    no_received = true;
                } else {
                    // Panic
                    panic!("Error receiving from HTTP stream: {:?}", e);
                }
            }
        }
    }
}

pub async fn handle_tcp(from_http_stream: mpsc::UnboundedReceiver<UpStreamMessage>, to_http_stream: mpsc::UnboundedSender<DownStreamMessage>) {

}