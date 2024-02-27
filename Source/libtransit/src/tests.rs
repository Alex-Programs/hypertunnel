use crate::*;

#[test]
fn test_generate_socket_id() {
    let socket_id = generate_socket_id();
    assert_eq!(socket_id, 0);
    let socket_id = generate_socket_id();
    assert_eq!(socket_id, 1);
}

#[test]
fn test_client_upstream() {
    let msg = ClientMessageUpstream {
        metadata: ClientMetaUpstream {
            packet_info: UnifiedPacketInfo {
                unix_ms: 0,
                seq_num: 0,
            },
            traffic_stats: ClientMetaUpstreamTrafficStats {
                socks_to_coordinator_bytes: 0,
                coordinator_to_request_buffer_bytes: 0,
                coordinator_to_request_channel_bytes: 0,
                up_request_in_progress_bytes: 0,
                response_to_socks_bytes: 0,
            },
            set: None,
            yellow_to_stop_reading_from: vec![0],
        },
        socks_sockets: vec![SocksSocketUpstream {
            socket_id: 0,
            dest_ip: 0,
            dest_port: 0,
            payload: vec![0, 1, 2, 3],
            red_terminate: false,
        }],
        payload_size: 4,
    };
    let encoded = msg.encoded().unwrap();
    let decoded = ClientMessageUpstream::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

#[test]
fn test_upstream_msg() {
    let msg = UpStreamMessage {
        socket_id: 0,
        dest_ip: 0,
        dest_port: 0,
        payload: vec![0, 1, 2, 3],
        red_terminate: false,
    };
    let encoded = msg.encoded().unwrap();
    let decoded = UpStreamMessage::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

#[test]
fn test_downstream_msg() {
    let msg = DownStreamMessage {
        socket_id: 0,
        payload: vec![0, 1, 2, 3],
        do_green_terminate: false,
    };
    let encoded = msg.encoded().unwrap();
    let decoded = DownStreamMessage::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}

#[test]
fn test_server_message_downstream() {
    let msg = ServerMessageDownstream {
        metadata: ServerMetaDownstream {
            packet_info: UnifiedPacketInfo {
                unix_ms: 0,
                seq_num: 0,
            },
            traffic_stats: ServerMetaDownstreamTrafficStats {
                http_up_to_coordinator_bytes: 0,
                coordinator_up_to_socket_bytes: 0,
                socket_down_to_coordinator_bytes: 0,
                coordinator_down_to_http_message_passer_bytes: 0,
                coordinator_down_to_http_buffer_bytes: 0,
                congestion_ctrl_intake_throttle: 0,
            },
            server_stats: ServerMetaDownstreamStats {
                cpu_usage: 0.0,
                memory_usage_kb: 0,
            },
            logs: vec![ServerMetaDownstreamLog {
                timestamp: 0,
                severity: ServerMetaDownstreamLogSeverity::Info,
                message: "test".to_string(),
            }],
        },
        socks_sockets: vec![SocksSocketDownstream {
            socket_id: 0,
            dest_ip: 0,
            dest_port: 0,
            payload: vec![0, 1, 2, 3],
            do_green_terminate: false,
        }],
        payload_size: 4,
    };
    let encoded = msg.encoded().unwrap();
    let decoded = ServerMessageDownstream::decode_from_bytes(&mut encoded.clone()).unwrap();
    assert_eq!(msg, decoded);
}