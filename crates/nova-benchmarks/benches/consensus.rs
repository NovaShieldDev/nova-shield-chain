/// BFT consensus motoru benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - BftEngine::process_vote()     — oy işleme (100 validator, 1 blok)
/// - BftEngine::verify_block()     — blok doğrulama
/// - GrandpaEngine::prevote()      — GRANDPA ön-oy ekleme
/// - GrandpaEngine::precommit()    — GRANDPA ön-taahhüt ekleme
/// - vote_throughput               — 100 validator × 100 blok = 10.000 oy
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use nova_consensus::{
    BftEngine, Vote,
    grandpa::GrandpaEngine,
    types::{Block, BlockHeader},
};
use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};
use nova_primitives::types::Hash32;
use blake3;

const VALIDATOR_COUNT: u64 = 100;
const QUORUM: u64 = 67; // 2/3 supermajority

/// Vote struct'ı doğrudan oluştur (Vote::mock #[cfg(test)] olduğundan)
fn make_vote(validator: ValidatorId, block_hash: BlockHash, height: u64, slot: SlotId) -> Vote {
    let mut sig = [0u8; 64];
    let mut hasher = blake3::Hasher::new();
    hasher.update(block_hash.as_bytes());
    hasher.update(&validator.0.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    let hash = hasher.finalize();
    sig[..32].copy_from_slice(hash.as_bytes());
    Vote { validator, block_hash, height, signature: sig, slot }
}

fn make_validators(n: u64) -> Vec<ValidatorId> {
    (1..=n).map(ValidatorId).collect()
}

fn make_block(height: u64, parent: BlockHash) -> Block {
    let header = BlockHeader {
        height,
        slot: SlotId(height),
        epoch: EpochId(height / 216_000),
        parent_hash: parent,
        state_root: [0u8; 32],
        proposer: ValidatorId(1),
        timestamp: height * 400,
        tx_count: 0,
    };
    Block::new(header)
}

fn genesis_hash() -> BlockHash {
    Hash32::from([0u8; 32])
}

// ─── Tek oy işleme ───────────────────────────────────────────────────────────

fn bench_process_vote_single(c: &mut Criterion) {
    let validators = make_validators(VALIDATOR_COUNT);
    let block = make_block(1, genesis_hash());

    let vote = make_vote(ValidatorId(1), block.block_hash, 1, SlotId(1));
    let slot = SlotId(1);

    c.bench_function("bft_process_vote_single", |b| {
        b.iter(|| {
            let mut e = BftEngine::new(validators.clone());
            e.process_vote(vote.clone(), slot).ok()
        })
    });
}

// ─── Quorum oluşturma ────────────────────────────────────────────────────────

fn bench_bft_quorum_formation(c: &mut Criterion) {
    let validators = make_validators(VALIDATOR_COUNT);
    let block = make_block(1, genesis_hash());

    c.bench_function("bft_quorum_formation_67_of_100", |b| {
        b.iter(|| {
            let mut engine = BftEngine::new(validators.clone());
            for v in 1..=QUORUM {
                let vote = make_vote(ValidatorId(v), block.block_hash, 1, SlotId(1));
                engine.process_vote(vote, SlotId(1)).ok();
            }
        })
    });
}

// ─── Blok doğrulama ──────────────────────────────────────────────────────────

fn bench_verify_block(c: &mut Criterion) {
    let validators = make_validators(VALIDATOR_COUNT);
    let block = make_block(1, genesis_hash());
    let engine = BftEngine::new(validators);

    c.bench_function("bft_verify_block", |b| {
        b.iter(|| engine.verify_block(&block).ok())
    });
}

// ─── Oy throughput (100 validator × 100 blok) ────────────────────────────────

fn bench_vote_throughput(c: &mut Criterion) {
    let validators = make_validators(VALIDATOR_COUNT);
    let block_count = 100u64;

    let blocks: Vec<Block> = {
        let mut blocks = Vec::new();
        let mut parent = genesis_hash();
        for h in 1..=block_count {
            let b = make_block(h, parent);
            parent = b.block_hash;
            blocks.push(b);
        }
        blocks
    };

    let total_votes = QUORUM * block_count;
    let mut group = c.benchmark_group("bft_vote_throughput");
    group.throughput(Throughput::Elements(total_votes));

    group.bench_function("100_validators_100_blocks", |b| {
        b.iter(|| {
            let mut engine = BftEngine::new(validators.clone());
            for block in &blocks {
                let slot = block.header.slot;
                for v in 1..=QUORUM {
                    let vote = make_vote(
                        ValidatorId(v),
                        block.block_hash,
                        block.header.height,
                        slot,
                    );
                    engine.process_vote(vote, slot).ok();
                }
            }
        })
    });
    group.finish();
}

// ─── GRANDPA prevote + precommit ─────────────────────────────────────────────

fn bench_grandpa_prevote(c: &mut Criterion) {
    let block = make_block(10, genesis_hash());
    let hash = block.block_hash;

    c.bench_function("grandpa_prevote_67_of_100", |b| {
        b.iter(|| {
            let mut engine = GrandpaEngine::new(VALIDATOR_COUNT);
            engine.start_round(1);
            for v in 1..=QUORUM {
                engine.prevote(ValidatorId(v), hash, 10).ok();
            }
        })
    });
}

fn bench_grandpa_precommit(c: &mut Criterion) {
    let block = make_block(10, genesis_hash());
    let hash = block.block_hash;

    c.bench_function("grandpa_precommit_67_of_100", |b| {
        b.iter(|| {
            let mut engine = GrandpaEngine::new(VALIDATOR_COUNT);
            engine.start_round(1);
            for v in 1..=QUORUM {
                engine.prevote(ValidatorId(v), hash, 10).ok();
            }
            for v in 1..=QUORUM {
                engine.precommit(ValidatorId(v), hash, 10).ok();
            }
        })
    });
}

criterion_group!(
    benches,
    bench_process_vote_single,
    bench_bft_quorum_formation,
    bench_verify_block,
    bench_vote_throughput,
    bench_grandpa_prevote,
    bench_grandpa_precommit,
);
criterion_main!(benches);
