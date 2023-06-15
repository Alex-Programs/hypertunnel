pub type VirtualSocketID = u32;

pub struct TransitController {
    virtual_sockets: Vec<VirtualSocketID>,
    pending_send_packets: Vec<Packet>,
    pending_receive_pakcets: Vec<Packet>,
    destination: String,
}

pub struct Packet {
    socket: VirtualSocketID,
    data: Vec<u8>,
}

impl TransitController {
    fn new() -> Self {
        Self {
            virtual_sockets: Vec::new(),
            pending_send_packets: Vec::new(),
        }

        // TODO create a load of HTTP requests and some receive pollers
    }

    fn create_socket(&mut self) -> VirtualSocketID {
        let new_socket_id = self.virtual_sockets.len() as VirtualSocketID;
        self.virtual_sockets.push(new_socket_id);

        // TODO push a new packet telling the server about the new socket

        new_socket_id
    }

    fn send(&mut self, socket: VirtualSocketID, data: Vec<u8>) {
        self.pending_send_packets.push(Packet {
            socket,
            data,
        });
    }

    fn receive(&mut self, socket: VirtualSocketID) -> Vec<u8> {
        // TODO check if there are any packets for this socket and send back. Don't keep polling
        // when unnecessary
    }

    fn get_send_congestion() -> f32 {
        // TODO. Use data returned from the server as well
    }
}

// TODO builder pattern