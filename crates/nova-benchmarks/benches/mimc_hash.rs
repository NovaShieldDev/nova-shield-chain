/// MiMC hash fonksiyonu benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - mimc_hash(a, b)           — tek hash (BLS12-381 Fr skalerleri)
/// - mimc_sponge(inputs)       — sponge modu (1k / 10k / 100k eleman)
/// - mimc_merkle_hash          — ikili Merkle düğümü
/// - mimc_merkle_root          — 20 seviyeli Merkle yolu
/// - mimc_nullifier            — nullifier türetimi
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_zk::circuits::mimc::{
    mimc_hash, mimc_sponge, mimc_merkle_hash, mimc_merkle_root, mimc_nullifier,
};
use ark_bls12_381::Fr;
use ark_ff::UniformRand;

fn bench_mimc_hash_single(c: &mut Criterion) {
    let a = Fr::from(12345678u64);
    let b = Fr::from(87654321u64);

    c.bench_function("mimc_hash_single", |b_| {
        b_.iter(|| mimc_hash(a, b))
    });
}

fn bench_mimc_sponge(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let sizes = [1_000usize, 10_000, 100_000];

    let mut group = c.benchmark_group("mimc_sponge");
    for &n in &sizes {
        let inputs: Vec<Fr> = (0..n).map(|_| Fr::rand(&mut rng)).collect();

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &inputs, |b_, inp| {
            b_.iter(|| mimc_sponge(inp))
        });
    }
    group.finish();
}

fn bench_mimc_merkle_hash(c: &mut Criterion) {
    let left  = [0x01u8; 32];
    let right = [0x02u8; 32];

    c.bench_function("mimc_merkle_hash", |b| {
        b.iter(|| mimc_merkle_hash(&left, &right))
    });
}

fn bench_mimc_merkle_root(c: &mut Criterion) {
    let leaf = [0xABu8; 32];
    let depth_20_path: Vec<([u8; 32], bool)> = (0..20)
        .map(|i| ([i as u8; 32], i % 2 == 0))
        .collect();

    c.bench_function("mimc_merkle_root_depth20", |b| {
        b.iter(|| mimc_merkle_root(&leaf, &depth_20_path))
    });
}

fn bench_mimc_nullifier(c: &mut Criterion) {
    let nk = [0xCAu8; 32];
    let cm = [0xFEu8; 32];

    c.bench_function("mimc_nullifier", |b| {
        b.iter(|| mimc_nullifier(&nk, &cm))
    });
}

criterion_group!(
    benches,
    bench_mimc_hash_single,
    bench_mimc_sponge,
    bench_mimc_merkle_hash,
    bench_mimc_merkle_root,
    bench_mimc_nullifier,
);
criterion_main!(benches);
