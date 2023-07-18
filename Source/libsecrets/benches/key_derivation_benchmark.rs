use criterion::{criterion_group, criterion_main, Criterion};
use libsecrets::{encrypt, form_key};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("key_derivation", |b| b.iter(|| {
        let key = form_key(b"01234567890123456789012345678901");
    }));

}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);