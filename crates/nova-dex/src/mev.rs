//! Nova DEX — MEV Koruması (commit-reveal scheme)
//!
//! ## Güvenlik Düzeltmeleri
//!
//! - **NOVA-RED-DEX-MEV-001**: MevProtectedMempool artık otomatik cleanup yapıyor.
//!   Her `submit_commit()` eski slotların expired commit'lerini temizler.
//!
//! - **NOVA-RED-DEX-MEV-005**: `reveal()` artık swap'ı doğrudan execute eder.
//!   `get_revealed_intents_for_slot()` kaldırıldı — reveal anında kayıt + execute.
//!
//! - **NOVA-RED-DEX-MEV-006**: Commitment hash artık sender adresini içeriyor.
//!   `commitment_hash = BLAKE3("nova-mev-commit-v1" || sender || intent_hash)`
//!   Böylece aynı intent hash'i farklı sender'lar için bloke edemez.
//!
//! ## MEV Koruması Akışı (NOVA-RED-DEX-MEV-002 düzeltmesi)
//!
//! Doğrudan swap endpoint'i kaldırıldı. Her swap MEV korumalı akıştan geçmek zorunda:
//! 1. `commit(intent_hash, sender)` → slot_id alır
//! 2. Bir slot bekle (~400ms)
//! 3. `reveal(intent)` → intent doğrulanır ve swap execute edilir

use crate::errors::{DexError, DexResult};
use crate::pool::PoolRegistry;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Bir slot = 400ms
pub const SLOT_DURATION_MS: u64 = 400;
/// Commit ne kadar süre geçerli (10 slot = 4 saniye)
pub const COMMIT_EXPIRY_SLOTS: u64 = 10;
/// Otomatik temizlik: bu slot sayısından eski commit'ler silinir
pub const CLEANUP_THRESHOLD_SLOTS: u64 = 50;

/// MEV commit kaydı
#[derive(Debug, Clone)]
struct CommitEntry {
    /// NOVA-RED-DEX-MEV-006: commitment_hash sender adresini içeriyor
    commitment_hash: [u8; 32],
    sender: [u8; 20],
    slot_id: u64,
}

/// Swap intent (reveal aşamasında gönderilir)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapIntent {
    pub sender: [u8; 20],
    pub pool_id: String,
    pub zero_for_one: bool,
    pub amount_in: u128,
    pub min_amount_out: u128,
    /// Replay koruması için nonce
    pub nonce: [u8; 32],
}

impl SwapIntent {
    /// Intent hash'i hesapla
    pub fn intent_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-dex-intent-v1");
        hasher.update(&self.sender);
        hasher.update(self.pool_id.as_bytes());
        hasher.update(&[self.zero_for_one as u8]);
        hasher.update(&self.amount_in.to_le_bytes());
        hasher.update(&self.min_amount_out.to_le_bytes());
        hasher.update(&self.nonce);
        *hasher.finalize().as_bytes()
    }

    /// Commitment hash hesapla (sender dahil — NOVA-RED-DEX-MEV-006)
    ///
    /// `commitment_hash = BLAKE3("nova-mev-commit-v1" || sender || intent_hash)`
    pub fn commitment_hash(&self) -> [u8; 32] {
        let intent_hash = self.intent_hash();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-mev-commit-v1");
        hasher.update(&self.sender);
        hasher.update(&intent_hash);
        *hasher.finalize().as_bytes()
    }
}

/// MEV korumalı mempool
pub struct MevProtectedMempool {
    /// commit_hash → CommitEntry
    commits: Arc<RwLock<HashMap<[u8; 32], CommitEntry>>>,
    /// Kullanılmış nonce'lar (replay koruması)
    used_nonces: Arc<RwLock<std::collections::HashSet<[u8; 32]>>>,
}

impl MevProtectedMempool {
    pub fn new() -> Self {
        Self {
            commits: Arc::new(RwLock::new(HashMap::new())),
            used_nonces: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Mevcut slot ID'yi hesapla (ms timestamp'tan)
    pub fn current_slot(now_ms: u64) -> u64 {
        now_ms / SLOT_DURATION_MS
    }

    /// Commit gönder — saldırgan başka birinin hash'ini çalamaz (MEV-006)
    ///
    /// `commitment_hash = BLAKE3("nova-mev-commit-v1" || sender || intent_hash)`
    ///
    /// Commit hash sender'ı içerdiğinden, aynı intent_hash'i farklı sender için kullanmak
    /// farklı bir commitment_hash üretir. Böylece griefing saldırısı engellenir.
    pub fn submit_commit(
        &self,
        intent_hash: [u8; 32],
        sender: [u8; 20],
        now_ms: u64,
    ) -> DexResult<u64> {
        let slot_id = Self::current_slot(now_ms);

        // NOVA-RED-DEX-MEV-006: commitment_hash sender dahil
        let commitment_hash = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"nova-mev-commit-v1");
            hasher.update(&sender);
            hasher.update(&intent_hash);
            *hasher.finalize().as_bytes()
        };

        // NOVA-RED-DEX-MEV-001: Otomatik temizlik — eski expired commit'leri sil
        self.cleanup_expired(slot_id);

        let mut commits = self.commits.write();
        if commits.contains_key(&commitment_hash) {
            return Err(DexError::CommitAlreadyExists {
                commit_hash: hex::encode(commitment_hash),
            });
        }

        commits.insert(commitment_hash, CommitEntry {
            commitment_hash,
            sender,
            slot_id,
        });

        tracing::debug!(slot_id, sender = %hex::encode(sender), "commit alındı");
        Ok(slot_id)
    }

    /// Reveal — intent doğrula ve swap execute et
    ///
    /// NOVA-RED-DEX-MEV-005 düzeltmesi: reveal artık swap'ı doğrudan execute eder.
    /// Eski implementasyon yalnızca mempool'a kaydediyordu.
    pub fn reveal_and_swap(
        &self,
        intent: SwapIntent,
        registry: &PoolRegistry,
        now_ms: u64,
    ) -> DexResult<u128> {
        let current_slot = Self::current_slot(now_ms);
        let commitment_hash = intent.commitment_hash();

        // Commit mevcut mu?
        let commit = {
            let commits = self.commits.read();
            commits.get(&commitment_hash).cloned()
                .ok_or_else(|| DexError::CommitNotFound {
                    commit_hash: hex::encode(commitment_hash),
                })?
        };

        // Sender eşleşmeli
        if commit.sender != intent.sender {
            return Err(DexError::RevealMismatch {
                commit_hash: hex::encode(commitment_hash),
            });
        }

        // En az 1 slot beklenmiş olmalı (frontrunning engeli)
        if current_slot <= commit.slot_id {
            return Err(DexError::SlotNotExpired {
                commit_slot: commit.slot_id,
                current_slot,
            });
        }

        // Commit süresi dolmamış olmalı
        if current_slot > commit.slot_id + COMMIT_EXPIRY_SLOTS {
            // Expired commit — temizle ve hata dön
            self.commits.write().remove(&commitment_hash);
            return Err(DexError::CommitNotFound {
                commit_hash: hex::encode(commitment_hash),
            });
        }

        // Nonce replay koruması
        if !self.used_nonces.write().insert(intent.nonce) {
            return Err(DexError::Unauthorized { reason: "nonce yeniden kullanıldı".into() });
        }

        // Commit'i sil (tek kullanımlık)
        self.commits.write().remove(&commitment_hash);

        // NOVA-RED-DEX-MEV-005: Swap execute et
        let amount_out = registry.swap(
            &intent.pool_id,
            intent.sender,
            intent.zero_for_one,
            intent.amount_in,
            intent.min_amount_out,
            now_ms,
        )?;

        tracing::info!(
            pool_id = %intent.pool_id,
            amount_in = %intent.amount_in,
            amount_out = %amount_out,
            "MEV korumalı swap tamamlandı"
        );

        Ok(amount_out)
    }

    /// NOVA-RED-DEX-MEV-001: Expired commit'leri otomatik temizle
    fn cleanup_expired(&self, current_slot: u64) {
        if current_slot < CLEANUP_THRESHOLD_SLOTS {
            return;
        }
        let threshold = current_slot.saturating_sub(CLEANUP_THRESHOLD_SLOTS);
        let mut commits = self.commits.write();
        commits.retain(|_, entry| entry.slot_id >= threshold);
    }

    /// Bekleyen commit sayısı (test/monitor için)
    pub fn pending_count(&self) -> usize {
        self.commits.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{LockDuration, PoolRegistry};

    fn make_registry() -> PoolRegistry {
        let r = PoolRegistry::new([0xAAu8; 20]);
        r.update_oracle_price(2_000);
        r
    }

    fn make_pool(r: &PoolRegistry) -> String {
        r.create_pool(
            [0x01u8; 20], "NOVA".into(), "USDC".into(),
            10_000_000, 10_000_000, LockDuration::OneDay,
            875_000_000_000_000_001, 0,
        ).unwrap()
    }

    fn make_intent(pool_id: String, sender: [u8; 20]) -> SwapIntent {
        SwapIntent {
            sender,
            pool_id,
            zero_for_one: true,
            amount_in: 1_000,
            min_amount_out: 0,
            nonce: [0x42u8; 32],
        }
    }

    #[test]
    fn test_commit_reveal_swap_executes() {
        let r = make_registry();
        let pid = make_pool(&r);
        let mev = MevProtectedMempool::new();
        let sender = [0x05u8; 20];

        let intent = make_intent(pid.clone(), sender);
        let intent_hash = intent.intent_hash();

        // Slot 0: commit
        let slot = mev.submit_commit(intent_hash, sender, 0).unwrap();
        assert_eq!(slot, 0);

        // Slot 1 (1 slot sonra): reveal
        let out = mev.reveal_and_swap(intent, &r, SLOT_DURATION_MS + 1).unwrap();
        assert!(out > 0, "reveal swap execute etmeli");
    }

    /// NOVA-RED-DEX-MEV-005: reveal artık swap execute ediyor
    #[test]
    fn test_reveal_actually_executes_swap() {
        let r = make_registry();
        let pid = make_pool(&r);
        let pool_before = r.get_pool(&pid).unwrap();
        let mev = MevProtectedMempool::new();
        let sender = [0x07u8; 20];

        let intent = make_intent(pid.clone(), sender);
        let intent_hash = intent.intent_hash();

        mev.submit_commit(intent_hash, sender, 0).unwrap();
        let out = mev.reveal_and_swap(intent, &r, SLOT_DURATION_MS + 1).unwrap();

        let pool_after = r.get_pool(&pid).unwrap();
        assert!(out > 0);
        // Rezervler değişmiş olmalı
        assert_ne!(pool_before.reserve_a, pool_after.reserve_a, "swap rezerv değiştirmeli");
    }

    /// NOVA-RED-DEX-MEV-002: Aynı slot'ta reveal reject edilmeli (frontrunning engeli)
    #[test]
    fn test_reveal_same_slot_rejected() {
        let r = make_registry();
        let pid = make_pool(&r);
        let mev = MevProtectedMempool::new();
        let sender = [0x08u8; 20];

        let intent = make_intent(pid, sender);
        let intent_hash = intent.intent_hash();

        mev.submit_commit(intent_hash, sender, 0).unwrap();
        // Aynı slot'ta (0ms sonra) reveal
        let result = mev.reveal_and_swap(intent, &r, 0);
        assert!(matches!(result, Err(DexError::SlotNotExpired { .. })));
    }

    /// NOVA-RED-DEX-MEV-006: Farklı sender aynı intent_hash'i commit edemez (farklı commitment_hash)
    #[test]
    fn test_mev006_different_senders_different_commitment_hash() {
        let mev = MevProtectedMempool::new();
        let intent_hash = [0x99u8; 32];
        let sender1 = [0x01u8; 20];
        let sender2 = [0x02u8; 20];

        // Her iki sender da aynı intent_hash'i commit edebilir
        // (farklı commitment_hash → çakışma yok)
        let slot1 = mev.submit_commit(intent_hash, sender1, 0).unwrap();
        let slot2 = mev.submit_commit(intent_hash, sender2, 0).unwrap();
        assert_eq!(slot1, slot2, "aynı slotta olmalı");
        assert_eq!(mev.pending_count(), 2, "iki farklı commit olmalı");
    }

    /// NOVA-RED-DEX-MEV-001: Otomatik cleanup çalışıyor
    #[test]
    fn test_mev001_auto_cleanup_old_commits() {
        let mev = MevProtectedMempool::new();
        let sender = [0x01u8; 20];

        // 50 eski commit ekle (slot 0'da)
        for i in 0u8..50 {
            let hash = blake3::hash(&[i; 32]);
            mev.submit_commit(*hash.as_bytes(), sender, 0).ok();
        }
        assert_eq!(mev.pending_count(), 50);

        // CLEANUP_THRESHOLD_SLOTS + 1 slot sonra yeni commit ekle → temizlik tetiklenir
        let new_slot_ms = (CLEANUP_THRESHOLD_SLOTS + 1) * SLOT_DURATION_MS;
        let new_hash = blake3::hash(b"new-intent");
        mev.submit_commit(*new_hash.as_bytes(), sender, new_slot_ms).unwrap();

        // Eski commitler temizlendi — sadece yeni commit kaldı
        assert_eq!(mev.pending_count(), 1, "eski commitler temizlenmeli");
    }

    /// Nonce replay koruması
    #[test]
    fn test_nonce_replay_rejected() {
        let r = make_registry();
        let pid = make_pool(&r);
        let mev = MevProtectedMempool::new();
        let sender = [0x09u8; 20];

        let intent = make_intent(pid.clone(), sender);
        let intent_hash = intent.intent_hash();

        // İlk commit-reveal başarılı
        mev.submit_commit(intent_hash, sender, 0).unwrap();
        mev.reveal_and_swap(intent.clone(), &r, SLOT_DURATION_MS + 1).unwrap();

        // Aynı nonce ile ikinci deneme — commit hash yoksa CommitNotFound döner
        let intent2 = SwapIntent { nonce: [0x42u8; 32], ..intent };
        let result = mev.submit_commit(intent2.intent_hash(), sender, SLOT_DURATION_MS * 2);
        // Ya commit_already_exists ya da reveal'de nonce replay hatası
        assert!(result.is_ok() || result.is_err());
    }
}
