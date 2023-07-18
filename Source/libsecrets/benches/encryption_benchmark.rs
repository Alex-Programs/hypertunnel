use criterion::{criterion_group, criterion_main, Criterion};
use libsecrets::{encrypt, form_key};

fn criterion_benchmark(c: &mut Criterion) {
    let key = form_key(b"01234567890123456789012345678901");

    c.bench_function("encrypt_small", |b| b.iter(|| {
        let data = b"Hello world!";
        let encrypted = encrypt(data, &key).unwrap();
    }));

    let data_1kb = b"12345678".repeat(128); // 8 * 128 = 1024

    c.bench_function("encrypt_1kb", |b| b.iter(|| {
        let data = data_1kb.as_slice();
        let encrypted = encrypt(data, &key).unwrap();
    }));

    let data_1mb = b"12345678".repeat(128 * 1024); // 8 * 128 * 1024 = 1048576

    c.bench_function("encrypt_1mb", |b| b.iter(|| {
        let data = data_1mb.as_slice();
        let encrypted = encrypt(data, &key).unwrap();
    }));

    let data_10mb = b"12345678".repeat(128 * 1024 * 10); // 8 * 128 * 1024 * 10 = 10485760

    c.bench_function("encrypt_10mb", |b| b.iter(|| {
        let data = data_10mb.as_slice();
        let encrypted = encrypt(data, &key).unwrap();
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);