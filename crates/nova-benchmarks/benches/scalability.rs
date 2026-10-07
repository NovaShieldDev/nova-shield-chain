/// Ölçeklenebilirlik benchmark'ları — validator sayısına göre performans.
///
/// Ölçülen operasyonlar:
/// - BFT quorum formation: N=3,7,21,50,100,200 validator
/// - Quorum oluşum süresi validator sayısıyla nasıl ölçekleniyor?
/// - vote_throughput: sabit validator sayısında artan round sayısı
/// - Consensus finality latency: ilk oydan QC'ye kadar süre
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_consensus::{BftEngine, Vote};
use nova_primitives::{BlockHash, SlotId, ValidatorId};
use blake3;

fn make_block_hash(height: u64) -> BlockHash {
    let mut h = blake3::Hasher::new();
    h.update(b"scalability-bench");
    h.update(&height.to_le_bytes());
    BlockHash::from_bytes(*h.finalize().as_bytes())
}

fn make_validators(n: usize) -> Vec<ValidatorId> {
    (0..n as u64).map(ValidatorId).collect()
}

fn make_vote(validator: ValidatorId, block_hash: BlockHash, height: u64, slot: SlotId) -> Vote {
    let mut sig = [0u8; 64];
    let mut h = blake3::Hasher::new();
    h.update(block_hash.as_bytes());
    h.update(&validator.0.to_le_bytes());
    h.update(&height.to_le_bytes());
    let hash = h.finalize();
    sig[..32].copy_from_slice(hash.as_bytes());
    Vote { validator, block_hash, height, signature: sig, slot }
}

/// BFT quorum eşiği: ceil(2n/3)
fn quorum(n: usize) -> usize {
    (2 * n + 2) / 3
}

// ─── BFT quorum formation — N validator ──────────────────────────────────────

fn bench_bft_quorum_scaling(c: &mut Criterion) {
    let validator_counts = [3usize, 7, 21, 50, 100, 200];
    let mut group = c.benchmark_group("bft_quorum_scaling");

    for &n in &validator_counts {
        let validators = make_validators(n);
        let bh = make_block_hash(1);
        let q = quorum(n);

        group.throughput(Throughput::Elements(q as u64));
        group.bench_with_input(
            BenchmarkId::new("validators", n),
            &(validators, bh, q),
            |b, (validators, &bh, &q)| {
                b.iter(|| {
                    let mut engine = BftEngine::new(validators.clone());
                    for i in 0..q {
                        let vote = make_vote(ValidatorId(i as u64), bh, 1, SlotId(1));
                        engine.process_vote(vote, SlotId(1)).ok();
                    }
                });
            },
        );
    }
    group.finish();
}

// ─── BFT throughput — sabit 100 validator, artan round sayısı ────────────────

fn bench_bft_rounds_throughput(c: &mut Criterion) {
    let round_counts = [10usize, 100, 1_000];
    let n_validators = 100usize;
    let mut group = c.benchmark_group("bft_rounds_throughput");

    let validators = make_validators(n_validators);
    let q = quorum(n_validators);

    for &rounds in &round_counts {
        let total_votes = (q * rounds) as u64;
        group.throughput(Throughput::Elements(total_votes));

        group.bench_with_input(
            BenchmarkId::new("rounds", rounds),
            &(validators.clone(), q, rounds),
            |b, (validators, &q, &rounds)| {
                b.iter(|| {
                    let mut engine = BftEngine::new(validators.clone());
                    for height in 1..=rounds as u64 {
                        let bh = make_block_hash(height);
                        let slot = SlotId(height);
                        for i in 0..q {
                            let vote = make_vote(ValidatorId(i as u64), bh, height, slot);
                            engine.process_vote(vote, slot).ok();
                        }
                    }
                });
            },
        );
    }
    group.finish();
}

// ─── İlk oy'dan QC'ye latency ────────────────────────────────────────────────

fn bench_bft_time_to_qc(c: &mut Criterion) {
    let validator_counts = [3usize, 10, 50, 100];
    let mut group = c.benchmark_group("bft_time_to_qc");

    for &n in &validator_counts {
        let validators = make_validators(n);
        let bh = make_block_hash(42);
        let q = quorum(n);

        group.bench_with_input(
            BenchmarkId::new("validators", n),
            &(validators, bh, q),
            |b, (validators, &bh, &q)| {
                b.iter(|| {
                    let mut engine = BftEngine::new(validators.clone());
                    let slot = SlotId(42);
                    let mut qc_formed = false;
                    for i in 0..q {
                        let vote = make_vote(ValidatorId(i as u64), bh, 1, slot);
                        if let Ok(Some(_qc)) = engine.process_vote(vote, slot) {
                            qc_formed = true;
                            break;
                        }
                    }
                    qc_formed
                });
            },
        );
    }
    group.finish();
}

// ─── Tek oy işleme latency — N validator set ile ─────────────────────────────

fn bench_single_vote_latency(c: &mut Criterion) {
    let validator_counts = [10usize, 100, 1_000];
    let mut group = c.benchmark_group("bft_single_vote_latency");

    for &n in &validator_counts {
        let validators = make_validators(n);
        let bh = make_block_hash(1);
        let vote = make_vote(ValidatorId(1), bh, 1, SlotId(1));

        group.bench_with_input(
            BenchmarkId::new("validator_set_size", n),
            &(validators, vote),
            |b, (validators, vote)| {
                b.iter(|| {
                    let mut engine = BftEngine::new(validators.clone());
                    engine.process_vote(vote.clone(), SlotId(1)).ok()
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_bft_quorum_scaling,
    bench_bft_rounds_throughput,
    bench_bft_time_to_qc,
    bench_single_vote_latency,
);
criterion_main!(benches);
