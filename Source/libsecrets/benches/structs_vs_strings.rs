use criterion::{criterion_group, criterion_main, Criterion};

struct ErrorStruct {
    error_type: u32,
    error_value: u32,
    idk_what_goes_here: u32,
}

struct OtherErrorType {
    thing: u64,
}

enum ErrorEnum {
    ErrorStruct(ErrorStruct),
    OtherErrorType(OtherErrorType),
}

fn create_structs() {
    let mut fill = Vec::with_capacity(1_000_000);
    for i in 0..1_000_000 {
        let error = ErrorEnum::ErrorStruct(ErrorStruct {
            error_type: 1,
            error_value: 2,
            idk_what_goes_here: 3,
        });
        fill.push(error);
    }
}

fn create_strings() {
    let mut fill = Vec::with_capacity(1_000_000);
    for i in 0..1_000_000 {
        let error = String::from("Hello world! I'm doing a string memory allocation here! :D");
        fill.push(error);
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("fill_with_structs", |b| {
        b.iter(|| {
            create_structs();
        })
    });

    c.bench_function("fill_with_strings", |b| {
        b.iter(|| {
            create_strings();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);