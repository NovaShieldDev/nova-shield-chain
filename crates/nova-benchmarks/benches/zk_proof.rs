/// ZK kanıt üretme ve doğrulama benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - MintWitness::prove()   — Groth16 kanıt üretimi (Mint devresi)
/// - MintProof::verify()    — Groth16 kanıt doğrulama
/// - SpendWitness::prove()  — Harcama kanıtı üretimi (6 kısıt)
/// - SpendProof::verify()   — Harcama kanıtı doğrulama
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use nova_zk::{
    MintWitness, MintProof,
    Note, NSC_ASSET_ID, NullifierKey,
    utxo::MerkleWitness,
};
use nova_zk::circuits::spend::SpendWitness;
use nova_primitives::SlotId;

fn make_note(value: u128, seed: u8) -> Note {
    Note::new(value, NSC_ASSET_ID, [seed; 32], [seed.wrapping_add(100); 32])
}

// ─── Mint kanıtı ─────────────────────────────────────────────────────────────

fn bench_mint_prove(c: &mut Criterion) {
    let note = make_note(1_000_000, 42);
    let witness = MintWitness::new(note);
    let slot = SlotId(100);

    c.bench_function("mint_prove", |b| {
        b.iter(|| {
            witness.prove(slot).unwrap()
        })
    });
}

fn bench_mint_verify(c: &mut Criterion) {
    let note = make_note(1_000_000, 42);
    let witness = MintWitness::new(note);
    let proof = witness.prove(SlotId(100)).unwrap();

    c.bench_function("mint_verify", |b| {
        b.iter(|| {
            proof.verify().unwrap()
        })
    });
}

// ─── Farklı değerlerle Mint ───────────────────────────────────────────────────

fn bench_mint_prove_values(c: &mut Criterion) {
    let values = [1u128, 1_000, 1_000_000, 1_000_000_000_000];
    let slot = SlotId(200);

    let mut group = c.benchmark_group("mint_prove_by_value");
    for &v in &values {
        let note = make_note(v, 1);
        let witness = MintWitness::new(note);

        group.bench_with_input(BenchmarkId::from_parameter(v), &witness, |b, w| {
            b.iter(|| w.prove(slot).unwrap())
        });
    }
    group.finish();
}

// ─── Spend kanıtı ────────────────────────────────────────────────────────────

fn bench_spend_prove(c: &mut Criterion) {
    let note = make_note(500_000, 7);
    let nk = NullifierKey([7u8; 32]);

    // Minimal Merkle yolu (1 seviye)
    let commitment = note.commit();
    let sibling = [0xAAu8; 32];
    let anchor = {
        let mut h = blake3::Hasher::new();
        h.update(commitment.as_bytes());
        h.update(&sibling);
        *h.finalize().as_bytes()
    };

    let witness = SpendWitness::new(
        note,
        nk,
        vec![sibling],
        vec![true],
        [0x11u8; 32],
        [0x22u8; 32],
    );

    c.bench_function("spend_prove", |b| {
        b.iter(|| witness.prove(anchor, SlotId(300)).unwrap())
    });
}

fn bench_spend_verify(c: &mut Criterion) {
    let note = make_note(500_000, 7);
    let nk = NullifierKey([7u8; 32]);

    let commitment = note.commit();
    let sibling = [0xAAu8; 32];
    let anchor = {
        let mut h = blake3::Hasher::new();
        h.update(commitment.as_bytes());
        h.update(&sibling);
        *h.finalize().as_bytes()
    };

    let witness = SpendWitness::new(
        note,
        nk,
        vec![sibling],
        vec![true],
        [0x11u8; 32],
        [0x22u8; 32],
    );
    let proof = witness.prove(anchor, SlotId(300)).unwrap();

    c.bench_function("spend_verify", |b| {
        b.iter(|| proof.verify().unwrap())
    });
}

// ─── Ardışık prove + verify ──────────────────────────────────────────────────

fn bench_mint_roundtrip(c: &mut Criterion) {
    c.bench_function("mint_prove_then_verify", |b| {
        b.iter(|| {
            let note = make_note(42_000, 55);
            let w = MintWitness::new(note);
            let proof = w.prove(SlotId(1)).unwrap();
            proof.verify().unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_mint_prove,
    bench_mint_verify,
    bench_mint_prove_values,
    bench_spend_prove,
    bench_spend_verify,
    bench_mint_roundtrip,
);
criterion_main!(benches);
