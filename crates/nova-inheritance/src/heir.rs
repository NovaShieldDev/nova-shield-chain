//! Hiyerarşik Varis Yönetimi
//!
//! ## Araştırma Kaynağı: Avalanchego Multisig + Cosmos Authz
//!
//! ### Avalanchego Multisig
//! `research/chains/avalanchego/vms/platformvm/txs/`:
//! ```text
//! // OutputOwners: çoklu alıcı tanımı
//! type OutputOwners struct {
//!     Locktime   uint64
//!     Threshold  uint32
//!     Addrs      []ids.ShortID
//! }
//! func (o *OutputOwners) Verify() error {
//!     if int(o.Threshold) > len(o.Addrs) { return ErrThresholdExceedsOwnerCount }
//! }
//! ```
//!
//! ### Cosmos Authz
//! `research/chains/cosmos-sdk/x/authz/`:
//! - `Authorization` — belirli adreslere belirli yetkiler verir
//! - Süre sınırlı yetki devri
//! - Yanıt süresi (expiration)
//!
//! ## Nova Uyarlaması
//!
//! Varis hiyerarşisi:
//! 1. Birincil varisler → yanıt vermezse ikincil
//! 2. İkincil varisler → yanıt vermezse üçüncül
//! 3. Üçüncül varisler → yanıt vermezse hazine/yakma

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{HeirRecord, HeirTier, InheritancePlan, InheritanceState, MAX_HEIRS};
use nova_primitives::types::{Address, Balance, SlotId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── VARİS ONAY DURUMU ───────────────────────────────────────────────────────

/// Tek bir varis'in onay durumu.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeirAcknowledgement {
    /// Varis adresi.
    pub heir_address: Address,
    /// Katmanı.
    pub tier: HeirTier,
    /// Onay alındı mı?
    pub acknowledged: bool,
    /// Onay alınan slot.
    pub acknowledged_at: Option<SlotId>,
    /// Yanıt süresi doldu mu?
    pub timed_out: bool,
    /// Aktif mi? (üst kat yanıt verdiyse alt kat aktif olmaz)
    pub active: bool,
}

// ─── VARİS ÇÖZÜMLEME SONUCU ──────────────────────────────────────────────────

/// Hangi varisler aktif ve ne kadar pay alacak.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeirResolution {
    /// Aktif katman.
    pub active_tier: HeirTier,
    /// Aktif varisler ve pay miktarları.
    pub allocations: Vec<(Address, u128)>,
    /// Toplam dağıtılacak miktar.
    pub total_amount: u128,
    /// Dağıtılamayan miktar (yuvarlama artığı).
    pub remainder: u128,
}

impl HeirResolution {
    /// Dağıtımın tutarlı olduğunu doğrula.
    pub fn verify(&self) -> bool {
        let total: u128 = self.allocations.iter().map(|(_, a)| a).sum();
        total + self.remainder == self.total_amount
    }
}

// ─── VARİS YÖNETİCİSİ ────────────────────────────────────────────────────────

/// Varis hiyerarşisi yönetimi.
///
/// Avalanchego `OutputOwners.Verify()` + Cosmos Authz yetki devri analog.
pub struct HeirManager;

impl HeirManager {
    /// Devir anında hangi katman aktif ve pay dağılımı ne?
    ///
    /// Algoritma:
    /// 1. Birincil katmanda yanıt veren var mı? → onlar alır
    /// 2. Hayır → İkincil katmana bak
    /// 3. Hayır → Üçüncül katmana bak
    /// 4. Hiç kimse yok → TreasuryBurn
    pub fn resolve_active_heirs(
        plan: &InheritancePlan,
        state: &InheritanceState,
        total_amount: u128,
        current_slot: SlotId,
    ) -> HeirResolution {
        // Hangi varisler onay verdi?
        let acknowledged: std::collections::HashSet<Address> = state
            .heir_acknowledgements
            .iter()
            .map(|(a, _)| *a)
            .collect();

        // Katman sırasıyla dene
        for tier in [HeirTier::Primary, HeirTier::Secondary, HeirTier::Tertiary] {
            let tier_heirs = plan.heirs_at_tier(tier);
            if tier_heirs.is_empty() {
                continue;
            }

            // Bu katmanda yanıt veren var mı?
            //
            // NOVA-RED-INH-006: Yalnızca açıkça onay veren varisler aktif sayılır.
            // Önceki hatalı davranış: timeout dolmamış herkes otomatik aktif kabul ediliyordu.
            // Doğru davranış: aktif olmak için heir_acknowledgements'ta yer alması gerekir.
            // Timeout dolmuş ve onay vermemiş varisler hariç tutulur.
            let active_heirs: Vec<&HeirRecord> = tier_heirs
                .iter()
                .filter(|h| acknowledged.contains(&h.address))
                .cloned()
                .collect();

            if !active_heirs.is_empty() {
                return Self::compute_allocations(tier, &active_heirs, total_amount);
            }
        }

        // Hiç kimse yok → TreasuryBurn
        HeirResolution {
            active_tier: HeirTier::TreasuryBurn,
            allocations: vec![], // nova-runtime hazine adresine gönderir
            total_amount,
            remainder: total_amount, // Tümü "remainder" olarak işaretlenir
        }
    }

    /// Varis'in onay süresi doldu mu?
    fn has_timed_out(
        heir: &HeirRecord,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> bool {
        let Some(release_start) = state.release_started_at else {
            return false; // Devir henüz başlamadı
        };

        let elapsed = current_slot.0.saturating_sub(release_start.0);
        elapsed > heir.onboarding_timeout_slots
    }

    /// Belirli kattaki varisler için pay dağılımı hesapla.
    ///
    /// Avalanchego `OutputOwners` - her adrese ağırlıklı pay.
    fn compute_allocations(
        tier: HeirTier,
        heirs: &[&HeirRecord],
        total_amount: u128,
    ) -> HeirResolution {
        let mut allocations = Vec::new();
        let mut distributed: u128 = 0;

        // Her varise payını hesapla
        let total_share: u32 = heirs.iter().map(|h| h.share_bps).sum();

        for heir in heirs {
            // Toplam pay 10000 değilse normalize et
            let normalized_bps = if total_share > 0 {
                (heir.share_bps as u128 * 10_000) / total_share as u128
            } else {
                10_000 / heirs.len() as u128 // Eşit dağıt
            };

            let amount = total_amount
                .saturating_mul(normalized_bps)
                .saturating_div(10_000);

            allocations.push((heir.address, amount));
            distributed = distributed.saturating_add(amount);
        }

        let remainder = total_amount.saturating_sub(distributed);

        // Artığı ilk varişe ver (yuvarlama hatası minimizasyonu)
        if remainder > 0 && !allocations.is_empty() {
            allocations[0].1 = allocations[0].1.saturating_add(remainder);
        }

        HeirResolution {
            active_tier: tier,
            allocations,
            total_amount,
            remainder: 0, // Hepsini dağıttık
        }
    }

    /// Varis'in planı kabul ettiğini kaydet.
    pub fn record_acknowledgement(
        state: &mut InheritanceState,
        heir_address: Address,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        // Zaten onay verdi mi?
        if state.heir_acknowledgements.iter().any(|(a, _)| *a == heir_address) {
            return Ok(()); // Idempotent — iki kez onay sorun değil
        }

        state.record_heir_acknowledgement(heir_address, current_slot);
        Ok(())
    }

    /// Plan'daki varis listesini doğrula.
    ///
    /// Avalanchego `Verify()` analog.
    pub fn validate_heirs(plan: &InheritancePlan) -> InheritanceResult<()> {
        if plan.heirs.is_empty() {
            return Err(InheritanceError::EmptyHeirList);
        }

        if plan.heirs.len() > MAX_HEIRS {
            return Err(InheritanceError::TooManyHeirs { count: plan.heirs.len() });
        }

        // Her kattaki pay toplamı = 10000
        let mut tier_totals: HashMap<u8, u32> = HashMap::new();
        let mut seen_addresses = std::collections::HashSet::new();

        for heir in &plan.heirs {
            let addr_bytes = heir.address.as_bytes().to_owned();
            if !seen_addresses.insert(addr_bytes) {
                return Err(InheritanceError::HeirAlreadyExists {
                    heir_address: *heir.address.as_bytes(),
                });
            }

            // NOVA-RED-INHERITANCE-001: u32 wrapping overflow koruması.
            // share_bps değerleri kasıtlı olarak 2^32 mod 10_000 = 0 yapabilir.
            // checked_add ile overflow tespiti yapılır.
            let entry = tier_totals.entry(heir.tier as u8).or_insert(0u32);
            *entry = entry.checked_add(heir.share_bps).ok_or_else(|| {
                InheritanceError::InvalidShareTotal { total_bps: u32::MAX }
            })?;
        }

        for (tier, total) in &tier_totals {
            if *total != 10_000 {
                return Err(InheritanceError::InvalidShareTotal { total_bps: *total });
            }
        }

        Ok(())
    }

    /// Bir varis'i plan'dan çıkar (güncelleme için).
    pub fn can_remove_heir(plan: &InheritancePlan, heir_address: &Address) -> bool {
        // Kaldırmak istenen varis gerçekten listede mi?
        plan.heirs.iter().any(|h| &h.address == heir_address)
    }
}

// ─── VARİS ONAY DEĞERLENDİRMESİ ─────────────────────────────────────────────

/// Devir anında varis onay durumu özeti.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeirAcknowledgementSummary {
    /// Toplam varis sayısı.
    pub total_heirs: usize,
    /// Onay veren varis sayısı.
    pub acknowledged_count: usize,
    /// Aktif katman (şimdiye kadar).
    pub current_active_tier: Option<HeirTier>,
    /// Birincil katta onay var mı?
    pub primary_acknowledged: bool,
    /// İkincil katta onay var mı?
    pub secondary_acknowledged: bool,
}

impl HeirAcknowledgementSummary {
    pub fn compute(plan: &InheritancePlan, state: &InheritanceState) -> Self {
        let acknowledged: std::collections::HashSet<Address> = state
            .heir_acknowledgements
            .iter()
            .map(|(a, _)| *a)
            .collect();

        let total_heirs = plan.heirs.len();
        let acknowledged_count = acknowledged.len();

        let primary_acknowledged = plan.primary_heirs().iter().any(|h| acknowledged.contains(&h.address));
        let secondary_acknowledged = plan.secondary_heirs().iter().any(|h| acknowledged.contains(&h.address));

        let current_active_tier = if primary_acknowledged {
            Some(HeirTier::Primary)
        } else if secondary_acknowledged {
            Some(HeirTier::Secondary)
        } else {
            None
        };

        Self {
            total_heirs,
            acknowledged_count,
            current_active_tier,
            primary_acknowledged,
            secondary_acknowledged,
        }
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        HeirRecord, HeirTier, InheritancePlan, InheritancePlanId, InheritanceState,
        StagedReleaseStep, TriggerType,
    };
    use nova_primitives::types::Address;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_plan_with_heirs(heirs: Vec<HeirRecord>) -> InheritancePlan {
        InheritancePlan {
            plan_id: InheritancePlanId([0u8; 32]),
            owner_account_id: 1,
            owner_address: addr(0),
            trigger: TriggerType::Inactivity { period_secs: 30 * 24 * 3600 },
            grace_period_slots: 2_000_000,
            heirs,
            release_steps: vec![],
            conditions: vec![],
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
            guardian_ids: vec![],
        }
    }

    fn make_state() -> InheritanceState {
        InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0))
    }

    // Tek birincil varis, tam pay alır
    // NOVA-RED-INH-006: Tek birincil varis — onay verdikten sonra tam tutarı alır
    #[test]
    fn test_single_primary_heir_full_amount() {
        let heirs = vec![HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000)];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();
        state.record_heir_acknowledgement(addr(1), SlotId(500)); // Onay gerekli

        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 1_000_000, SlotId(1000));
        assert_eq!(resolution.active_tier, HeirTier::Primary);
        assert_eq!(resolution.allocations.len(), 1);
        assert_eq!(resolution.allocations[0].1, 1_000_000);
    }

    // NOVA-RED-INH-006: Onay vermeden aktif sayılmaz → TreasuryBurn
    #[test]
    fn test_single_primary_heir_no_acknowledgement_treasury_burn() {
        let heirs = vec![HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000)];
        let plan = make_plan_with_heirs(heirs);
        let state = make_state(); // Onay yok

        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 1_000_000, SlotId(1000));
        assert_eq!(resolution.active_tier, HeirTier::TreasuryBurn);
    }

    // İki birincil varis, %60/%40 pay — ikisi de onay vermiş
    #[test]
    fn test_two_primary_heirs_split() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 6_000, 100_000),
            HeirRecord::new(addr(2), HeirTier::Primary, 4_000, 100_000),
        ];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();
        state.record_heir_acknowledgement(addr(1), SlotId(100)); // Onay gerekli
        state.record_heir_acknowledgement(addr(2), SlotId(100)); // Onay gerekli

        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 1_000_000, SlotId(1000));
        assert_eq!(resolution.active_tier, HeirTier::Primary);

        let heir1 = resolution.allocations.iter().find(|(a, _)| *a == addr(1)).unwrap();
        let heir2 = resolution.allocations.iter().find(|(a, _)| *a == addr(2)).unwrap();
        assert_eq!(heir1.1, 600_000);
        assert_eq!(heir2.1, 400_000);
    }

    // Birincil varis onay vermedi, ikincil onay verdi → ikincil devralır
    #[test]
    fn test_fallback_to_secondary_on_timeout() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100), // 100 slot timeout
            HeirRecord::new(addr(2), HeirTier::Secondary, 10_000, 200_000),
        ];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();
        state.release_started_at = Some(SlotId(0));
        // addr(1) onay vermedi (primary → aktif sayılmaz)
        state.record_heir_acknowledgement(addr(2), SlotId(100)); // Secondary onay verdi

        // NOVA-RED-INH-006: Yalnızca onay verenler aktif — ikincil devralır
        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 1_000_000, SlotId(200));
        assert_eq!(resolution.active_tier, HeirTier::Secondary);
        assert_eq!(resolution.allocations[0].0, addr(2));
    }

    // Hiç kimse yok → TreasuryBurn
    #[test]
    fn test_all_timed_out_treasury_burn() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 10), // 10 slot timeout
        ];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();
        state.release_started_at = Some(SlotId(0));

        // 1000 slot sonra → birincil timeout, ikincil/üçüncül yok → TreasuryBurn
        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 1_000_000, SlotId(1000));
        assert_eq!(resolution.active_tier, HeirTier::TreasuryBurn);
    }

    // Validate: boş liste
    #[test]
    fn test_validate_empty_heirs() {
        let plan = make_plan_with_heirs(vec![]);
        let err = HeirManager::validate_heirs(&plan).unwrap_err();
        assert!(matches!(err, InheritanceError::EmptyHeirList));
    }

    // Validate: pay toplamı hatalı
    #[test]
    fn test_validate_invalid_share_total() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 5_000, 100_000),
            HeirRecord::new(addr(2), HeirTier::Primary, 3_000, 100_000),
            // Toplam = 8000, 10000 olmalı
        ];
        let plan = make_plan_with_heirs(heirs);
        let err = HeirManager::validate_heirs(&plan).unwrap_err();
        assert!(matches!(err, InheritanceError::InvalidShareTotal { total_bps: 8_000 }));
    }

    // Validate: duplicate varis
    #[test]
    fn test_validate_duplicate_heir() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 5_000, 100_000),
            HeirRecord::new(addr(1), HeirTier::Primary, 5_000, 100_000), // Aynı adres
        ];
        let plan = make_plan_with_heirs(heirs);
        let err = HeirManager::validate_heirs(&plan).unwrap_err();
        assert!(matches!(err, InheritanceError::HeirAlreadyExists { .. }));
    }

    // Varis onayı kayıt — idempotent
    #[test]
    fn test_heir_acknowledgement_idempotent() {
        let mut state = make_state();
        HeirManager::record_acknowledgement(&mut state, addr(1), SlotId(100)).unwrap();
        HeirManager::record_acknowledgement(&mut state, addr(1), SlotId(200)).unwrap();

        assert_eq!(state.heir_acknowledgements.len(), 1);
    }

    // Onay veren birincil varis kazanır
    #[test]
    fn test_acknowledged_primary_wins() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100),
            HeirRecord::new(addr(2), HeirTier::Secondary, 10_000, 200_000),
        ];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();
        state.release_started_at = Some(SlotId(0));

        // Birincil varis 1000 slot sonra onay verdi (timeout = 100)
        // Ama onay vermişse → aktif sayılır
        state.record_heir_acknowledgement(addr(1), SlotId(1000));

        let resolution = HeirManager::resolve_active_heirs(&plan, &state, 500_000, SlotId(1000));
        assert_eq!(resolution.active_tier, HeirTier::Primary);
        assert_eq!(resolution.allocations[0].0, addr(1));
    }

    // HeirAcknowledgementSummary
    #[test]
    fn test_acknowledgement_summary() {
        let heirs = vec![
            HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000),
            HeirRecord::new(addr(2), HeirTier::Secondary, 10_000, 100_000),
        ];
        let plan = make_plan_with_heirs(heirs);
        let mut state = make_state();

        state.record_heir_acknowledgement(addr(1), SlotId(100));

        let summary = HeirAcknowledgementSummary::compute(&plan, &state);
        assert!(summary.primary_acknowledged);
        assert!(!summary.secondary_acknowledged);
        assert_eq!(summary.acknowledged_count, 1);
        assert_eq!(summary.current_active_tier, Some(HeirTier::Primary));
    }

    // Maksimum varis sayısı kontrolü
    #[test]
    fn test_max_heirs_exceeded() {
        let heirs: Vec<HeirRecord> = (0u8..16)
            .map(|i| HeirRecord::new(addr(i + 1), HeirTier::Primary, 625, 100_000))
            .collect();
        let plan = make_plan_with_heirs(heirs);
        let err = HeirManager::validate_heirs(&plan).unwrap_err();
        assert!(matches!(err, InheritanceError::TooManyHeirs { count: 16 }));
    }
}
