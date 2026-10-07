/// Ağ katmanı benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - ShredBuilder::shred_block()    — blok parçalama (1MB / 10MB)
/// - ShredBuilder::deshred()        — parça birleştirme
/// - ReceivedCache deduplication    — gossip mesaj tekilleştirme (1k / 10k)
/// - ErasureEncoder::encode()       — Reed-Solomon kodlama
/// - ErasureDecoder::decode()       — Reed-Solomon kurtarma (veri kaybıyla)
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_network::{
    ErasureConfig, ErasureEncoder, ErasureDecoder, Shred, ShredBuilder, SHRED_DATA_PAYLOAD_SIZE,
};
use nova_network::turbine::ShredType;
use nova_network::gossip::push::ReceivedCache;

// ─── Blok parçalama ──────────────────────────────────────────────────────────

fn bench_shred_block(c: &mut Criterion) {
    let leader = [0x01u8; 32];
    let builder = ShredBuilder::new(leader, 1024); // 1KB shred boyutu

    let block_sizes = [512 * 1024usize, 1_024 * 1024, 10 * 1024 * 1024]; // 512KB, 1MB, 10MB

    let mut group = c.benchmark_group("shred_block");
    for &size in &block_sizes {
        let data = vec![0xABu8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("size_bytes", size),
            &data,
            |b, d| b.iter(|| builder.shred_block(1, d)),
        );
    }
    group.finish();
}

// ─── Shred → deshred roundtrip ───────────────────────────────────────────────

fn bench_shred_deshred_roundtrip(c: &mut Criterion) {
    let leader = [0x02u8; 32];
    let builder = ShredBuilder::new(leader, 1024);
    let data = vec![0xCCu8; 256 * 1024]; // 256KB
    let shreds = builder.shred_block(5, &data);

    c.bench_function("deshred_256kb", |b| {
        b.iter(|| builder.deshred(&shreds))
    });
}

// ─── Gossip deduplication ────────────────────────────────────────────────────

fn bench_received_cache_record(c: &mut Criterion) {
    let message_counts = [1_000usize, 10_000];
    let mut group = c.benchmark_group("gossip_record");
    for &n in &message_counts {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &count| {
            b.iter(|| {
                let mut cache = ReceivedCache::new(30_000);
                let from = [0xFFu8; 32];
                for i in 0..count {
                    let mut hash = [0u8; 32];
                    hash[..8].copy_from_slice(&(i as u64).to_le_bytes());
                    cache.record(hash, from, i as u64);
                }
            })
        });
    }
    group.finish();
}

fn bench_received_cache_dedup_check(c: &mut Criterion) {
    // Önceden doldurulmuş cache üzerinde is_duplicate çağrısı
    let mut cache = ReceivedCache::new(60_000);
    let from = [0x11u8; 32];
    for i in 0..10_000u64 {
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&i.to_le_bytes());
        cache.record(hash, from, i);
    }
    let known_hash = {
        let mut h = [0u8; 32];
        h[..8].copy_from_slice(&42u64.to_le_bytes());
        h
    };

    c.bench_function("gossip_is_duplicate_hit", |b| {
        b.iter(|| cache.is_duplicate(&known_hash, 99_999))
    });
}

// ─── Reed-Solomon encode ─────────────────────────────────────────────────────

fn bench_erasure_encode(c: &mut Criterion) {
    let config = ErasureConfig::default_32_32();
    let encoder = ErasureEncoder::new(config);

    let leader = [0x03u8; 32];
    let data_shreds: Vec<Shred> = (0..32u32)
        .map(|i| Shred::new(1, i, ShredType::Data, leader, vec![i as u8; 512]))
        .collect();

    c.bench_function("erasure_encode_32_32", |b| {
        b.iter(|| encoder.encode(1, 0, &data_shreds))
    });
}

// ─── Reed-Solomon decode (bazı shred'ler eksik) ───────────────────────────────

fn bench_erasure_decode(c: &mut Criterion) {
    let config = ErasureConfig::default_32_32();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    let leader = [0x04u8; 32];
    let data_shreds: Vec<Shred> = (0..32u32)
        .map(|i| Shred::new(2, i, ShredType::Data, leader, vec![i as u8; 512]))
        .collect();
    let code_shreds = encoder.encode(2, 0, &data_shreds);

    // 8 data shred eksik bırak (all code shreds mevcut = 32 → kurtarılabilir)
    let data_with_gaps: Vec<Option<Shred>> = data_shreds
        .iter()
        .enumerate()
        .map(|(i, s)| if i < 8 { None } else { Some(s.clone()) })
        .collect();

    c.bench_function("erasure_decode_32_32_gap8", |b| {
        b.iter(|| decoder.decode(data_with_gaps.clone(), &code_shreds, 2).ok())
    });
}

criterion_group!(
    benches,
    bench_shred_block,
    bench_shred_deshred_roundtrip,
    bench_received_cache_record,
    bench_received_cache_dedup_check,
    bench_erasure_encode,
    bench_erasure_decode,
);
criterion_main!(benches);
