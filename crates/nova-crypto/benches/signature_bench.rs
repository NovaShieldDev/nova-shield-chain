use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nova_crypto::signatures::ecdsa::EcdsaKeypair;
use nova_crypto::hash::blake3_hash;

fn bench_ecdsa(c: &mut Criterion) {
    let kp = EcdsaKeypair::generate().unwrap();
    let hash = blake3_hash(b"benchmark message");
    c.bench_function("ecdsa_sign", |b| {
        b.iter(|| kp.sign_hash(black_box(&hash)))
    });
}

criterion_group!(benches, bench_ecdsa);
criterion_main!(benches);
