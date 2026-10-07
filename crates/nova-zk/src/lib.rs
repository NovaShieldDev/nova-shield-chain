//! nova-zk — ZK-Shield gizlilik katmanı (§5.7)
//!
//! Modüller:
//! - `errors`      — ZkError
//! - `utxo`        — Note, Nullifier, CommitmentTree, ShieldedPool
//! - `keys`        — SpendingKey, FullViewingKey, DiversifiedAddress
//! - `circuits`    — Mint, Transfer, Burn devreleri
//! - `proof`       — Groth16, PLONK, TrustedSetupParams
//! - `modes`       — Standard, FullShield, SemiShield
//! - `integration` — ZkShieldEngine, ShieldedTx

pub mod circuits;
pub mod errors;
pub mod integration;
pub mod keys;
pub mod modes;
pub mod proof;
pub mod utxo;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use circuits::{BurnProof, BurnWitness, MintProof, MintWitness, TransferProof, TransferWitness};
pub use errors::{ZkError, ZkResult};
pub use integration::{ShieldedAction, ShieldedTx, ZkShieldEngine};
pub use keys::{DiversifiedAddress, FullViewingKey, SpendingKey};
pub use modes::{FullShieldMode, SemiShieldMode, ShieldMode, StandardMode};
pub use proof::{CircuitType, Groth16Proof, PlonkProof, TrustedSetupParams};
pub use utxo::{
    AssetId, CommitmentTree, MerkleWitness, Note, NoteCommitment, NSC_ASSET_ID, Nullifier,
    NullifierKey, NullifierSet, PoolStats, ShieldedPool, SpendOutput, ValueCommitment,
};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::SlotId;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn make_note(value: u128, rng: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [rng; 32], [rng + 100; 32])
    }

    // ─── Tam ZK-Shield yaşam döngüsü ─────────────────────────────────────────

    #[test]
    fn test_full_zk_shield_lifecycle() {
        let pool = ShieldedPool::new();
        let nk = NullifierKey([42u8; 32]);

        // 1. Mint: transparent → shielded
        let note = make_note(1_000, 1);
        let commitment = pool.mint(note.clone(), SlotId(0)).unwrap();
        assert!(pool.has_commitment(&commitment));

        // 2. Transfer kanıtı üret
        let output_note = make_note(900, 2);
        let witness = TransferWitness::new(note.clone(), nk, output_note.clone()).unwrap();
        let proof = witness.prove(SlotId(10)).unwrap();
        assert!(proof.verify().unwrap());

        // 3. Harca ve yeni not mint et
        let spend_out = pool.spend(&commitment, &nk, SlotId(10)).unwrap();
        assert_eq!(spend_out.note.value, 1_000);
        let new_cm = pool.mint(output_note, SlotId(11)).unwrap();
        assert!(pool.has_commitment(&new_cm));
    }

    // ─── Spending key → FVK → ödeme adresi ────────────────────────────────────

    #[test]
    fn test_key_hierarchy() {
        let mut rng = StdRng::seed_from_u64(12345);
        let sk = SpendingKey::generate(&mut rng);
        let fvk = sk.full_viewing_key();

        // Her index farklı adres üretmeli
        let addr0 = fvk.payment_address(0);
        let addr1 = fvk.payment_address(1);
        assert_ne!(addr0.transmission_key(), addr1.transmission_key());

        // Aynı index deterministik
        let addr0_again = fvk.payment_address(0);
        assert_eq!(addr0.transmission_key(), addr0_again.transmission_key());

        // NullifierKey tutarlı
        assert_eq!(fvk.nullifier_key, sk.nullifier_key());
    }

    // ─── Mint → Transfer → Burn tam döngü ────────────────────────────────────

    #[test]
    fn test_mint_transfer_burn_cycle() {
        let pool = ShieldedPool::new();
        let nk = NullifierKey([11u8; 32]);

        // Mint
        let note = make_note(500, 5);
        let cm = pool.mint(note.clone(), SlotId(0)).unwrap();

        // Transfer kanıtı
        let output = make_note(400, 6);
        let transfer_witness = TransferWitness::new(note, nk, output.clone()).unwrap();
        let t_proof = transfer_witness.prove(SlotId(1)).unwrap();
        assert!(t_proof.verify().unwrap());

        // Harca
        pool.spend(&cm, &nk, SlotId(1)).unwrap();
        let new_cm = pool.mint(output, SlotId(2)).unwrap();

        // Burn
        let recipient = [0x99u8; 20];
        let burn_note = make_note(400, 6); // output_note ile aynı
        let burn_witness = BurnWitness::new(burn_note, nk, recipient);
        let b_proof = burn_witness.prove(SlotId(3)).unwrap();
        assert!(b_proof.verify().unwrap());
        assert!(pool.has_commitment(&new_cm));
    }

    // ─── Çifte harcama koruması ────────────────────────────────────────────────

    #[test]
    fn test_double_spend_protection() {
        let pool = ShieldedPool::new();
        let nk = NullifierKey([7u8; 32]);
        let note = make_note(100, 3);
        let cm = pool.mint(note, SlotId(0)).unwrap();

        pool.spend(&cm, &nk, SlotId(1)).unwrap();
        let err = pool.spend(&cm, &nk, SlotId(2));
        assert!(matches!(err, Err(ZkError::NullifierAlreadySpent)));
    }

    // ─── ZK kanıt tutarlılığı ─────────────────────────────────────────────────

    #[test]
    fn test_zk_proof_consistency() {
        let note = make_note(2_000, 7);
        let w = MintWitness::new(note.clone());
        let proof = w.prove(SlotId(100)).unwrap();

        assert_eq!(proof.statement.commitment, note.commit());
        assert!(proof.verify().unwrap());
        assert!(proof.is_consistent());
    }

    // ─── ShieldedPool istatistikleri ──────────────────────────────────────────

    #[test]
    fn test_pool_statistics() {
        let pool = ShieldedPool::new();
        let nk = NullifierKey([1u8; 32]);

        for i in 0u8..5 {
            pool.mint(make_note(100, i), SlotId(0)).unwrap();
        }
        let cm = pool.mint(make_note(50, 99), SlotId(0)).unwrap();
        pool.spend(&cm, &nk, SlotId(1)).unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total_commitments, 6);
        assert_eq!(stats.total_nullifiers, 1);
    }

    // ─── Gizlilik modu karşılaştırması ────────────────────────────────────────

    #[test]
    fn test_shield_modes_gas_ordering() {
        let standard = StandardMode::new().gas_cost();
        let semi = SemiShieldMode::new().gas_cost();
        let full = FullShieldMode::new().gas_cost();
        assert!(standard < semi);
        assert!(semi < full);
    }

    // ─── PLONK universal SRS ──────────────────────────────────────────────────

    #[test]
    fn test_plonk_universal_srs() {
        use crate::proof::UniversalSrs;
        let srs = UniversalSrs::mock(1 << 20);
        let mint_params = srs.specialize(CircuitType::Mint);
        let transfer_params = srs.specialize(CircuitType::Transfer);

        assert_ne!(mint_params.setup_hash, transfer_params.setup_hash);
        let mint_proof = Groth16Proof::mock_prove(&mint_params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        // Yanlış parametrelerle doğrulama başarısız olmalı (Err veya Ok(false))
        assert!(matches!(mint_proof.verify(&transfer_params), Err(ZkError::InvalidProof) | Ok(false)));
    }

    // ─── ZkShieldEngine uçtan uca ─────────────────────────────────────────────

    #[test]
    fn test_engine_end_to_end() {
        let engine = ZkShieldEngine::new();
        let note = make_note(800, 10);
        let witness = MintWitness::new(note);
        let proof = witness.prove(SlotId(0)).unwrap();
        let tx = ShieldedTx::new(
            ShieldedAction::Mint(proof),
            ShieldMode::FullShield,
            SlotId(0),
            500,
        );
        assert!(engine.apply(&tx).is_ok());
    }
}
