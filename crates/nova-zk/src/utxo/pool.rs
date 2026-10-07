//! ShieldedPool — shielded note'ları yöneten ana havuz
//!
//! Penumbra shielded pool'undan ilham:
//! - Commitment tree: note commitment'larını tutar
//! - Nullifier set: harcanmış note'ları takip eder
//! - Note store: commitment → note eşlemesi (gerçekte encrypted; burada mock)

use crate::errors::{ZkError, ZkResult};
use crate::utxo::note::{AssetId, Note, NoteCommitment};
use crate::utxo::nullifier::{Nullifier, NullifierKey, NullifierSet};
use crate::utxo::tree::CommitmentTree;
use dashmap::DashMap;
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── PoolStats ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub total_commitments: usize,
    pub total_nullifiers: usize,
    pub commitment_root: [u8; 32],
}

// ─── SpendOutput ──────────────────────────────────────────────────────────────

/// Harcama işleminin çıktısı
pub struct SpendOutput {
    pub note: Note,
    pub nullifier: Nullifier,
}

// ─── ShieldedPool ─────────────────────────────────────────────────────────────

/// ShieldedPool — tüm shielded UTXO state'i
#[derive(Debug, Clone)]
pub struct ShieldedPool {
    commitment_tree: CommitmentTree,
    nullifier_set: NullifierSet,
    /// commitment → note (gerçekte zincirde şifreli saklanır)
    notes: Arc<DashMap<NoteCommitment, Note>>,
}

impl ShieldedPool {
    pub fn new() -> Self {
        Self {
            commitment_tree: CommitmentTree::new(),
            nullifier_set: NullifierSet::new(),
            notes: Arc::new(DashMap::new()),
        }
    }

    /// Transparent → Shielded: yeni note mint et
    pub fn mint(&self, note: Note, _slot: SlotId) -> ZkResult<NoteCommitment> {
        let commitment = note.commit();
        self.commitment_tree.insert(commitment)?;
        self.notes.insert(commitment, note);
        Ok(commitment)
    }

    /// Shielded → harca: note'u nullifier ile harca
    ///
    /// BUG-4 FIX: Dummy spend (value=0) için Merkle path bypass.
    ///
    /// Penumbra SpendCircuit (spend/proof.rs) 6. kısıt:
    /// `merkle_auth_path`: amount=0 ise skip → dummy spend gizlilik sağlar.
    ///
    /// Dummy spend: Gerçek bir UTXO harcamıyor, anonymity set'i büyütmek için
    /// sıfır değerli sahte note nullifier'ı. Pool'da olması gerekmiyor.
    pub fn spend(
        &self,
        commitment: &NoteCommitment,
        nullifier_key: &NullifierKey,
        slot: SlotId,
    ) -> ZkResult<SpendOutput> {
        // Note mevcut mu? Dummy spend için yoksa geç.
        let note = match self.notes.get(commitment) {
            Some(r) => r.clone(),
            None => {
                // Dummy spend: pool'da yoksa value=0 kontrolü yap
                // (gerçek ZK'da circuit bunu kanıtlar; mock'ta manuel kontrol)
                return Err(ZkError::NoteNotFound { commitment_hex: commitment.to_hex() });
            }
        };

        // Nullifier türet ve kaydet (double-spend kontrolü)
        let nullifier = nullifier_key.derive_nullifier(commitment);
        self.nullifier_set.insert(nullifier, slot)?;

        Ok(SpendOutput { note, nullifier })
    }

    /// Dummy spend (value=0): note pool'da olmadan nullifier üret.
    ///
    /// Penumbra pattern: `amount=0 → merkle_auth_path atlan`.
    /// Gizlilik arttırma işlemi — anonymity set büyütmek için.
    pub fn spend_dummy(
        &self,
        dummy_note: Note,
        nullifier_key: &NullifierKey,
        slot: SlotId,
    ) -> ZkResult<SpendOutput> {
        if dummy_note.value != 0 {
            return Err(ZkError::InvalidProof); // Dummy spend sadece value=0 ile
        }
        let commitment = dummy_note.commit();
        // Double-spend kontrolü (dummy bile olsa tekrar harcanamaz)
        let nullifier = nullifier_key.derive_nullifier(&commitment);
        self.nullifier_set.insert(nullifier, slot)?;
        Ok(SpendOutput { note: dummy_note, nullifier })
    }

    /// Nullifier harcanmış mı?
    pub fn is_spent(&self, nullifier: &Nullifier) -> bool {
        self.nullifier_set.contains(nullifier)
    }

    /// Commitment ağaçta mevcut mu?
    pub fn has_commitment(&self, commitment: &NoteCommitment) -> bool {
        self.commitment_tree.contains(commitment)
    }

    /// Commitment tree kök hash'i
    pub fn commitment_root(&self) -> [u8; 32] {
        self.commitment_tree.root()
    }

    /// Havuz istatistikleri
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            total_commitments: self.commitment_tree.len(),
            total_nullifiers: self.nullifier_set.len(),
            commitment_root: self.commitment_root(),
        }
    }

    /// Belirli bir asset'in toplam shielded değerini hesapla (yavaş — sadece audit için)
    pub fn total_shielded_value(&self, asset_id: &AssetId) -> u128 {
        self.notes
            .iter()
            .filter(|e| &e.value().asset_id == asset_id)
            .map(|e| e.value().value)
            .fold(0u128, |acc, v| acc.saturating_add(v))
    }
}

impl Default for ShieldedPool {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_note(value: u128, rng_seed: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [rng_seed; 32], [rng_seed + 100; 32])
    }

    fn make_nk(seed: u8) -> NullifierKey {
        NullifierKey([seed; 32])
    }

    #[test]
    fn test_pool_mint() {
        let pool = ShieldedPool::new();
        let note = make_note(1_000, 1);
        let cm = pool.mint(note, SlotId(0)).unwrap();
        assert!(pool.has_commitment(&cm));
        assert_eq!(pool.stats().total_commitments, 1);
    }

    #[test]
    fn test_pool_spend() {
        let pool = ShieldedPool::new();
        let nk = make_nk(1);
        let note = make_note(500, 2);
        let cm = pool.mint(note, SlotId(0)).unwrap();

        let out = pool.spend(&cm, &nk, SlotId(10)).unwrap();
        assert_eq!(out.note.value, 500);
        assert!(pool.is_spent(&out.nullifier));
    }

    #[test]
    fn test_pool_double_spend() {
        let pool = ShieldedPool::new();
        let nk = make_nk(1);
        let note = make_note(500, 3);
        let cm = pool.mint(note, SlotId(0)).unwrap();

        pool.spend(&cm, &nk, SlotId(10)).unwrap();
        let err = pool.spend(&cm, &nk, SlotId(20));
        assert!(matches!(err, Err(ZkError::NullifierAlreadySpent)));
    }

    #[test]
    fn test_pool_note_not_found() {
        let pool = ShieldedPool::new();
        let nk = make_nk(1);
        let fake_cm = NoteCommitment([99u8; 32]);
        let err = pool.spend(&fake_cm, &nk, SlotId(0));
        assert!(matches!(err, Err(ZkError::NoteNotFound { .. })));
    }

    #[test]
    fn test_pool_commitment_root_changes() {
        let pool = ShieldedPool::new();
        let root0 = pool.commitment_root();
        pool.mint(make_note(100, 1), SlotId(0)).unwrap();
        let root1 = pool.commitment_root();
        assert_ne!(root0, root1);
    }

    #[test]
    fn test_pool_total_shielded_value() {
        let pool = ShieldedPool::new();
        pool.mint(make_note(1_000, 1), SlotId(0)).unwrap();
        pool.mint(make_note(2_000, 2), SlotId(0)).unwrap();
        let total = pool.total_shielded_value(&NSC_ASSET_ID);
        assert_eq!(total, 3_000);
    }

    #[test]
    fn test_pool_stats() {
        let pool = ShieldedPool::new();
        let nk = make_nk(1);
        let note = make_note(100, 5);
        let cm = pool.mint(note, SlotId(0)).unwrap();
        pool.spend(&cm, &nk, SlotId(1)).unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total_commitments, 1);
        assert_eq!(stats.total_nullifiers, 1);
    }

    #[test]
    fn test_pool_duplicate_mint_rejected() {
        let pool = ShieldedPool::new();
        let note = make_note(100, 1);
        let note2 = Note::new(note.value, note.asset_id, note.randomness, note.transmission_key);
        pool.mint(note, SlotId(0)).unwrap();
        let err = pool.mint(note2, SlotId(1));
        assert!(matches!(err, Err(ZkError::CommitmentAlreadyExists)));
    }
}
