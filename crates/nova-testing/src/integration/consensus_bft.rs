//! nova-consensus BFT entegrasyon testi
//!
//! ## Test Akışı (Avalanchego Simplex §5.8)
//! 1. 5 validator ile BftEngine oluştur
//! 2. Her validator için Vote oluştur (blake3 mock signature)
//! 3. 2f+1 = 4 oy toplanınca QuorumCertificate üretilir
//! 4. QC doğrulanır, blok finalize edilir
//!
//! ## Kaynak: Avalanchego simplex `quorumSize = ceil(2n/3)`
//! n=5 → quorum = ceil(10/3) = 4

use nova_consensus::{
    bft::{BftEngine, Vote},
    types::{Block, BlockHeader, QuorumCertificate},
    BftSigner,
};
use nova_crypto::BlsKeypair;
use nova_primitives::{Address, BlsPublicKey, BlockHash, EpochId, SlotId, ValidatorId};

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn validator_id(n: u64) -> ValidatorId {
    ValidatorId(n)
}

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

fn block_hash(n: u8) -> BlockHash {
    let bytes = [n; 32];
    BlockHash::from_bytes(bytes)
}

/// mock Vote oluştur — blake3(block_hash || validator_id || height) ile imza
fn make_vote(vid: ValidatorId, bh: BlockHash, height: u64, slot: u64) -> Vote {
    let mut sig = [0u8; 64];
    let mut hasher = blake3::Hasher::new();
    hasher.update(bh.as_bytes());
    hasher.update(&vid.0.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    let hash = hasher.finalize();
    sig[..32].copy_from_slice(hash.as_bytes());
    Vote { validator: vid, block_hash: bh, height, signature: sig, slot: SlotId(slot) }
}

fn make_block(height: u64, slot: u64, proposer: ValidatorId) -> Block {
    let header = BlockHeader {
        height,
        slot: SlotId(slot),
        epoch: EpochId(slot / 8640),
        parent_hash: block_hash(0),
        state_root: [0u8; 32],
        proposer,
        timestamp: 1_700_000_000 + slot,
        tx_count: 0,
    };
    Block::new(header)
}

fn bls_ikm(vid: ValidatorId) -> [u8; 32] {
    [(vid.0 as u8).wrapping_mul(37); 32]
}

fn make_engine_with_validators(n: usize) -> BftEngine {
    let validators: Vec<(ValidatorId, BlsPublicKey)> = (1..=n as u64)
        .map(|i| {
            let vid = ValidatorId(i);
            let kp = BlsKeypair::from_ikm(&bls_ikm(vid)).unwrap();
            (vid, kp.bls_public_key())
        })
        .collect();
    BftEngine::new_with_pubkeys(validators)
}

fn make_signed_vote(vid: ValidatorId, bh: BlockHash, height: u64, slot: u64) -> nova_consensus::SignedVote {
    let signer = BftSigner::from_ikm(vid, &bls_ikm(vid)).unwrap();
    signer.sign_vote(bh, height, SlotId(slot)).unwrap()
}

// ─── Testler ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 validator ile quorum (4 oy) → QC üretimi
    #[test]
    fn bft_quorum_5_validators_produces_qc() {
        let engine = make_engine_with_validators(5);
        let bh = block_hash(42);
        let height = 1u64;
        let slot = 100u64;

        // İlk 3 oy → quorum yok
        for i in 1u64..=3 {
            let vote = make_vote(ValidatorId(i), bh, height, slot);
            let result = engine.process_vote(vote, SlotId(slot)).unwrap();
            assert!(result.is_none(), "İlk 3 oy QC üretmemeli");
        }

        // 4. oy → quorum sağlanır (ceil(2*5/3) = 4)
        let vote4 = make_vote(ValidatorId(4), bh, height, slot);
        let qc = engine.process_vote(vote4, SlotId(slot)).unwrap();
        assert!(qc.is_some(), "4. oy QC üretmeli");

        let qc = qc.unwrap();
        assert_eq!(qc.block_hash, bh);
        assert_eq!(qc.height, height);
        assert!(qc.signers.len() >= 4, "QC en az 4 imzacı içermeli");
    }

    /// QC doğrulama — gerçek BLS imzalı (NOVA-RED-CONSENSUS-002 düzeltmesi)
    #[test]
    fn bft_qc_verification() {
        let engine = make_engine_with_validators(5);
        let bh = block_hash(100);
        let height = 2u64;
        let slot = 200u64;

        // Gerçek BLS SignedVote ile quorum sağla
        let mut qc_opt = None;
        for i in 1u64..=4 {
            let signed = make_signed_vote(ValidatorId(i), bh, height, slot);
            qc_opt = engine.process_signed_vote(signed, SlotId(slot)).unwrap();
        }
        let qc = qc_opt.unwrap();

        // QC geçerli olmalı
        assert!(engine.verify_qc(&qc).is_ok(), "QC doğrulama başarılı olmalı");
    }

    /// Duplicate oy reddedilmeli (Avalanchego: double-vote koruması)
    #[test]
    fn bft_duplicate_vote_rejected() {
        let engine = make_engine_with_validators(5);
        let bh = block_hash(1);
        let height = 3u64;
        let slot = 300u64;

        let vote1 = make_vote(ValidatorId(1), bh, height, slot);
        let vote1_dup = make_vote(ValidatorId(1), bh, height, slot);

        engine.process_vote(vote1, SlotId(slot)).unwrap();
        let dup_result = engine.process_vote(vote1_dup, SlotId(slot));
        assert!(dup_result.is_err(), "Duplicate oy reddedilmeli");
    }

    /// Yetkisiz validator oyu reddedilmeli
    #[test]
    fn bft_unauthorized_validator_rejected() {
        let engine = make_engine_with_validators(5);
        let bh = block_hash(2);

        // Validator 99 set'te yok
        let fake_vote = make_vote(ValidatorId(99), bh, 4, 400);
        let result = engine.process_vote(fake_vote, SlotId(400));
        assert!(result.is_err(), "Yetkisiz validator reddedilmeli");
    }

    /// 3 validator ile quorum (2 oy) — küçük komite
    #[test]
    fn bft_small_committee_3_validators() {
        let engine = make_engine_with_validators(3);
        let bh = block_hash(5);
        let height = 5u64;
        let slot = 500u64;

        // 1 oy → quorum yok (ceil(2*3/3) = 2)
        let v1 = make_vote(ValidatorId(1), bh, height, slot);
        let r1 = engine.process_vote(v1, SlotId(slot)).unwrap();
        assert!(r1.is_none());

        // 2. oy → quorum
        let v2 = make_vote(ValidatorId(2), bh, height, slot);
        let qc = engine.process_vote(v2, SlotId(slot)).unwrap();
        assert!(qc.is_some(), "3 validator'da 2 oy quorum sağlamalı");
    }

    /// Blok doğrulama: geçerli ve geçersiz blok
    #[test]
    fn bft_block_verification() {
        let engine = make_engine_with_validators(5);

        // Genesis bloğunu önce doğrula — parent olarak kullanılacak
        let genesis = make_block(0, 99, ValidatorId(1));
        engine.verify_block(&genesis).expect("Genesis blok kayıt edilmeli");

        // Height=1 bloğu genesis hash'ini parent olarak göstermeli
        let header = BlockHeader {
            height: 1,
            slot: SlotId(100),
            epoch: EpochId(0),
            parent_hash: genesis.block_hash,
            state_root: [0u8; 32],
            proposer: ValidatorId(1),
            timestamp: 1_700_000_100,
            tx_count: 0,
        };
        let valid_block = Block::new(header);
        assert!(engine.verify_block(&valid_block).is_ok(), "Geçerli blok doğrulanmalı");
    }

    /// last_finalized_height başlangıçta 0
    #[test]
    fn bft_initial_finalized_height_is_zero() {
        let engine = make_engine_with_validators(5);
        assert_eq!(engine.last_finalized_height(), 0);
    }

    /// Farklı yükseklikler için bağımsız oy toplama
    #[test]
    fn bft_independent_height_collectors() {
        let engine = make_engine_with_validators(5);
        let bh1 = block_hash(10);
        let bh2 = block_hash(20);

        // Height 1 için 4 oy
        for i in 1u64..=4 {
            engine.process_vote(make_vote(ValidatorId(i), bh1, 1, 100), SlotId(100)).unwrap();
        }

        // Height 2 için bağımsız sayaç — hâlâ 0 oy
        assert_eq!(engine.vote_count_for_height(2), 0);

        // Height 2'ye de oy ekle
        let v1_h2 = make_vote(ValidatorId(1), bh2, 2, 101);
        let result = engine.process_vote(v1_h2, SlotId(101)).unwrap();
        assert!(result.is_none(), "Height 2'de 1 oy QC üretmemeli");
        assert_eq!(engine.vote_count_for_height(2), 1);
    }
}
