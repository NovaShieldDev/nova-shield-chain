use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nova_crypto::hash::blake3_hash;

fn bench_blake3(c: &mut Criterion) {
    let data = vec![0u8; 1024];
    c.bench_function("blake3_1kb", |b| {
        b.iter(|| blake3_hash(black_box(&data)))
    });
}

criterion_group!(benches, bench_blake3);
criterion_main!(benches);
