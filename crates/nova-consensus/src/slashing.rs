//! Slashing motoru — ceza + jail + tombstone (§5.10)
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing `HandleValidatorSignature`:
//!   `SigningInfo.MissedBlocksCounter`, `SigningInfo.IndexOffset`,
//!   sliding window bitmap → downtime slash.
//!   Bizde: `ValidatorRegistry.record_block_signature` + `SlashingEngine::check_downtime`.
//! - Cosmos SDK x/evidence `handleEquivocationEvidence`:
//!   `DoubleSignEvidence → tombstone → jail(MaxInt) → slash(100%)`.
//!   Bizde: `SlashingEngine::handle_double_sign`.
//! - Polkadot pallet-offences `on_offence`:
//!   `OffenceDetails { offender, reporters }` → `slash_fraction`, `disable_validator`.
//!   Bizde: `SlashingEngine::apply_slash` → `SlashRecord`.
//! - Polkadot pallet-staking `slash_validator`:
//!   `exposure → slash_amount = stake * slash_fraction`.
//!   Bizde: `ValidatorRegistry.slash(validator_id, slash_bps, ...)`.

use crate::errors::{ConsensusError, ConsensusResult};
use crate::types::{
    ConsensusEvent, ConsensusEventType, DoubleSignEvidence, SlashReason, SlashRecord,
    MAX_EVIDENCE_AGE_SLOTS, SLASH_DOUBLE_SIGN_BPS,
};
use crate::validator::ValidatorRegistry;
use nova_primitives::{SlotId, ValidatorId};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{info, warn};

// ─── SlashingEngine ───────────────────────────────────────────────────────────

/// Ceza motoru.
///
/// Pattern: Cosmos SDK x/slashing + Polkadot pallet-offences.
/// Çift imza, downtime, mempool sızıntısı, yanlış batch için cezalar.
pub struct SlashingEngine {
    /// Validator kayıt defteri (mutable erişim için Arc).
    registry: Arc<ValidatorRegistry>,
    /// İşlenmiş slash kayıtları.
    slash_history: Arc<RwLock<Vec<SlashRecord>>>,
    /// Tombstone'lanmış validator'lar (çift imza — kalıcı).
    tombstoned: Arc<RwLock<HashSet<ValidatorId>>>,
    /// İşlenmiş double-sign kanıtı hash'leri (tekrar işleme engeli).
    processed_evidence: Arc<RwLock<HashSet<[u8; 32]>>>,
}

impl SlashingEngine {
    /// Yeni SlashingEngine.
    pub fn new(registry: Arc<ValidatorRegistry>) -> Self {
        Self {
            registry,
            slash_history: Arc::new(RwLock::new(Vec::new())),
            tombstoned: Arc::new(RwLock::new(HashSet::new())),
            processed_evidence: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    // ─── Double-sign ─────────────────────────────────────────────────────────

    /// Çift imza kanıtını işle.
    ///
    /// Pattern: Cosmos SDK `handleEquivocationEvidence`:
    /// 1. Kanıt geçerliliği + yaş kontrolü
    /// 2. Tombstone (kalıcı devre dışı)
    /// 3. %100 slash
    /// 4. Jail (MaxInt slot)
    pub fn handle_double_sign(
        &self,
        evidence: DoubleSignEvidence,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<ConsensusEvent>> {
        // Geçerlilik
        if !evidence.is_valid() {
            return Err(ConsensusError::InvalidDoubleSignEvidence);
        }

        // Yaş kontrolü
        if evidence.is_too_old(current_slot.0) {
            return Err(ConsensusError::EvidenceTooOld {
                age_slots: current_slot.0.saturating_sub(evidence.submitted_at_slot.0),
                max_slots: MAX_EVIDENCE_AGE_SLOTS,
            });
        }

        // Daha önce işlendi mi?
        let evidence_key = self.evidence_key(&evidence);
        if self.processed_evidence.read().contains(&evidence_key) {
            return Err(ConsensusError::InvalidDoubleSignEvidence);
        }

        let validator = evidence.validator;

        // Tombstone
        self.registry.tombstone(&validator)?;
        self.tombstoned.write().insert(validator);

        // %100 slash
        let slashed_amount = self
            .registry
            .slash(&validator, SLASH_DOUBLE_SIGN_BPS, SlashReason::DoubleSign, current_slot)?;

        // Jail (MaxInt slot — kalıcı)
        let _ = self.registry.jail(&validator, SlotId(u64::MAX));

        // Kanıtı işlenmiş olarak işaretle
        self.processed_evidence.write().insert(evidence_key);

        // Kayıt
        let record = SlashRecord {
            validator,
            reason: SlashReason::DoubleSign,
            slash_bps: SLASH_DOUBLE_SIGN_BPS,
            slashed_amount,
            evidence_slot: evidence.submitted_at_slot,
            applied_at_slot: current_slot,
        };
        self.slash_history.write().push(record);

        let mut events = Vec::new();
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorSlashed {
                validator,
                reason: SlashReason::DoubleSign,
                slash_bps: SLASH_DOUBLE_SIGN_BPS,
            },
        });
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorTombstoned {
                validator,
                reason: SlashReason::DoubleSign,
            },
        });

        info!(
            target: "nova_consensus",
            validator = ?validator,
            slashed_amount,
            "çift imza: tombstone + slash %100"
        );

        Ok(events)
    }

    // ─── Downtime ─────────────────────────────────────────────────────────────

    /// Downtime kontrolü ve cezası.
    ///
    /// Pattern: Cosmos SDK `HandleValidatorSignature`:
    /// `missed_blocks_counter > (window * (10000-min_signed_bps)) / 10000` → slash + jail.
    /// Bizde: `ValidatorRegistry.record_block_signature` sliding window mantığına göre
    /// downtime tespit eder; bu fonksiyon çağrıldığında slashing uygular.
    pub fn slash_downtime(
        &self,
        validator: ValidatorId,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<ConsensusEvent>> {
        use crate::types::SLASH_DOWNTIME_BPS;

        let slashed_amount = self
            .registry
            .slash(&validator, SLASH_DOWNTIME_BPS, SlashReason::ExtendedOffline, current_slot)?;

        // Jail süresi: 2 epoch (432_000 slot)
        let jail_until = SlotId(current_slot.0 + 432_000);
        self.registry.jail(&validator, jail_until)?;

        let record = SlashRecord {
            validator,
            reason: SlashReason::ExtendedOffline,
            slash_bps: SLASH_DOWNTIME_BPS,
            slashed_amount,
            evidence_slot: current_slot,
            applied_at_slot: current_slot,
        };
        self.slash_history.write().push(record);

        let mut events = Vec::new();
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorSlashed {
                validator,
                reason: SlashReason::ExtendedOffline,
                slash_bps: SLASH_DOWNTIME_BPS,
            },
        });
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorJailed {
                validator,
                jailed_until_slot: jail_until.0,
            },
        });

        warn!(
            target: "nova_consensus",
            validator = ?validator,
            "downtime: slash + jail"
        );

        Ok(events)
    }

    // ─── Mempool sızıntısı ────────────────────────────────────────────────────

    /// Mempool sızıntısı cezası — MEV koruyucu.
    ///
    /// Whitepaper §5.7: Validator işlem sırasını sızdırırsa %50 slash + jail.
    pub fn slash_mempool_leak(
        &self,
        validator: ValidatorId,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<ConsensusEvent>> {
        use crate::types::SLASH_MEMPOOL_LEAK_BPS;

        // NOVA-RED-CONSENSUS-001: MempoolLeak requires_tombstone() == true →
        // DoubleSign akışıyla tutarlı: önce tombstone, sonra slash.
        // Tombstone kalıcıdır — jail çağrısı kaldırıldı (tombstoned validator jail'lenemez).
        self.registry.tombstone(&validator)?;
        self.tombstoned.write().insert(validator);

        let slashed_amount = self
            .registry
            .slash(&validator, SLASH_MEMPOOL_LEAK_BPS, SlashReason::MempoolLeak, current_slot)?;

        let record = SlashRecord {
            validator,
            reason: SlashReason::MempoolLeak,
            slash_bps: SLASH_MEMPOOL_LEAK_BPS,
            slashed_amount,
            evidence_slot: current_slot,
            applied_at_slot: current_slot,
        };
        self.slash_history.write().push(record);

        let mut events = Vec::new();
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorTombstoned {
                validator,
                reason: SlashReason::MempoolLeak,
            },
        });
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorSlashed {
                validator,
                reason: SlashReason::MempoolLeak,
                slash_bps: SLASH_MEMPOOL_LEAK_BPS,
            },
        });

        Ok(events)
    }

    // ─── Yanlış batch ─────────────────────────────────────────────────────────

    /// Yanlış batch temizleme cezası.
    ///
    /// Whitepaper §5.7: Validator yanlış şifreli batch'i finalize ettirirse %25 slash.
    pub fn slash_wrong_batch(
        &self,
        validator: ValidatorId,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<ConsensusEvent>> {
        use crate::types::SLASH_WRONG_BATCH_BPS;

        let slashed_amount = self.registry.slash(
            &validator,
            SLASH_WRONG_BATCH_BPS,
            SlashReason::WrongBatchClearing,
            current_slot,
        )?;

        // Jail süresi: 1 epoch
        let jail_until = SlotId(current_slot.0 + 216_000);
        self.registry.jail(&validator, jail_until)?;

        let record = SlashRecord {
            validator,
            reason: SlashReason::WrongBatchClearing,
            slash_bps: SLASH_WRONG_BATCH_BPS,
            slashed_amount,
            evidence_slot: current_slot,
            applied_at_slot: current_slot,
        };
        self.slash_history.write().push(record);

        let mut events = Vec::new();
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorSlashed {
                validator,
                reason: SlashReason::WrongBatchClearing,
                slash_bps: SLASH_WRONG_BATCH_BPS,
            },
        });
        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::ValidatorJailed {
                validator,
                jailed_until_slot: jail_until.0,
            },
        });

        Ok(events)
    }

    // ─── Sorgular ─────────────────────────────────────────────────────────────

    /// Tombstone kontrolü.
    pub fn is_tombstoned(&self, validator: ValidatorId) -> bool {
        self.tombstoned.read().contains(&validator)
    }

    /// Slash geçmişi.
    pub fn slash_history(&self) -> Vec<SlashRecord> {
        self.slash_history.read().clone()
    }

    /// Belirli validator'ın slash geçmişi.
    pub fn slash_history_for(&self, validator: ValidatorId) -> Vec<SlashRecord> {
        self.slash_history
            .read()
            .iter()
            .filter(|r| r.validator == validator)
            .cloned()
            .collect()
    }

    /// Toplam slash sayısı.
    pub fn total_slash_count(&self) -> usize {
        self.slash_history.read().len()
    }

    // ─── Yardımcılar ──────────────────────────────────────────────────────────

    /// Kanıt için tekil anahtar (validator + height).
    fn evidence_key(&self, evidence: &DoubleSignEvidence) -> [u8; 32] {
        let mut key = [0u8; 32];
        let vid_bytes = evidence.validator.0.to_le_bytes();
        key[..8].copy_from_slice(&vid_bytes);
        let h = evidence.height.to_le_bytes();
        key[8..16].copy_from_slice(&h);
        key
    }
}

// ─── Graduated Slashing (3 Tier) ─────────────────────────────────────────────

/// Slash tier — kümülatif offence geçmişine göre ceza kademesi
///
/// Polkadot: `sp_staking::offence::Offence` + escalating slash fraction
/// Cosmos SDK: `ConsecutiveDowntimeSlashMultiplier`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlashTier {
    /// Tier 1 — İlk offence, uyarı/küçük ceza
    Minor,
    /// Tier 2 — Tekrarlayan offence, orta ceza
    Moderate,
    /// Tier 3 — Sistematik offence, ağır ceza + kalıcı jail
    Severe,
}

impl SlashTier {
    /// Tier'e göre slash multiplier (base_bps üzerine)
    pub fn multiplier(&self) -> u32 {
        match self {
            SlashTier::Minor => 1,
            SlashTier::Moderate => 3,
            SlashTier::Severe => 10,
        }
    }

    /// Jail süresi (slot)
    pub fn jail_duration_slots(&self) -> u64 {
        match self {
            SlashTier::Minor => 100_800,        // ~7 gün @ 400ms
            SlashTier::Moderate => 302_400,      // ~3 hafta
            SlashTier::Severe => u64::MAX,       // Kalıcı
        }
    }

    pub fn is_severe(&self) -> bool {
        *self == SlashTier::Severe
    }
}

/// Validator'ın slash geçmişi özeti
#[derive(Debug, Clone)]
pub struct ValidatorSlashHistory {
    pub validator_id: ValidatorId,
    /// Toplam offence sayısı
    pub total_offences: u32,
    /// Son offence sloту
    pub last_offence_slot: u64,
    /// Mevcut tier
    pub current_tier: SlashTier,
}

impl ValidatorSlashHistory {
    pub fn new(validator_id: ValidatorId) -> Self {
        Self {
            validator_id,
            total_offences: 0,
            last_offence_slot: 0,
            current_tier: SlashTier::Minor,
        }
    }

    /// Yeni offence ekle, tier'ı güncelle
    pub fn record_offence(&mut self, slot: u64) -> SlashTier {
        self.total_offences += 1;
        self.last_offence_slot = slot;
        self.current_tier = match self.total_offences {
            1 => SlashTier::Minor,
            2..=4 => SlashTier::Moderate,
            _ => SlashTier::Severe,
        };
        self.current_tier
    }
}

/// Koordineli slash hesaplama — Polkadot `f(k,n) = min((3k/n)², 1)` formülü.
///
/// k = aynı suç döneminde suç işleyen validator sayısı
/// n = toplam aktif validator sayısı
///
/// Mantık:
/// - Az sayıda validator suç işlerse (bireysel hata) → küçük slash
/// - Çok sayıda validator aynı anda suç işlerse (koordineli saldırı) → büyük slash
/// - k/n = 0.33 → f ≈ 1.0 → %100 slash (supermajority saldırısı)
///
/// # Örnek
/// n=100 validator, k=10 suçlu: f = (3*10/100)² = 0.09 → 900 bps
/// n=100 validator, k=34 suçlu: f = (3*34/100)² = 1.04 → capped 10000 bps
pub struct CoordinatedSlashCalculator;

impl CoordinatedSlashCalculator {
    /// Polkadot `f(k,n) = min((3k/n)², 1)` formülü → basis points (0..=10_000)
    ///
    /// - `k`: suç işleyen validator sayısı (≥1)
    /// - `n`: toplam aktif validator sayısı (≥1)
    ///
    /// Döndürür: slash_bps ∈ [0, 10_000]
    pub fn slash_fraction_bps(k: u64, n: u64) -> u32 {
        if k == 0 || n == 0 {
            return 0;
        }
        let k = k.min(n); // k > n mümkün değil

        // (3k/n)² = 9k²/n² — integer aritmetiği için 10^8 ölçekle hesapla
        // fraction = 9 * k² / n² (∈ [0, 9])
        // bps = fraction * 10_000 → max 90_000, capped at 10_000
        let k2 = (k as u128).saturating_mul(k as u128);
        let n2 = (n as u128).saturating_mul(n as u128);
        let numerator = 9u128.saturating_mul(k2).saturating_mul(10_000u128);
        let bps_full = numerator / n2;
        bps_full.min(10_000) as u32
    }

    /// Koordineli downtime slash bps hesapla (downtime için k/n bazlı)
    ///
    /// Solana Tower BFT SWITCH_FORK_THRESHOLD=0.38 ilkesinden: büyük bir
    /// grubun suç işlemesi güvenlik modelinin çöküşünü gösterir → ağır ceza.
    pub fn coordinated_downtime_bps(k: u64, n: u64, base_bps: u32) -> u32 {
        let coordinated = Self::slash_fraction_bps(k, n);
        // Koordineli formül base_bps'ten büyükse onu kullan
        coordinated.max(base_bps)
    }

    /// k/n oranının %33'ü geçip geçmediğini kontrol et (BFT güvenlik sınırı)
    ///
    /// %33+ validator aynı anda suç işliyorsa bu koordineli saldırı sinyalidir.
    pub fn is_coordinated_attack(k: u64, n: u64) -> bool {
        // k/n > 1/3 → k*3 > n
        k.saturating_mul(3) > n
    }
}

/// Kademeli (graduated) slash hesaplama
///
/// Polkadot: `slash_fraction = min(3 * slash_fraction, 1)` per offence
/// Nova: base_bps * tier.multiplier()
pub struct GraduatedSlashCalculator;

impl GraduatedSlashCalculator {
    /// Toplam slash bps hesapla
    pub fn calculate_slash_bps(base_bps: u32, tier: SlashTier) -> u32 {
        let raw = base_bps * tier.multiplier();
        raw.min(10_000) // %100'ü geçemez
    }

    /// Tier'e göre açıklama
    pub fn tier_description(tier: SlashTier) -> &'static str {
        match tier {
            SlashTier::Minor => "Tier 1: İlk offence — küçük ceza",
            SlashTier::Moderate => "Tier 2: Tekrarlayan offence — orta ceza",
            SlashTier::Severe => "Tier 3: Sistematik offence — ağır ceza + kalıcı jail",
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DoubleSignEvidence, MIN_VALIDATOR_STAKE, SLASH_DOWNTIME_BPS, SLASH_MEMPOOL_LEAK_BPS,
        SLASH_WRONG_BATCH_BPS,
    };
    use nova_primitives::{Address, Balance, BlockHash, SlotId, ValidatorId};

    fn make_vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn make_registry_with_validator(seed: u8) -> Arc<ValidatorRegistry> {
        let reg = Arc::new(ValidatorRegistry::new());
        reg.register(
            make_vid(seed),
            Address::from_bytes([seed; 20]),
            Balance(MIN_VALIDATOR_STAKE * 10),
            500,
            [seed; 32],
            SlotId(0),
        )
        .unwrap();
        reg
    }

    fn make_engine(seed: u8) -> (Arc<ValidatorRegistry>, SlashingEngine) {
        let reg = make_registry_with_validator(seed);
        let engine = SlashingEngine::new(Arc::clone(&reg));
        (reg, engine)
    }

    fn valid_evidence(validator: ValidatorId) -> DoubleSignEvidence {
        DoubleSignEvidence {
            validator,
            height: 100,
            block_hash_a: BlockHash::from_bytes([0xAAu8; 32]),
            block_hash_b: BlockHash::from_bytes([0xBBu8; 32]),
            submitted_at_slot: SlotId(1_000),
        }
    }

    // ─── handle_double_sign ───────────────────────────────────────────────────

    #[test]
    fn test_double_sign_tombstones_validator() {
        let (reg, engine) = make_engine(1);
        let v = make_vid(1);

        engine.handle_double_sign(valid_evidence(v), SlotId(1_000)).unwrap();

        assert!(engine.is_tombstoned(v));
        let info = reg.get(&v).unwrap();
        assert!(info.status.is_tombstoned());
    }

    #[test]
    fn test_double_sign_emits_slash_and_tombstone_events() {
        let (_, engine) = make_engine(2);
        let v = make_vid(2);

        let events = engine.handle_double_sign(valid_evidence(v), SlotId(1_000)).unwrap();

        let has_slash = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorSlashed { .. })
        });
        let has_tombstone = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorTombstoned { .. })
        });
        assert!(has_slash);
        assert!(has_tombstone);
    }

    #[test]
    fn test_double_sign_slashes_full_stake() {
        let (reg, engine) = make_engine(3);
        let v = make_vid(3);

        engine.handle_double_sign(valid_evidence(v), SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        // %100 slash → stake = 0
        assert_eq!(info.stake, Balance(0));
    }

    #[test]
    fn test_double_sign_same_evidence_twice_errors() {
        let (_, engine) = make_engine(4);
        let v = make_vid(4);

        engine.handle_double_sign(valid_evidence(v), SlotId(1_000)).unwrap();

        // Aynı kanıt tekrar
        let err = engine.handle_double_sign(valid_evidence(v), SlotId(1_001));
        assert!(matches!(err, Err(ConsensusError::InvalidDoubleSignEvidence)));
    }

    #[test]
    fn test_double_sign_invalid_evidence_errors() {
        let (_, engine) = make_engine(5);
        let v = make_vid(5);

        // Aynı hash → geçersiz
        let bad_evidence = DoubleSignEvidence {
            validator: v,
            height: 100,
            block_hash_a: BlockHash::from_bytes([0xAAu8; 32]),
            block_hash_b: BlockHash::from_bytes([0xAAu8; 32]), // aynı
            submitted_at_slot: SlotId(1_000),
        };
        let err = engine.handle_double_sign(bad_evidence, SlotId(1_000));
        assert!(matches!(err, Err(ConsensusError::InvalidDoubleSignEvidence)));
    }

    #[test]
    fn test_double_sign_too_old_evidence_errors() {
        let (_, engine) = make_engine(6);
        let v = make_vid(6);

        let old_evidence = DoubleSignEvidence {
            validator: v,
            height: 100,
            block_hash_a: BlockHash::from_bytes([0xAAu8; 32]),
            block_hash_b: BlockHash::from_bytes([0xBBu8; 32]),
            submitted_at_slot: SlotId(0),
        };

        // current_slot çok ileride
        let current = SlotId(MAX_EVIDENCE_AGE_SLOTS + 1);
        let err = engine.handle_double_sign(old_evidence, current);
        assert!(matches!(err, Err(ConsensusError::EvidenceTooOld { .. })));
    }

    #[test]
    fn test_double_sign_creates_slash_record() {
        let (_, engine) = make_engine(7);
        let v = make_vid(7);

        engine.handle_double_sign(valid_evidence(v), SlotId(1_000)).unwrap();

        let history = engine.slash_history_for(v);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reason, SlashReason::DoubleSign);
        assert_eq!(history[0].slash_bps, SLASH_DOUBLE_SIGN_BPS);
    }

    // ─── slash_downtime ───────────────────────────────────────────────────────

    #[test]
    fn test_downtime_slash_and_jail() {
        let (reg, engine) = make_engine(10);
        let v = make_vid(10);

        let events = engine.slash_downtime(v, SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        assert!(info.status.is_jailed());

        let has_slash = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorSlashed { .. })
        });
        let has_jail = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorJailed { .. })
        });
        assert!(has_slash);
        assert!(has_jail);
    }

    #[test]
    fn test_downtime_slash_bps() {
        let (reg, engine) = make_engine(11);
        let v = make_vid(11);
        let initial_stake = MIN_VALIDATOR_STAKE * 10;

        engine.slash_downtime(v, SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        let expected_slash = initial_stake * SLASH_DOWNTIME_BPS as u128 / 10_000;
        let expected_remaining = initial_stake - expected_slash;
        assert_eq!(info.stake.0, expected_remaining);
    }

    // ─── slash_mempool_leak ───────────────────────────────────────────────────

    #[test]
    fn test_mempool_leak_slash_bps() {
        let (reg, engine) = make_engine(20);
        let v = make_vid(20);
        let initial_stake = MIN_VALIDATOR_STAKE * 10;

        engine.slash_mempool_leak(v, SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        let expected_slash = initial_stake * SLASH_MEMPOOL_LEAK_BPS as u128 / 10_000;
        let expected_remaining = initial_stake - expected_slash;
        assert_eq!(info.stake.0, expected_remaining);
    }

    /// NOVA-RED-CONSENSUS-001 düzeltme regresyonu: MempoolLeak → tombstone (artık sadece jail değil)
    #[test]
    fn test_mempool_leak_tombstones_validator() {
        let (reg, engine) = make_engine(21);
        let v = make_vid(21);

        let events = engine.slash_mempool_leak(v, SlotId(1_000)).unwrap();

        // Tombstoned olmalı (artık sadece jail değil)
        assert!(engine.is_tombstoned(v), "mempool leak → tombstone olmalı");
        let info = reg.get(&v).unwrap();
        assert!(info.status.is_tombstoned(), "registry tombstone olmalı");

        // Tombstone event yayınlanmalı
        let has_tombstone = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorTombstoned { .. })
        });
        assert!(has_tombstone, "tombstone event yayınlanmalı");

        // Slash event yayınlanmalı
        let has_slash = events.iter().any(|e| {
            matches!(e.event_type, ConsensusEventType::ValidatorSlashed { .. })
        });
        assert!(has_slash, "slash event yayınlanmalı");
    }

    // ─── slash_wrong_batch ────────────────────────────────────────────────────

    #[test]
    fn test_wrong_batch_slash_bps() {
        let (reg, engine) = make_engine(30);
        let v = make_vid(30);
        let initial_stake = MIN_VALIDATOR_STAKE * 10;

        engine.slash_wrong_batch(v, SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        let expected_slash = initial_stake * SLASH_WRONG_BATCH_BPS as u128 / 10_000;
        let expected_remaining = initial_stake - expected_slash;
        assert_eq!(info.stake.0, expected_remaining);
    }

    #[test]
    fn test_wrong_batch_jails() {
        let (reg, engine) = make_engine(31);
        let v = make_vid(31);

        engine.slash_wrong_batch(v, SlotId(1_000)).unwrap();

        let info = reg.get(&v).unwrap();
        assert!(info.status.is_jailed());
    }

    // ─── slash_history ────────────────────────────────────────────────────────

    #[test]
    fn test_slash_history_grows() {
        let reg = Arc::new(ValidatorRegistry::new());
        for i in 1u8..=3 {
            reg.register(
                make_vid(i),
                Address::from_bytes([i; 20]),
                Balance(MIN_VALIDATOR_STAKE * 10),
                500,
                [i; 32],
                SlotId(0),
            )
            .unwrap();
        }
        let engine = SlashingEngine::new(Arc::clone(&reg));

        engine.slash_downtime(make_vid(1), SlotId(100)).unwrap();
        engine.slash_mempool_leak(make_vid(2), SlotId(200)).unwrap();
        engine.slash_wrong_batch(make_vid(3), SlotId(300)).unwrap();

        assert_eq!(engine.total_slash_count(), 3);
    }

    #[test]
    fn test_slash_history_for_specific_validator() {
        let (_, engine) = make_engine(40);
        let v = make_vid(40);

        engine.slash_downtime(v, SlotId(100)).unwrap();

        let history = engine.slash_history_for(v);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].validator, v);
    }

    #[test]
    fn test_is_tombstoned_false_for_non_tombstoned() {
        let (_, engine) = make_engine(50);
        let v = make_vid(50);
        assert!(!engine.is_tombstoned(v));
    }

    // ─── Graduated Slashing testleri ─────────────────────────────────────────

    #[test]
    fn test_slash_tier_minor_first_offence() {
        let mut history = ValidatorSlashHistory::new(make_vid(1));
        let tier = history.record_offence(100);
        assert_eq!(tier, SlashTier::Minor);
        assert_eq!(history.total_offences, 1);
    }

    #[test]
    fn test_slash_tier_moderate_second_offence() {
        let mut history = ValidatorSlashHistory::new(make_vid(1));
        history.record_offence(100);
        let tier = history.record_offence(200);
        assert_eq!(tier, SlashTier::Moderate);
    }

    #[test]
    fn test_slash_tier_severe_fifth_offence() {
        let mut history = ValidatorSlashHistory::new(make_vid(1));
        for i in 1..=5u64 {
            history.record_offence(i * 100);
        }
        assert_eq!(history.current_tier, SlashTier::Severe);
        assert!(history.current_tier.is_severe());
    }

    #[test]
    fn test_slash_tier_ordering() {
        assert!(SlashTier::Minor < SlashTier::Moderate);
        assert!(SlashTier::Moderate < SlashTier::Severe);
    }

    #[test]
    fn test_slash_tier_jail_duration_escalates() {
        assert!(SlashTier::Minor.jail_duration_slots() < SlashTier::Moderate.jail_duration_slots());
        assert!(SlashTier::Moderate.jail_duration_slots() < SlashTier::Severe.jail_duration_slots());
        // Severe = kalıcı
        assert_eq!(SlashTier::Severe.jail_duration_slots(), u64::MAX);
    }

    #[test]
    fn test_graduated_slash_bps_escalates() {
        let base_bps = 100u32;
        let minor_bps = GraduatedSlashCalculator::calculate_slash_bps(base_bps, SlashTier::Minor);
        let moderate_bps = GraduatedSlashCalculator::calculate_slash_bps(base_bps, SlashTier::Moderate);
        let severe_bps = GraduatedSlashCalculator::calculate_slash_bps(base_bps, SlashTier::Severe);

        assert_eq!(minor_bps, 100);    // 1x
        assert_eq!(moderate_bps, 300); // 3x
        assert_eq!(severe_bps, 1000);  // 10x
        assert!(minor_bps < moderate_bps);
        assert!(moderate_bps < severe_bps);
    }

    #[test]
    fn test_graduated_slash_bps_capped_at_10000() {
        // 10x 2000bps = 20000 → capped at 10000
        let bps = GraduatedSlashCalculator::calculate_slash_bps(2000, SlashTier::Severe);
        assert_eq!(bps, 10_000);
    }

    #[test]
    fn test_tier_multiplier() {
        assert_eq!(SlashTier::Minor.multiplier(), 1);
        assert_eq!(SlashTier::Moderate.multiplier(), 3);
        assert_eq!(SlashTier::Severe.multiplier(), 10);
    }

    #[test]
    fn test_validator_slash_history_tracks_slots() {
        let mut history = ValidatorSlashHistory::new(make_vid(5));
        history.record_offence(500);
        assert_eq!(history.last_offence_slot, 500);
        history.record_offence(1000);
        assert_eq!(history.last_offence_slot, 1000);
    }

    // ─── CoordinatedSlashCalculator testleri ─────────────────────────────────

    #[test]
    fn test_coordinated_slash_single_validator() {
        // k=1, n=100: f=(3*1/100)²=0.0009 → 9 bps
        let bps = CoordinatedSlashCalculator::slash_fraction_bps(1, 100);
        assert_eq!(bps, 9, "k=1/100 → 9 bps");
    }

    #[test]
    fn test_coordinated_slash_ten_percent() {
        // k=10, n=100: f=(3*10/100)²=0.09 → 900 bps
        let bps = CoordinatedSlashCalculator::slash_fraction_bps(10, 100);
        assert_eq!(bps, 900, "k=10/100 → 900 bps");
    }

    #[test]
    fn test_coordinated_slash_supermajority_capped() {
        // k=34, n=100: f=(3*34/100)²=1.0404 → capped at 10000 bps
        let bps = CoordinatedSlashCalculator::slash_fraction_bps(34, 100);
        assert_eq!(bps, 10_000, "k=34/100 → %100 slash");
    }

    #[test]
    fn test_coordinated_slash_zero_k() {
        let bps = CoordinatedSlashCalculator::slash_fraction_bps(0, 100);
        assert_eq!(bps, 0);
    }

    #[test]
    fn test_coordinated_slash_k_equals_n() {
        // k=n → f=(3*1)²=9 → capped at 10000
        let bps = CoordinatedSlashCalculator::slash_fraction_bps(100, 100);
        assert_eq!(bps, 10_000);
    }

    #[test]
    fn test_coordinated_slash_escalates_with_k() {
        // k arttıkça bps artmalı
        let n = 100u64;
        let bps1 = CoordinatedSlashCalculator::slash_fraction_bps(1, n);
        let bps5 = CoordinatedSlashCalculator::slash_fraction_bps(5, n);
        let bps10 = CoordinatedSlashCalculator::slash_fraction_bps(10, n);
        assert!(bps1 < bps5, "k=5 > k=1");
        assert!(bps5 < bps10, "k=10 > k=5");
    }

    #[test]
    fn test_coordinated_attack_detection() {
        // k/n > 1/3 → koordineli saldırı
        assert!(CoordinatedSlashCalculator::is_coordinated_attack(34, 100));
        assert!(!CoordinatedSlashCalculator::is_coordinated_attack(33, 100));
        assert!(!CoordinatedSlashCalculator::is_coordinated_attack(1, 100));
    }

    #[test]
    fn test_coordinated_downtime_uses_higher_of_base_and_coordinated() {
        // Koordineli formül base_bps'ten küçükse base_bps kullan
        let base_bps = 100u32;
        let bps = CoordinatedSlashCalculator::coordinated_downtime_bps(1, 100, base_bps);
        // k=1/100 → 9 bps < 100 bps → base_bps kullan
        assert_eq!(bps, base_bps);

        // Koordineli formül daha büyükse onu kullan
        let bps_large = CoordinatedSlashCalculator::coordinated_downtime_bps(20, 100, base_bps);
        // k=20/100 → 3600 bps > 100 bps
        assert_eq!(bps_large, 3600);
    }

    #[test]
    fn test_tier_description_not_empty() {
        for tier in [SlashTier::Minor, SlashTier::Moderate, SlashTier::Severe] {
            assert!(!GraduatedSlashCalculator::tier_description(tier).is_empty());
        }
    }
}
