use criterion::{criterion_group, criterion_main, Criterion};

use libtransit::{Message, CreateSocketMessage, StreamMessage, CloseSocketMessage, MessageType, MultipleMessages};

fn gen_data(size: usize) -> Vec<u8> {
    Vec::from_iter((0..(size)).map(|_| rand::random::<u8>()))
}

fn stress_test_multiple_messages(messagecount: u32, data: Vec<u8>) {
    let mut messages: Vec<Message> = Vec::new();

    for i in 0..messagecount {
        messages.push(Message::CreateSocketMessage(CreateSocketMessage {
            message_type: MessageType::CreateSocket,
            socket_id: i,
            forwarding_address: 43,
            forwarding_port: 80,
        }));

        messages.push(Message::StreamMessage(StreamMessage {
            message_type: MessageType::SendStreamPacket,
            socket_id: i,
            send_buffer_size: 0,
            receive_buffer_size: 0,
            packet_data: data.clone(),
        }));

        messages.push(Message::CloseSocketMessage(CloseSocketMessage {
            message_type: MessageType::CloseSocket,
            socket_id: i,
        }));
    }

    let multiple_messages = MultipleMessages::from_messages(messages.clone());

    let buffer = multiple_messages.to_bytes();

    let multiple_messages = MultipleMessages::from_bytes(buffer);

    let messages = multiple_messages.pull_messages();

    assert_eq!(messages, messages);
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("stress_test_multiple_messages_50mc_100b    (5kb)", |b| b.iter(|| stress_test_multiple_messages(50, gen_data(100))));
    c.bench_function("stress_test_multiple_messages_50mc_1kb     (50kb)", |b| b.iter(|| stress_test_multiple_messages(50, gen_data(1000))));
    c.bench_function("stress_test_multiple_messages_50mc_10kb    (500kb)", |b| b.iter(|| stress_test_multiple_messages(50, gen_data(10_000))));
    c.bench_function("stress_test_multiple_messages_50mc_100kb   (5mb)", |b| b.iter(|| stress_test_multiple_messages(50, gen_data(100_000))));

    c.bench_function("stress_test_multiple_messages_10mc_1mb   (10mb)", |b| b.iter(|| stress_test_multiple_messages(10, gen_data(1_000_000))));
    c.bench_function("stress_test_multiple_messages_10mc_10mb  (100mb)", |b| b.iter(|| stress_test_multiple_messages(10, gen_data(10_000_000))));
    c.bench_function("stress_test_multiple_messages_10mc_100mb (1000mb)", |b| b.iter(|| stress_test_multiple_messages(10, gen_data(100_000_000))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);