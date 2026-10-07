//! Nullifier — çifte harcama önleme (Penumbra/Zcash pattern'inden ilham)
//!
//! Nullifier = BLAKE3("nova-nullifier-v1" || nullifier_key || commitment)
//! Nullifier zincire yazılır; aynı note iki kez harcandığında tespit edilir.

use crate::errors::{ZkError, ZkResult};
use crate::utxo::note::NoteCommitment;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── NullifierKey ─────────────────────────────────────────────────────────────

/// Nullifier key — spending key'den türetilir; nullifier hesaplamak için kullanılır
/// Penumbra: nk ∈ F_q, Nova: [u8; 32] (BLAKE3 domain)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct NullifierKey(pub [u8; 32]);

impl NullifierKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Nullifier türet: BLAKE3("nova-nullifier-v1" || nk || commitment)
    pub fn derive_nullifier(&self, commitment: &NoteCommitment) -> Nullifier {
        // ZK-friendly MiMC nullifier — NullifierDerivationCircuit ile tutarlı
        // nf = mimc_sponge([domain_nf, nk, commitment])
        let nf = crate::circuits::mimc::mimc_nullifier(&self.0, commitment.as_bytes());
        Nullifier(nf)
    }
}

// ─── Nullifier ────────────────────────────────────────────────────────────────

/// Nullifier — harcanan note'un "damgası"
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Nullifier(pub [u8; 32]);

impl Nullifier {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

// ─── NullifierSet ─────────────────────────────────────────────────────────────

/// NullifierSet — harcanmış nullifier deposu (thread-safe, DashMap)
#[derive(Debug, Default, Clone)]
pub struct NullifierSet {
    /// Nullifier → harcandığı slot
    spent: Arc<DashMap<Nullifier, SlotId>>,
}

impl NullifierSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nullifier ekle (daha önce harcandıysa hata döner)
    ///
    /// # Atomik Güvenlik
    /// DashMap `entry()` API'si kullanılır: tek shard lock ile check+insert atomik yapılır.
    /// `contains_key()` + `insert()` çifti TOCTOU race condition'a açıktır — bu implementasyon
    /// o açığı kapatır.
    pub fn insert(&self, nullifier: Nullifier, slot: SlotId) -> ZkResult<()> {
        match self.spent.entry(nullifier) {
            Entry::Occupied(_) => Err(ZkError::NullifierAlreadySpent),
            Entry::Vacant(e) => {
                e.insert(slot);
                Ok(())
            }
        }
    }

    /// Nullifier harcanmış mı?
    pub fn contains(&self, nullifier: &Nullifier) -> bool {
        self.spent.contains_key(nullifier)
    }

    /// Toplam harcanmış nullifier sayısı
    pub fn len(&self) -> usize {
        self.spent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spent.is_empty()
    }

    /// Hangi slot'ta harcandığını döner
    pub fn spent_at(&self, nullifier: &Nullifier) -> Option<SlotId> {
        self.spent.get(nullifier).map(|v| *v)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_commitment(seed: u8) -> NoteCommitment {
        NoteCommitment([seed; 32])
    }

    fn make_nk(seed: u8) -> NullifierKey {
        NullifierKey([seed; 32])
    }

    #[test]
    fn test_nullifier_deterministic() {
        let nk = make_nk(1);
        let cm = make_commitment(2);
        let n1 = nk.derive_nullifier(&cm);
        let n2 = nk.derive_nullifier(&cm);
        assert_eq!(n1, n2);
    }

    #[test]
    fn test_nullifier_unique_per_key() {
        let nk1 = make_nk(1);
        let nk2 = make_nk(2);
        let cm = make_commitment(5);
        assert_ne!(nk1.derive_nullifier(&cm), nk2.derive_nullifier(&cm));
    }

    #[test]
    fn test_nullifier_unique_per_commitment() {
        let nk = make_nk(1);
        let cm1 = make_commitment(1);
        let cm2 = make_commitment(2);
        assert_ne!(nk.derive_nullifier(&cm1), nk.derive_nullifier(&cm2));
    }

    #[test]
    fn test_nullifier_set_insert_and_contains() {
        let set = NullifierSet::new();
        let nk = make_nk(1);
        let n = nk.derive_nullifier(&make_commitment(1));
        assert!(!set.contains(&n));
        set.insert(n, SlotId(100)).unwrap();
        assert!(set.contains(&n));
    }

    #[test]
    fn test_nullifier_set_double_spend() {
        let set = NullifierSet::new();
        let nk = make_nk(1);
        let n = nk.derive_nullifier(&make_commitment(1));
        set.insert(n, SlotId(100)).unwrap();
        let err = set.insert(n, SlotId(200));
        assert!(matches!(err, Err(ZkError::NullifierAlreadySpent)));
    }

    #[test]
    fn test_nullifier_set_spent_at() {
        let set = NullifierSet::new();
        let nk = make_nk(1);
        let n = nk.derive_nullifier(&make_commitment(1));
        set.insert(n, SlotId(42)).unwrap();
        assert_eq!(set.spent_at(&n), Some(SlotId(42)));
    }

    #[test]
    fn test_nullifier_hex() {
        let nk = make_nk(3);
        let hex = nk.derive_nullifier(&make_commitment(7)).to_hex();
        assert_eq!(hex.len(), 64);
    }

    /// TOCTOU race condition testi — eş zamanlı çift harcama girişimi
    ///
    /// Atomik entry() olmadan bu test bazen başarısız olurdu.
    #[test]
    fn test_nullifier_set_concurrent_double_spend() {
        use std::sync::Arc;
        use std::thread;

        let set = Arc::new(NullifierSet::new());
        let nk = make_nk(42);
        let cm = make_commitment(99);
        let nullifier = nk.derive_nullifier(&cm);

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let set = Arc::clone(&set);
                thread::spawn(move || set.insert(nullifier, SlotId(i)))
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let ok_count = results.iter().filter(|r| r.is_ok()).count();
        let err_count = results.iter().filter(|r| r.is_err()).count();

        // Sadece BİR thread başarılı olmalı, gerisi NullifierAlreadySpent
        assert_eq!(ok_count, 1, "Sadece bir thread nullifier'ı yazabilmeli");
        assert_eq!(err_count, 7, "Kalan tüm thread'ler hata almalı");
    }
}
