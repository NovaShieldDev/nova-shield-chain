//! Equivocation (Eşdeğer Oy) Tespit ve Raporlama — Polkadot GRANDPA pattern
//!
//! ## Tasarım
//!
//! Polkadot `grandpa/src/equivocation.rs` `process_evidence()` akışından ilham:
//! ```text
//! process_evidence():
//!   1. KeyOwnerProofSystem → validator membership proof
//!   2. check_equivocation_proof() → 2 farklı vote + valid sigs
//!   3. Set ID session boundary check
//!   4. OffenceReportSystem → slash
//! ```
//!
//! Nova uyarlaması:
//! 1. Validator registry membership check (key ownership = BLS sig verification)
//! 2. İki farklı blok hash'ini işaret eden iki oy → eşdeğer oy kanıtı
//! 3. Her iki oyun BLS imzalarını validator'ın registered pubkey'i ile doğrula
//! 4. Yaş kontrolü (MAX_EVIDENCE_AGE_SLOTS)
//! 5. Duplicate evidence kontrolü (aynı kanıt iki kez işlenemez)
//! 6. `SlashingEngine::handle_double_sign()` → otomatik slash
//!
//! ## Raporlama Modeli
//!
//! - **Açık raporlama**: Herhangi bir düğüm kanıt gönderebilir
//! - **Kaynak sınırlaması**: Aynı kanıt hash'i iki kez işlenemez
//! - **Ödül mekanizması**: Geçerli rapor gönderen reporter ödül kazanır (gelecek impl)
//! - **Spam koruması**: Reporter başına 64 slot içinde max 10 rapor
//!
//! ## Polkadot referansı
//! `polkadot-sdk/substrate/frame/grandpa/src/equivocation.rs`
//! `polkadot-sdk/substrate/primitives/consensus/grandpa/src/lib.rs`

use crate::errors::{ConsensusError, ConsensusResult};
use crate::signing::{vote_message, BftVerifier};
use crate::slashing::SlashingEngine;
use crate::types::{DoubleSignEvidence, MAX_EVIDENCE_AGE_SLOTS};
use crate::validator::ValidatorRegistry;
use nova_crypto::BlsVerifier;
use nova_primitives::{Address, BlsPublicKey, BlsSignature, BlockHash, SlotId, ValidatorId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{info, warn};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Bir reporter'ın 64 slot içinde gönderebileceği maksimum rapor sayısı (spam koruması)
pub const MAX_REPORTS_PER_REPORTER_PER_WINDOW: u64 = 10;

/// Reporter spam kontrol penceresi (slot)
pub const REPORTER_RATE_WINDOW_SLOTS: u64 = 64;

// ─── EquivocationVote ─────────────────────────────────────────────────────────

/// Bir equivocation kanıtındaki tek oy
///
/// Validator hem oy verdiği blok hash'ini hem de BLS imzasını içerir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquivocationVote {
    /// Oy verilen blok hash'i
    pub block_hash: BlockHash,
    /// Oy verilen yükseklik
    pub height: u64,
    /// Oy verildiği slot
    pub slot: SlotId,
    /// Validator'ın BLS public key'i (key ownership proof)
    pub validator_pubkey: BlsPublicKey,
    /// BLS imzası (96 byte)
    pub bls_signature: BlsSignature,
}

impl EquivocationVote {
    /// Bu oyun BLS imzasını doğrula
    ///
    /// `vote_message(block_hash, validator_id, height, slot)` üzerinden doğrulama.
    /// Bu, key ownership proof'udur: validator pubkey'ini bilen biri imzaladı.
    pub fn verify_signature(&self, validator_id: ValidatorId) -> bool {
        let msg = vote_message(self.block_hash, validator_id, self.height, self.slot);
        BlsVerifier::verify(&msg, &self.bls_signature, &self.validator_pubkey).is_ok()
    }
}

// ─── EquivocationReport ───────────────────────────────────────────────────────

/// Equivocation raporu — iki çelişen oy + reporter bilgisi
///
/// Polkadot: `EquivocationProof { set_id, equivocation: Equivocation { round_number, identity, first, second } }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquivocationReport {
    /// Suç işleyen validator
    pub offender: ValidatorId,

    /// Birinci oy (farklı blok hash'i)
    pub vote_a: EquivocationVote,

    /// İkinci oy (farklı blok hash'i, aynı yükseklik)
    pub vote_b: EquivocationVote,

    /// Raporu gönderen düğüm (ödül için)
    pub reporter: Address,

    /// Raporun gönderildiği slot
    pub submitted_at_slot: SlotId,
}

impl EquivocationReport {
    /// Temel tutarlılık kontrolü:
    /// 1. Her iki oy aynı yükseklikte
    /// 2. Blok hash'leri farklı (gerçek equivocation)
    /// 3. Her iki oyun pubkey'leri aynı (aynı validator)
    pub fn is_structurally_valid(&self) -> bool {
        // Yükseklik eşleşmeli
        if self.vote_a.height != self.vote_b.height {
            return false;
        }
        // Blok hash'leri farklı olmalı (equivocation = farklı iki oy)
        if self.vote_a.block_hash == self.vote_b.block_hash {
            return false;
        }
        // Aynı validator pubkey olmalı
        if self.vote_a.validator_pubkey != self.vote_b.validator_pubkey {
            return false;
        }
        true
    }

    /// Kanıtın tekil parmak izi (duplicate engeli için)
    ///
    /// offender + height kombinasyonu — aynı yükseklikte aynı validator için bir kez işlenir.
    pub fn evidence_key(&self) -> [u8; 32] {
        let mut key = [0u8; 32];
        let vid = self.offender.0.to_le_bytes();
        key[..8].copy_from_slice(&vid);
        let h = self.vote_a.height.to_le_bytes();
        key[8..16].copy_from_slice(&h);
        key
    }
}

// ─── ReporterStats ────────────────────────────────────────────────────────────

/// Reporter başına istatistik (spam koruması için)
#[derive(Debug, Clone, Default)]
struct ReporterStats {
    /// Son pencerede gönderilen rapor sayısı
    report_count: u64,
    /// Pencere başlangıç slotu
    window_start_slot: u64,
}

impl ReporterStats {
    /// Rapor gönderilebilir mi? Rate limit kontrolü.
    fn can_report(&self, current_slot: u64) -> bool {
        // Pencere geçtiyse sayacı sıfırla (sanal reset)
        if current_slot >= self.window_start_slot + REPORTER_RATE_WINDOW_SLOTS {
            return true;
        }
        self.report_count < MAX_REPORTS_PER_REPORTER_PER_WINDOW
    }

    /// Rapor kaydını güncelle
    fn record_report(&mut self, current_slot: u64) {
        if current_slot >= self.window_start_slot + REPORTER_RATE_WINDOW_SLOTS {
            self.window_start_slot = current_slot;
            self.report_count = 1;
        } else {
            self.report_count += 1;
        }
    }
}

// ─── EquivocationReporter ─────────────────────────────────────────────────────

/// Equivocation raporlama servisi
///
/// Polkadot GRANDPA equivocation pattern:
/// - Herhangi bir düğüm rapor gönderebilir
/// - Kanıt doğrulanır (yapısal + kriptografik)
/// - Validator registry'den key ownership kontrolü
/// - SlashingEngine üzerinden otomatik slash
/// - Duplicate ve spam koruması
pub struct EquivocationReporter {
    /// Validator kayıt defteri (membership + pubkey kontrolü)
    registry: Arc<ValidatorRegistry>,

    /// Ceza motoru (slash dispatch)
    slashing: Arc<SlashingEngine>,

    /// İşlenmiş kanıt hash'leri (duplicate engeli)
    processed: Arc<RwLock<HashSet<[u8; 32]>>>,

    /// Reporter başına istatistik (spam koruması)
    reporter_stats: Arc<RwLock<HashMap<Address, ReporterStats>>>,

    /// Toplam işlenen rapor sayısı
    total_processed: Arc<std::sync::atomic::AtomicU64>,

    /// Toplam geçerli rapor sayısı
    total_valid: Arc<std::sync::atomic::AtomicU64>,
}

impl EquivocationReporter {
    /// Yeni EquivocationReporter oluştur
    pub fn new(registry: Arc<ValidatorRegistry>, slashing: Arc<SlashingEngine>) -> Self {
        Self {
            registry,
            slashing,
            processed: Arc::new(RwLock::new(HashSet::new())),
            reporter_stats: Arc::new(RwLock::new(HashMap::new())),
            total_processed: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            total_valid: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Equivocation raporu işle
    ///
    /// Akış:
    /// 1. Spam koruması — reporter rate limit
    /// 2. Yapısal doğrulama — yükseklik, hash, pubkey tutarlılığı
    /// 3. Duplicate kontrolü — aynı kanıt daha önce işlendi mi
    /// 4. Yaş kontrolü — MAX_EVIDENCE_AGE_SLOTS
    /// 5. Validator membership — registry'de var mı + pubkey eşleşiyor mu
    /// 6. Kriptografik doğrulama — her iki oyun BLS imzası
    /// 7. Slash dispatch — SlashingEngine::handle_double_sign
    ///
    /// Döndürür: İşlenen olaylar listesi
    pub fn process_report(
        &self,
        report: EquivocationReport,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<crate::types::ConsensusEvent>> {
        self.total_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // ── 1. Spam koruması ──────────────────────────────────────────────
        {
            let mut stats = self.reporter_stats.write();
            let entry = stats.entry(report.reporter).or_default();
            if !entry.can_report(current_slot.0) {
                return Err(ConsensusError::Internal(format!(
                    "reporter {:?} rate limit aşıldı (pencerede max {} rapor)",
                    report.reporter, MAX_REPORTS_PER_REPORTER_PER_WINDOW
                )));
            }
            entry.record_report(current_slot.0);
        }

        // ── 2. Yapısal doğrulama ──────────────────────────────────────────
        if !report.is_structurally_valid() {
            return Err(ConsensusError::InvalidDoubleSignEvidence);
        }

        // ── 3. Duplicate kontrolü ─────────────────────────────────────────
        let evidence_key = report.evidence_key();
        if self.processed.read().contains(&evidence_key) {
            return Err(ConsensusError::InvalidDoubleSignEvidence);
        }

        // ── 4. Yaş kontrolü ───────────────────────────────────────────────
        let report_slot = report.submitted_at_slot.0;
        let age = current_slot.0.saturating_sub(report_slot);
        if age > MAX_EVIDENCE_AGE_SLOTS {
            return Err(ConsensusError::EvidenceTooOld {
                age_slots: age,
                max_slots: MAX_EVIDENCE_AGE_SLOTS,
            });
        }

        // ── 5. Validator membership ve pubkey kontrolü ────────────────────
        // Polkadot: `KeyOwnerProofSystem::check_proof(key_owner_proof)`
        // Nova: registry.get() + pubkey karşılaştırma
        let validator_info = self.registry.get(&report.offender)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: report.offender.0.to_string(),
            })?;

        // Validator'ın zaten tombstone'lanmış olup olmadığını kontrol et
        if validator_info.status.is_tombstoned() {
            // Zaten tombstone → evidence işlemeden geç ama duplicate işaretle
            self.processed.write().insert(evidence_key);
            return Err(ConsensusError::ValidatorTombstoned);
        }

        // Registered pubkey rapordaki pubkey ile eşleşmeli (key ownership proof)
        let registered_pubkey = validator_info.bls_pubkey
            .as_ref()
            .ok_or_else(|| ConsensusError::Internal(
                format!("validator {} için BLS pubkey kayıtlı değil", report.offender.0)
            ))?;

        if registered_pubkey.as_bytes() != report.vote_a.validator_pubkey.as_bytes() {
            return Err(ConsensusError::Internal(
                "rapordaki pubkey registry ile uyuşmuyor".to_string()
            ));
        }

        // ── 6. Kriptografik doğrulama — BLS imzaları ─────────────────────
        // Polkadot: `check_equivocation_proof()` → iki farklı signed vote
        // Her iki oyun BLS imzası doğrulanır → key ownership kanıtlanır
        if !report.vote_a.verify_signature(report.offender) {
            return Err(ConsensusError::Internal(
                "birinci oyun BLS imzası geçersiz".to_string()
            ));
        }

        if !report.vote_b.verify_signature(report.offender) {
            return Err(ConsensusError::Internal(
                "ikinci oyun BLS imzası geçersiz".to_string()
            ));
        }

        // ── 7. Slash dispatch ─────────────────────────────────────────────
        // SlashingEngine'e DoubleSignEvidence oluştur ve işlet
        let ds_evidence = DoubleSignEvidence {
            validator: report.offender,
            height: report.vote_a.height,
            block_hash_a: report.vote_a.block_hash,
            block_hash_b: report.vote_b.block_hash,
            submitted_at_slot: report.submitted_at_slot,
        };

        let events = self.slashing.handle_double_sign(ds_evidence, current_slot)?;

        // ── Kayıt ─────────────────────────────────────────────────────────
        self.processed.write().insert(evidence_key);
        self.total_valid.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        info!(
            target: "nova_equivocation",
            offender = ?report.offender,
            height = report.vote_a.height,
            reporter = ?report.reporter,
            "equivocation kanıtı işlendi → slash uygulandı"
        );

        Ok(events)
    }

    // ─── Sorgular ─────────────────────────────────────────────────────────────

    /// Belirli bir kanıt daha önce işlendi mi?
    pub fn is_processed(&self, key: &[u8; 32]) -> bool {
        self.processed.read().contains(key)
    }

    /// Toplam işlenen rapor sayısı
    pub fn total_processed(&self) -> u64 {
        self.total_processed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Toplam geçerli rapor sayısı
    pub fn total_valid(&self) -> u64 {
        self.total_valid.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Belirli bir reporter'ın mevcut penceredeki rapor sayısı
    pub fn reporter_count(&self, reporter: &Address, current_slot: u64) -> u64 {
        let stats = self.reporter_stats.read();
        match stats.get(reporter) {
            Some(s) if current_slot < s.window_start_slot + REPORTER_RATE_WINDOW_SLOTS => {
                s.report_count
            }
            _ => 0,
        }
    }
}

// ─── Yardımcı: EquivocationReport builder ─────────────────────────────────────

/// Test ve production'da rapor oluşturmak için builder
pub struct EquivocationReportBuilder {
    offender: ValidatorId,
    reporter: Address,
    submitted_at_slot: SlotId,
}

impl EquivocationReportBuilder {
    pub fn new(offender: ValidatorId, reporter: Address, submitted_at_slot: SlotId) -> Self {
        Self { offender, reporter, submitted_at_slot }
    }

    /// BLS signed oyları ile rapor oluştur
    pub fn build(
        self,
        vote_a: EquivocationVote,
        vote_b: EquivocationVote,
    ) -> EquivocationReport {
        EquivocationReport {
            offender: self.offender,
            vote_a,
            vote_b,
            reporter: self.reporter,
            submitted_at_slot: self.submitted_at_slot,
        }
    }
}

// ─── Testler ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MIN_VALIDATOR_STAKE, SLASH_DOUBLE_SIGN_BPS};
    use crate::validator::ValidatorRegistry;
    use nova_primitives::{Address, Balance, BlockHash};

    fn make_vid(n: u8) -> ValidatorId {
        ValidatorId(n as u64)
    }

    fn make_addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    /// Gerçek BLS keypair + validator registry oluştur
    fn make_bls_validator(
        registry: &Arc<ValidatorRegistry>,
        seed: u8,
    ) -> (ValidatorId, nova_crypto::BlsKeypair) {
        let vid = make_vid(seed);
        // from_ikm: deterministic key derivation from input keying material
        let mut ikm = [0u8; 32];
        ikm[0] = seed;
        let keypair = nova_crypto::BlsKeypair::from_ikm(&ikm).unwrap();
        let pubkey = keypair.bls_public_key();

        // signing_key: 32-byte placeholder (BLS pubkey ayrıca set_bls_pubkey ile kaydedilir)
        registry.register(
            vid,
            make_addr(seed),
            Balance(MIN_VALIDATOR_STAKE * 5),
            500,
            ikm,
            SlotId(0),
        ).unwrap();

        // BLS pubkey'i registry'ye kaydet
        registry.set_bls_pubkey(vid, pubkey);

        (vid, keypair)
    }

    /// Geçerli BLS imzalı EquivocationVote oluştur
    fn make_signed_vote(
        keypair: &nova_crypto::BlsKeypair,
        validator_id: ValidatorId,
        block_hash: BlockHash,
        height: u64,
        slot: SlotId,
    ) -> EquivocationVote {
        let msg = vote_message(block_hash, validator_id, height, slot);
        let signature = keypair.sign_raw(&msg);
        let pubkey = keypair.bls_public_key();

        EquivocationVote {
            block_hash,
            height,
            slot,
            validator_pubkey: pubkey,
            bls_signature: signature,
        }
    }

    fn make_reporter_and_engine(seed: u8) -> (
        Arc<ValidatorRegistry>,
        Arc<SlashingEngine>,
        EquivocationReporter,
        ValidatorId,
        nova_crypto::BlsKeypair,
    ) {
        let registry = Arc::new(ValidatorRegistry::new());
        let (vid, keypair) = make_bls_validator(&registry, seed);
        let slashing = Arc::new(SlashingEngine::new(Arc::clone(&registry)));
        let reporter = EquivocationReporter::new(Arc::clone(&registry), Arc::clone(&slashing));
        (registry, slashing, reporter, vid, keypair)
    }

    // ─── Yapısal doğrulama testleri ───────────────────────────────────────────

    #[test]
    fn test_structurally_valid_report() {
        let (_, _, reporter, vid, keypair) = make_reporter_and_engine(1);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let height = 100u64;
        let slot = SlotId(500);

        let vote_a = make_signed_vote(&keypair, vid, hash_a, height, slot);
        let vote_b = make_signed_vote(&keypair, vid, hash_b, height, slot);

        let report = EquivocationReportBuilder::new(vid, make_addr(99), SlotId(500))
            .build(vote_a, vote_b);

        assert!(report.is_structurally_valid());
    }

    #[test]
    fn test_same_hash_not_equivocation() {
        let (_, _, _, vid, keypair) = make_reporter_and_engine(2);
        let hash = BlockHash::from_bytes([0xAAu8; 32]);
        let slot = SlotId(100);

        let vote_a = make_signed_vote(&keypair, vid, hash, 50, slot);
        let vote_b = make_signed_vote(&keypair, vid, hash, 50, slot); // AYNI hash

        let report = EquivocationReportBuilder::new(vid, make_addr(99), SlotId(100))
            .build(vote_a, vote_b);

        assert!(!report.is_structurally_valid(), "aynı hash equivocation değil");
    }

    #[test]
    fn test_different_heights_not_equivocation() {
        let (_, _, _, vid, keypair) = make_reporter_and_engine(3);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);

        let vote_a = make_signed_vote(&keypair, vid, hash_a, 50, SlotId(100));
        let vote_b = make_signed_vote(&keypair, vid, hash_b, 51, SlotId(101)); // FARKLI height

        let report = EquivocationReportBuilder::new(vid, make_addr(99), SlotId(110))
            .build(vote_a, vote_b);

        assert!(!report.is_structurally_valid(), "farklı yükseklik equivocation değil");
    }

    // ─── Tam işleme testi ─────────────────────────────────────────────────────

    #[test]
    fn test_valid_report_triggers_slash() {
        let (registry, _, reporter_svc, vid, keypair) = make_reporter_and_engine(10);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let height = 200u64;
        let slot = SlotId(1_000);

        let vote_a = make_signed_vote(&keypair, vid, hash_a, height, slot);
        let vote_b = make_signed_vote(&keypair, vid, hash_b, height, slot);

        let report = EquivocationReportBuilder::new(vid, make_addr(88), slot)
            .build(vote_a, vote_b);

        let events = reporter_svc.process_report(report, SlotId(1_001)).unwrap();

        // Slash + tombstone eventi bekleniyor
        let has_slash = events.iter().any(|e| {
            matches!(e.event_type, crate::types::ConsensusEventType::ValidatorSlashed { .. })
        });
        assert!(has_slash, "geçerli rapor slash tetiklemeli");

        // Validator tombstone olmalı
        let info = registry.get(&vid).unwrap();
        assert!(info.status.is_tombstoned(), "validator tombstone edilmeli");
        assert_eq!(info.stake.0, 0, "tüm stake slash edilmeli");

        // İstatistik
        assert_eq!(reporter_svc.total_valid(), 1);
        assert_eq!(reporter_svc.total_processed(), 1);
    }

    // ─── Duplicate kanıt testi ────────────────────────────────────────────────

    #[test]
    fn test_duplicate_evidence_rejected() {
        let (_, _, reporter_svc, vid, keypair) = make_reporter_and_engine(11);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let height = 300u64;
        let slot = SlotId(2_000);

        let vote_a = make_signed_vote(&keypair, vid, hash_a, height, slot);
        let vote_b = make_signed_vote(&keypair, vid, hash_b, height, slot);

        let report1 = EquivocationReportBuilder::new(vid, make_addr(77), slot)
            .build(vote_a.clone(), vote_b.clone());
        let report2 = EquivocationReportBuilder::new(vid, make_addr(77), slot)
            .build(vote_a, vote_b);

        reporter_svc.process_report(report1, SlotId(2_001)).unwrap();

        // İkinci aynı rapor → reddedilmeli
        let err = reporter_svc.process_report(report2, SlotId(2_002)).unwrap_err();
        assert!(
            matches!(err, ConsensusError::InvalidDoubleSignEvidence | ConsensusError::ValidatorTombstoned),
            "duplicate kanıt reddedilmeli: {err:?}"
        );
    }

    // ─── Yaş kontrolü testi ───────────────────────────────────────────────────

    #[test]
    fn test_old_evidence_rejected() {
        let (_, _, reporter_svc, vid, keypair) = make_reporter_and_engine(12);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let height = 400u64;

        // Çok eski kanıt: submitted_at_slot = 0
        let vote_a = make_signed_vote(&keypair, vid, hash_a, height, SlotId(0));
        let vote_b = make_signed_vote(&keypair, vid, hash_b, height, SlotId(0));

        let report = EquivocationReportBuilder::new(vid, make_addr(66), SlotId(0))
            .build(vote_a, vote_b);

        // current_slot = MAX_EVIDENCE_AGE_SLOTS + 1 → çok eski
        let current = SlotId(MAX_EVIDENCE_AGE_SLOTS + 1);
        let err = reporter_svc.process_report(report, current).unwrap_err();
        assert!(
            matches!(err, ConsensusError::EvidenceTooOld { .. }),
            "eski kanıt reddedilmeli: {err:?}"
        );
    }

    // ─── Bilinmeyen validator testi ───────────────────────────────────────────

    #[test]
    fn test_unknown_validator_rejected() {
        let (_, _, reporter_svc, _, keypair) = make_reporter_and_engine(13);
        let unknown_vid = ValidatorId(9999); // registry'de yok
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let slot = SlotId(500);

        let vote_a = make_signed_vote(&keypair, unknown_vid, hash_a, 100, slot);
        let vote_b = make_signed_vote(&keypair, unknown_vid, hash_b, 100, slot);

        let report = EquivocationReportBuilder::new(unknown_vid, make_addr(55), slot)
            .build(vote_a, vote_b);

        let err = reporter_svc.process_report(report, SlotId(501)).unwrap_err();
        assert!(
            matches!(err, ConsensusError::ValidatorNotFound { .. }),
            "bilinmeyen validator reddedilmeli: {err:?}"
        );
    }

    // ─── Spam koruması testi ──────────────────────────────────────────────────

    #[test]
    fn test_reporter_rate_limit() {
        let reporter_addr = make_addr(200);
        let mut stats = ReporterStats::default();

        // MAX_REPORTS_PER_REPORTER_PER_WINDOW rapor gönder
        for _ in 0..MAX_REPORTS_PER_REPORTER_PER_WINDOW {
            assert!(stats.can_report(0));
            stats.record_report(0);
        }

        // Sonraki → rate limit aşıldı
        assert!(!stats.can_report(0), "rate limit aşılmalı");

        // Pencere ilerleyince reset
        assert!(
            stats.can_report(REPORTER_RATE_WINDOW_SLOTS),
            "yeni pencerede rate limit sıfırlanmalı"
        );

        let _ = reporter_addr;
    }

    #[test]
    fn test_rate_limit_resets_in_new_window() {
        let mut stats = ReporterStats::default();

        // Mevcut pencereyi doldur
        for _ in 0..MAX_REPORTS_PER_REPORTER_PER_WINDOW {
            stats.record_report(0);
        }
        assert!(!stats.can_report(0));

        // Yeni pencere
        stats.record_report(REPORTER_RATE_WINDOW_SLOTS + 1);
        // Yeni pencerede 1 rapor var, devam edebilir
        assert!(stats.can_report(REPORTER_RATE_WINDOW_SLOTS + 1));
    }

    // ─── Evidence key tekliği ─────────────────────────────────────────────────

    #[test]
    fn test_evidence_key_unique_per_validator_and_height() {
        let (_, _, _, vid1, keypair1) = make_reporter_and_engine(20);
        let (_, _, _, vid2, keypair2) = make_reporter_and_engine(21);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let slot = SlotId(1000);

        let vote_a1 = make_signed_vote(&keypair1, vid1, hash_a, 100, slot);
        let vote_b1 = make_signed_vote(&keypair1, vid1, hash_b, 100, slot);
        let report1 = EquivocationReportBuilder::new(vid1, make_addr(1), slot)
            .build(vote_a1, vote_b1);

        let vote_a2 = make_signed_vote(&keypair2, vid2, hash_a, 100, slot);
        let vote_b2 = make_signed_vote(&keypair2, vid2, hash_b, 100, slot);
        let report2 = EquivocationReportBuilder::new(vid2, make_addr(2), slot)
            .build(vote_a2, vote_b2);

        // Farklı validator → farklı key
        assert_ne!(report1.evidence_key(), report2.evidence_key());
    }

    #[test]
    fn test_evidence_key_same_for_same_validator_height() {
        let (_, _, _, vid, keypair) = make_reporter_and_engine(22);
        let hash_a = BlockHash::from_bytes([0xAAu8; 32]);
        let hash_b = BlockHash::from_bytes([0xBBu8; 32]);
        let slot = SlotId(1000);

        let vote_a1 = make_signed_vote(&keypair, vid, hash_a, 200, slot);
        let vote_b1 = make_signed_vote(&keypair, vid, hash_b, 200, slot);
        let report1 = EquivocationReportBuilder::new(vid, make_addr(3), slot)
            .build(vote_a1.clone(), vote_b1.clone());

        // Aynı validator + height → aynı key (duplicate tespiti için)
        let vote_a2 = make_signed_vote(&keypair, vid, hash_a, 200, slot);
        let vote_b2 = make_signed_vote(&keypair, vid, hash_b, 200, slot);
        let report2 = EquivocationReportBuilder::new(vid, make_addr(4), slot)
            .build(vote_a2, vote_b2);

        assert_eq!(report1.evidence_key(), report2.evidence_key());
    }

    // ─── BLS imza doğrulama testi ─────────────────────────────────────────────

    #[test]
    fn test_bls_signature_verification() {
        let (_, _, _, vid, keypair) = make_reporter_and_engine(30);
        let hash = BlockHash::from_bytes([0xCCu8; 32]);
        let slot = SlotId(100);

        let vote = make_signed_vote(&keypair, vid, hash, 50, slot);
        assert!(vote.verify_signature(vid), "geçerli BLS imza doğrulanmalı");
    }

    #[test]
    fn test_wrong_validator_id_fails_verification() {
        let (_, _, _, vid, keypair) = make_reporter_and_engine(31);
        let hash = BlockHash::from_bytes([0xDDu8; 32]);
        let slot = SlotId(100);

        let vote = make_signed_vote(&keypair, vid, hash, 50, slot);
        // Farklı bir validator ID ile doğrula → mesaj farklı → imza geçersiz
        let wrong_vid = ValidatorId(vid.0 + 1);
        assert!(
            !vote.verify_signature(wrong_vid),
            "yanlış validator ID ile doğrulama başarısız olmalı"
        );
    }
}
