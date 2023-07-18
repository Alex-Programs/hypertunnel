use criterion::{criterion_group, criterion_main, Criterion};
use libsecrets::{encrypt, form_key, decrypt};

fn criterion_benchmark(c: &mut Criterion) {
    let key = form_key(b"01234567890123456789012345678901");
    let data = b"Hello world!";
    let encrypted = encrypt(data, &key).unwrap();

    c.bench_function("decrypt_small", |b| b.iter(|| {
        let decrypted = decrypt(&encrypted, &key);
    }));

    let data_1kb = b"12345678".repeat(128); // 8 * 128 = 1024
    let encrypted = encrypt(data_1kb.as_slice(), &key).unwrap();

    c.bench_function("decrypt_1kb", |b| b.iter(|| {
        let decrypted = decrypt(&encrypted, &key);
    }));

    let data_1mb = b"12345678".repeat(128 * 1024); // 8 * 128 * 1024 = 1048576
    let encrypted = encrypt(data_1mb.as_slice(), &key).unwrap();

    c.bench_function("decrypt_1mb", |b| b.iter(|| {
        let decrypted = decrypt(&encrypted, &key);
    }));

    let data_10mb = b"12345678".repeat(128 * 1024 * 10); // 8 * 128 * 1024 * 10 = 10485760
    let encrypted = encrypt(data_10mb.as_slice(), &key).unwrap();

    c.bench_function("decrypt_10mb", |b| b.iter(|| {
        let decrypted = decrypt(&encrypted, &key);
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);