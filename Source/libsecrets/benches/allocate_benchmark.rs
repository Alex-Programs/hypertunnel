use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    let data = b"Hello world!";
    c.bench_function("alloc_small", |b| b.iter(|| {
        let other = Box::new(data.clone());
    }));

    let data_1kb = b"12345678".repeat(128); // 8 * 128 = 1024

    c.bench_function("alloc_1kb", |b| b.iter(|| {
        let other = Box::new(data_1kb.clone());
    }));

    let data_1mb = b"12345678".repeat(128 * 1024); // 8 * 128 * 1024 = 1048576

    c.bench_function("alloc_1mb", |b| b.iter(|| {
        let other = Box::new(data_1mb.clone());
    }));

    let data_10mb = b"12345678".repeat(128 * 1024 * 10); // 8 * 128 * 1024 * 10 = 10485760

    c.bench_function("alloc_10mb", |b| b.iter(|| {
        let other = Box::new(data_10mb.clone());
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);