use criterion::{criterion_group, criterion_main, Criterion};

use libtransit::{Message, UpStreamMessage, DownStreamMessage, MultipleMessagesUpstream, MultipleMessagesDownstream, CloseSocketMessage};

fn gen_data(size: usize) -> Vec<u8> {
    // Generate random data
    let mut data: Vec<u8> = Vec::with_capacity(size);

    for _ in 0..size {
        data.push(rand::random::<u8>());
    }

    data
}

fn gen_upstream_messages(messagecount: u32, data: Vec<u8>) -> Vec<UpStreamMessage> {
    let mut messages: Vec<UpStreamMessage> = Vec::new();

    for i in 0..messagecount {
        let message = UpStreamMessage {
            socket_id: i,
            message_sequence_number: i,
            dest_ip: 0,
            dest_port: 0,
            payload: data.clone(),
        };

        messages.push(message);
    }

    messages
}

fn gen_close_sockets(messagecount: u32) -> Vec<CloseSocketMessage> {
    let mut messages: Vec<CloseSocketMessage> = Vec::new();

    for i in 0..messagecount {
        let message = CloseSocketMessage {
            socket_id: i,
            message_sequence_number: i,
        };

        messages.push(message);
    }

    messages
}

fn stress_test_multiple_messages_upstream(upstreams: Vec<UpStreamMessage>, close_sockets: Vec<CloseSocketMessage>) {
    let multiple_messages_up = MultipleMessagesUpstream {
        stream_messages: upstreams,
        close_socket_messages: close_sockets,
    };

    let mut encoded = multiple_messages_up.encoded().unwrap();

    let decoded = MultipleMessagesUpstream::decode_from_bytes(&mut encoded).unwrap();

    assert_eq!(multiple_messages_up, decoded);
}

fn criterion_benchmark(c: &mut Criterion) {
    let stream_messages = gen_upstream_messages(50, gen_data(100));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_100b    (5kb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));
    
    let stream_messages = gen_upstream_messages(50, gen_data(1_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_1kb     (50kb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));
    
    let stream_messages = gen_upstream_messages(50, gen_data(10_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_10kb    (500kb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));

    let stream_messages = gen_upstream_messages(50, gen_data(100_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_100kb   (5mb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));

    let stream_messages = gen_upstream_messages(10, gen_data(1_000_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_10mc_1mb     (10mb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));
    
    let stream_messages = gen_upstream_messages(10, gen_data(10_000_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_10mb    (100mb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));

    let stream_messages = gen_upstream_messages(10, gen_data(100_000_000));
    let close_socket_messages = gen_close_sockets(10);
    c.bench_function("stress_test_multiple_messages_50mc_100mb   (1gb)", |b| b.iter(|| stress_test_multiple_messages_upstream(stream_messages.clone(), close_socket_messages.clone())));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);