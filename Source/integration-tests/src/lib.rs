use client_transit::{TransitSocket, TransitSocketBuilder};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_socket_connect() {
        let mut socket = TransitSocketBuilder::new()
            .with_target("http://localhost:8000".to_string())
            .with_password("12345".to_string())
            .with_timeout_time(1)
            .build();

        socket.connect().expect("Failed to connect to server");
    }
}
