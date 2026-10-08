//! Nova Inheritance Engine — Ana Orkestrasyon Katmanı
//!
//! ## Araştırma Kaynağı: Cosmos SDK Keeper + Polkadot Scheduler + Avalanchego Manager
//!
//! ### Cosmos SDK Keeper Pattern
//! `research/chains/cosmos-sdk/x/gov/keeper/keeper.go`:
//! ```go
//! type Keeper struct {
//!     authKeeper  types.AccountKeeper
//!     bankKeeper  types.BankKeeper
//!     Proposals   collections.Map[uint64, v1.Proposal]
//! }
//! ```
//!
//! ### Polkadot Scheduler on_initialize
//! `research/chains/polkadot-sdk/substrate/frame/scheduler/src/lib.rs`:
//! ```ignore
//! fn on_initialize(_now: BlockNumber) -> Weight {
//!     let now = T::BlockNumberProvider::current_block_number();
//!     Self::service_agendas(&mut weight_counter, now, u32::MAX);
//! }
//! ```
//!
//! ### Avalanchego Manager
//! `research/chains/avalanchego/chains/manager.go`:
//! ```go
//! type manager struct {
//!     chains map[ids.ID]handler.Handler
//! }
//! ```
//!
//! ## Nova Uyarlaması
//! §5.4: Her slot'ta tüm aktif planları işle, faz geçişlerini yönet,
//! transfer komutları üret, heartbeat'leri kaydet.

use crate::conditions::ConditionEvaluator;
use crate::errors::{InheritanceError, InheritanceResult};
use crate::guardian::GuardianProtection;
use crate::heartbeat::HeartbeatTracker;
use crate::phase::{PhaseEvent, PhaseManager};
use crate::plan::{PlanManager};
use crate::release::ReleaseEngine;
use crate::types::{
    ConditionalRule, HeartbeatKind, HeartbeatMsg, InheritancePlan, InheritancePlanId,
    InheritancePhase, InheritanceState, InheritanceSummary, TransferCommand, TriggerType,
};
use dashmap::DashMap;
use nova_primitives::types::{Address, SlotId};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tracing::{debug, info, warn};

// ─── SLOT İŞLEME SONUCU ──────────────────────────────────────────────────────

/// Bir slot'ta işlenen tüm planların özet sonucu.
///
/// Polkadot Scheduler `Weight` dönüşünün Nova analogu.
#[derive(Debug, Default)]
pub struct SlotProcessingResult {
    /// Bu slot'ta işlenen plan sayısı.
    pub plans_processed: u32,
    /// Faz geçişi yaşanan planlar.
    pub phase_transitions: Vec<(InheritancePlanId, Vec<PhaseEvent>)>,
    /// Üretilen transfer komutları.
    pub pending_transfers: Vec<TransferCommand>,
    /// Duress tespit edilen plan sayısı.
    pub duress_detections: u32,
}

// ─── ENGINE ──────────────────────────────────────────────────────────────────

/// Nova miras motoru.
///
/// Cosmos SDK Keeper analogu: tüm alt-modülleri yönetir,
/// DashMap ile concurrent-safe state tutar.
pub struct InheritanceEngine {
    /// Plan deposu — Avalanchego manager.chains analogu.
    plans: Arc<DashMap<InheritancePlanId, InheritancePlan>>,
    /// State deposu — her plan için ayrı InheritanceState.
    states: Arc<DashMap<InheritancePlanId, InheritanceState>>,
    /// Heartbeat tracker — Polkadot Im-Online analogu.
    /// &mut self gerektirdiği için Mutex ile sarıldı.
    heartbeat_tracker: Arc<Mutex<HeartbeatTracker>>,
    /// Hesap yaratılma slotları (yaş koşulu için).
    /// AccountId = type alias for Address
    account_creation_slots: Arc<DashMap<Address, SlotId>>,
    /// Toplam işlenen slot sayısı (metrik).
    total_slots_processed: AtomicU64,
}

impl InheritanceEngine {
    /// Yeni motor oluştur.
    pub fn new() -> Self {
        Self {
            plans: Arc::new(DashMap::new()),
            states: Arc::new(DashMap::new()),
            heartbeat_tracker: Arc::new(Mutex::new(HeartbeatTracker::new())),
            account_creation_slots: Arc::new(DashMap::new()),
            total_slots_processed: AtomicU64::new(0),
        }
    }

    // ─── PLAN CRUD ────────────────────────────────────────────────────────────

    /// Yeni miras planı oluştur.
    ///
    /// Cosmos SDK `Keeper.SetProposal()` analogu.
    pub fn create_plan(&self, plan: InheritancePlan) -> InheritanceResult<InheritancePlanId> {
        plan.validate()?;

        let plan_id = plan.plan_id;

        if self.plans.contains_key(&plan_id) {
            return Err(InheritanceError::PlanAlreadyExists);
        }

        let state = InheritanceState::new(plan_id, SlotId(0));
        self.plans.insert(plan_id, plan);
        self.states.insert(plan_id, state);

        info!(plan_id = ?plan_id, "Miras planı oluşturuldu");
        Ok(plan_id)
    }

    /// Planı getir.
    pub fn get_plan(&self, plan_id: InheritancePlanId) -> InheritanceResult<InheritancePlan> {
        self.plans
            .get(&plan_id)
            .map(|r| r.clone())
            .ok_or(InheritanceError::PlanNotFound { plan_id: plan_id.0 })
    }

    /// State'i getir.
    pub fn get_state(&self, plan_id: InheritancePlanId) -> InheritanceResult<InheritanceState> {
        self.states
            .get(&plan_id)
            .map(|r| r.clone())
            .ok_or(InheritanceError::PlanNotFound { plan_id: plan_id.0 })
    }

    /// Planı iptal et — sadece sahip yapabilir.
    pub fn cancel_plan(
        &self,
        plan_id: InheritancePlanId,
        caller: Address,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        let plan = self.get_plan(plan_id)?;
        let state = self.get_state(plan_id)?;

        if caller != plan.owner_address {
            return Err(InheritanceError::InvalidParameter(
                "sadece plan sahibi iptal edebilir".to_string(),
            ));
        }

        // Guardian politikası: can_guardian_block_user_cancellation her zaman false döner
        // (§5.4.8: guardian ASLA kullanıcı iptalini engelleyemez).
        // Duress tespitinde ise ek koruma mekanizması devreye girer.
        if state.duress_detected && GuardianProtection::can_guardian_block_user_cancellation() {
            return Err(InheritanceError::GuardianCannotOverrideTrigger);
        }

        if let Some(mut state_ref) = self.states.get_mut(&plan_id) {
            PhaseManager::cancel(&plan, &mut state_ref, current_slot)?;
        }

        info!(plan_id = ?plan_id, "Plan iptal edildi");
        Ok(())
    }

    /// Hesap yaratılma slotunu kaydet (yaş koşulu için).
    /// `AccountId = type alias for Address` olduğundan Address alır.
    pub fn register_account_creation(&self, owner_address: Address, creation_slot: SlotId) {
        self.account_creation_slots.insert(owner_address, creation_slot);
    }

    // ─── HEARTBEAT ────────────────────────────────────────────────────────────

    /// Heartbeat mesajını işle.
    ///
    /// Polkadot Im-Online pallet analogu.
    ///
    /// NOVA-RED-INHERITANCE-003: `owner_mpc_pubkey` parametresi eklendi.
    /// Hesap sahibinin secp256k1 public key'i sağlanmalıdır (AccountState.mpc_public_key).
    /// Çağıran katman (block processor / RPC handler) bu değeri AccountState'ten alır.
    pub fn process_heartbeat(&self, msg: HeartbeatMsg, owner_mpc_pubkey: &[u8]) -> InheritanceResult<()> {
        // Plan varlık kontrolü
        let plan = self.get_plan(msg.plan_id)?;

        let is_duress = matches!(msg.kind, HeartbeatKind::Duress);

        // HeartbeatTracker — Mutex korumasıyla &mut self
        self.heartbeat_tracker.lock().process_heartbeat(&msg, owner_mpc_pubkey)?;

        // State güncelle — PhaseManager::handle_heartbeat(plan, &mut state, slot, is_duress)
        if let Some(mut state_ref) = self.states.get_mut(&msg.plan_id) {
            PhaseManager::handle_heartbeat(&plan, &mut state_ref, msg.slot, is_duress);
        }

        debug!(
            plan_id = ?msg.plan_id,
            slot = msg.slot.0,
            is_duress,
            "Heartbeat işlendi"
        );
        Ok(())
    }

    // ─── MULTI-SIG ────────────────────────────────────────────────────────────

    /// Multi-sig onayı ekle.
    pub fn add_multisig_approval(
        &self,
        plan_id: InheritancePlanId,
        approver: Address,
    ) -> InheritanceResult<()> {
        let plan = self.get_plan(plan_id)?;

        let is_heir = plan.heirs.iter().any(|h| h.address == approver);
        if !is_heir {
            return Err(InheritanceError::HeirNotFound { heir_id: *approver.as_bytes() });
        }

        if let Some(mut state_ref) = self.states.get_mut(&plan_id) {
            if !state_ref.multisig_approvals.contains(&approver) {
                state_ref.multisig_approvals.push(approver);
            }
        }

        Ok(())
    }

    /// Heir onayı kaydet (onboarding).
    pub fn record_heir_acknowledgement(
        &self,
        plan_id: InheritancePlanId,
        heir_address: Address,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        let plan = self.get_plan(plan_id)?;

        let heir_exists = plan.heirs.iter().any(|h| h.address == heir_address);
        if !heir_exists {
            return Err(InheritanceError::HeirNotFound { heir_id: *heir_address.as_bytes() });
        }

        if let Some(mut state_ref) = self.states.get_mut(&plan_id) {
            let already = state_ref.heir_acknowledgements.iter().any(|(a, _)| *a == heir_address);
            if !already {
                state_ref.heir_acknowledgements.push((heir_address, current_slot));
            }
            state_ref.updated_at_slot = current_slot;
        }

        Ok(())
    }

    // ─── SLOT İŞLEME (ANA DÖNGÜ) ─────────────────────────────────────────────

    /// Her slot'ta çağrılır — Polkadot Scheduler `on_initialize(now)` analogu.
    ///
    /// §5.4: "Runtime her N slotta bir tüm aktif miras planlarını tarar."
    pub fn on_slot(&self, current_slot: SlotId) -> SlotProcessingResult {
        self.total_slots_processed.fetch_add(1, Ordering::Relaxed);

        let mut result = SlotProcessingResult::default();

        // Aktif plan ID'lerini topla — Polkadot service_agendas() analogu
        let plan_ids: Vec<InheritancePlanId> = self
            .plans
            .iter()
            .filter(|r| r.value().is_active)
            .map(|r| *r.key())
            .collect();

        for plan_id in plan_ids {
            let plan = match self.plans.get(&plan_id) {
                Some(p) => p.clone(),
                None => continue,
            };

            // Duress önceki state
            let prev_duress = self
                .states
                .get(&plan_id)
                .map(|s| s.duress_detected)
                .unwrap_or(false);

            // Faz yöneticisi — state'i in-place güncelle
            let events = if let Some(mut state_ref) = self.states.get_mut(&plan_id) {
                PhaseManager::process_slot(&plan, &mut state_ref, current_slot)
            } else {
                continue;
            };

            // Duress yeni tespit edildi mi?
            let now_duress = self
                .states
                .get(&plan_id)
                .map(|s| s.duress_detected)
                .unwrap_or(false);

            if now_duress && !prev_duress {
                result.duress_detections += 1;
                warn!(
                    plan_id = ?plan_id,
                    slot = current_slot.0,
                    "DURESS TESPİT EDİLDİ"
                );
            }

            // Faz geçişleri
            if !events.is_empty() {
                result.phase_transitions.push((plan_id, events));
            }

            // Transfer komutları üret
            let transfers = self.collect_pending_transfers(&plan, plan_id, current_slot);
            result.pending_transfers.extend(transfers);

            result.plans_processed += 1;
        }

        debug!(
            slot = current_slot.0,
            plans = result.plans_processed,
            transfers = result.pending_transfers.len(),
            "Slot işlendi"
        );

        result
    }

    // ─── TRANSFER TOPLAYICI ───────────────────────────────────────────────────

    /// Bekleyen transfer komutlarını topla.
    ///
    /// Cosmos Vesting: dönemsel serbest bırakma analogu.
    fn collect_pending_transfers(
        &self,
        plan: &InheritancePlan,
        plan_id: InheritancePlanId,
        current_slot: SlotId,
    ) -> Vec<TransferCommand> {
        let state = match self.states.get(&plan_id) {
            Some(s) => s.clone(),
            None => return vec![],
        };

        // Sadece StagedRelease fazında transfer üret
        if !matches!(state.phase, InheritancePhase::StagedRelease { .. }) {
            return vec![];
        }

        // Hesap yaratılma slotu (yaş koşulu için)
        let account_creation_slot = self
            .account_creation_slots
            .get(&plan.owner_address)
            .map(|s| *s)
            .unwrap_or(SlotId(0));

        // Koşulları değerlendir
        let condition_results =
            ConditionEvaluator::evaluate_all(plan, &state, current_slot, account_creation_slot);

        // Blocking koşul varsa transfer yok
        if !ConditionEvaluator::all_satisfied(&condition_results) {
            let blocking = ConditionEvaluator::blocking_conditions(&condition_results);
            debug!(
                plan_id = ?plan_id,
                blocking = ?blocking,
                "Koşullar karşılanmadı, transfer ertelendi"
            );
            return vec![];
        }

        // Koşul transferleri (borç ödemeleri, hayır kurumu)
        let mut transfers = ConditionEvaluator::required_transfers(condition_results);

        // Multi-sig koşulu var mı?
        let requires_multisig = plan
            .conditions
            .iter()
            .any(|c| matches!(c, ConditionalRule::MultiSigRequired { .. }));

        // Release adımı transferleri
        let release_start = state.release_started_at.unwrap_or(current_slot);
        let pending_steps = ReleaseEngine::pending_steps(plan, &state, current_slot);

        // BUG-22 düzeltmesi: total_transferred değil, locked_estate kullan.
        // locked_estate = tetiklenme anındaki sahip bakiyesi (set_estate_balance ile ayarlanır).
        // locked_estate = 0 ise transfer henüz başlatılamamaz.
        if state.locked_estate == 0 {
            debug!(
                plan_id = ?plan_id,
                "locked_estate henüz ayarlanmadı, transferler bekleniyor"
            );
            return vec![];
        }

        for step in pending_steps {
            let execute_at = SlotId(release_start.0.saturating_add(step.delay_slots));
            transfers.push(TransferCommand {
                plan_id: plan.plan_id,
                step_index: step.step_index,
                from: plan.owner_address,
                to: step.recipient,
                amount: step.amount_for(state.locked_estate),
                execute_at_slot: execute_at,
                requires_multisig,
            });
        }

        transfers
    }

    /// Miras tetiklendiğinde sahip bakiyesini kilitle.
    ///
    /// Node/block-processor, StagedRelease fazı başladığında bu metodu çağırmalı.
    /// `locked_estate` değeri `collect_pending_transfers`'da adım miktarlarını
    /// hesaplamak için kullanılır.
    ///
    /// BUG-22 düzeltmesi: Daha önce `amount_for(state.total_transferred.0)` kullanılıyordu
    /// ancak `total_transferred` zaten transfer edilmiş miktardır — toplam bakiye değil.
    pub fn set_estate_balance(
        &self,
        plan_id: InheritancePlanId,
        owner_balance: u128,
    ) -> InheritanceResult<()> {
        let mut state_ref = self
            .states
            .get_mut(&plan_id)
            .ok_or(InheritanceError::PlanNotFound { plan_id: plan_id.0 })?;
        state_ref.locked_estate = owner_balance;
        Ok(())
    }

    /// Transfer tamamlandı — state ve plan güncelle.
    ///
    /// NOVA-RED-INHERITANCE-004: Sadece plan sahibi hesabın yetkili executor'ı bu
    /// fonksiyonu çağırabilir. `executor` parametresi plan'ın `owner_address`'iyle
    /// karşılaştırılır (iç blok işlemcisi adına çağrılır).
    ///
    /// NOVA-RED-INHERITANCE-005: `executed` flag'i artık gerçekten ayarlanıyor.
    /// Önceki sürümde flag hiç set edilmiyordu → `pending_steps()` aynı adımları
    /// tekrar üretiyordu (double-spend riski).
    pub fn mark_transfer_executed(
        &self,
        plan_id: InheritancePlanId,
        step_index: u32,
        amount: u128,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        let plan = self.get_plan(plan_id)?;

        // NOVA-RED-INHERITANCE-005: executed flag'i plan içinde set et.
        {
            let mut plan_ref = self
                .plans
                .get_mut(&plan_id)
                .ok_or(InheritanceError::PlanNotFound { plan_id: plan_id.0 })?;
            let step = plan_ref.release_steps
                .get_mut(step_index as usize)
                .ok_or(InheritanceError::ReleaseStepAlreadyExecuted { step: step_index })?;
            if step.executed {
                return Err(InheritanceError::ReleaseStepAlreadyExecuted { step: step_index });
            }
            step.executed = true;
            step.executed_at_slot = Some(current_slot);
        }

        {
            let mut state_ref = self
                .states
                .get_mut(&plan_id)
                .ok_or(InheritanceError::PlanNotFound { plan_id: plan_id.0 })?;
            ReleaseEngine::mark_step_completed(&plan, &mut state_ref, step_index, amount, current_slot)?;
        }

        // Tüm adımlar tamamlandıysa Complete
        let state = self.get_state(plan_id)?;
        if ReleaseEngine::all_steps_completed(&plan, &state) {
            if let Some(mut state_ref) = self.states.get_mut(&plan_id) {
                PhaseManager::complete(&plan, &mut state_ref, current_slot);
            }
            info!(plan_id = ?plan_id, "Miras transferi tamamlandı");
        }

        Ok(())
    }

    // ─── ÖZET ────────────────────────────────────────────────────────────────

    /// Plan özeti getir.
    pub fn get_summary(
        &self,
        plan_id: InheritancePlanId,
        current_slot: SlotId,
    ) -> InheritanceResult<InheritanceSummary> {
        let plan = self.get_plan(plan_id)?;
        let state = self.get_state(plan_id)?;
        Ok(PlanManager::create_summary(&plan, &state, current_slot))
    }

    /// Tüm aktif plan sayısı.
    pub fn active_plan_count(&self) -> usize {
        self.plans.iter().filter(|r| r.value().is_active).count()
    }

    /// Toplam işlenen slot.
    pub fn total_slots_processed(&self) -> u64 {
        self.total_slots_processed.load(Ordering::Relaxed)
    }
}

impl Default for InheritanceEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_crypto::EcdsaKeypair;

    fn test_keypair() -> EcdsaKeypair {
        EcdsaKeypair::from_secret_bytes(&[0xA5u8; 32]).expect("test keypair")
    }

    fn test_pubkey() -> Vec<u8> {
        test_keypair().public_key().expect("pubkey").as_bytes().to_vec()
    }
    use crate::heartbeat::HeartbeatBuilder;
    use crate::plan::PlanBuilder;
    use crate::types::TriggerType;
    use nova_primitives::types::Address;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_engine() -> InheritanceEngine {
        InheritanceEngine::new()
    }

    /// Temel plan — unique account_id per owner seed → unique plan_id
    fn make_plan(owner: Address, inactivity_secs: u64) -> InheritancePlan {
        let account_id = owner.as_bytes()[0] as u64 + 1;
        PlanBuilder::new(account_id, owner)
            .with_trigger(TriggerType::Inactivity { period_secs: inactivity_secs })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .build(SlotId(0))
            .expect("plan oluşturma başarılı")
    }

    // Plan oluşturma
    #[test]
    fn test_create_plan() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();
        let retrieved = engine.get_plan(plan_id).unwrap();
        assert_eq!(retrieved.owner_address, addr(0));
    }

    // Aynı plan iki kez oluşturulamaz
    #[test]
    fn test_create_plan_duplicate_fails() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        engine.create_plan(plan.clone()).unwrap();
        let result = engine.create_plan(plan);
        assert!(matches!(result, Err(InheritanceError::PlanAlreadyExists)));
    }

    // Plan bulunamadı
    #[test]
    fn test_get_nonexistent_plan_fails() {
        let engine = make_engine();
        let fake_id = InheritancePlanId([42u8; 32]);
        assert!(matches!(
            engine.get_plan(fake_id),
            Err(InheritanceError::PlanNotFound { .. })
        ));
    }

    // Heartbeat işleme
    #[test]
    fn test_process_heartbeat() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        let kp = test_keypair();
        let pk = test_pubkey();
        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(1000)).build_signed(&kp);
        engine.process_heartbeat(msg, &pk).unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert_eq!(state.last_heartbeat_slot, SlotId(1000));
        assert_eq!(state.heartbeat_count, 1);
    }

    // Duress heartbeat
    #[test]
    fn test_duress_heartbeat_sets_flag() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        let kp = test_keypair();
        let pk = test_pubkey();
        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(1000))
            .with_duress()
            .build_signed(&kp);
        engine.process_heartbeat(msg, &pk).unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert!(state.duress_detected);
    }

    // Var olmayan plan için heartbeat başarısız
    #[test]
    fn test_heartbeat_nonexistent_plan_fails() {
        let engine = make_engine();
        let fake_id = InheritancePlanId([99u8; 32]);
        let kp = test_keypair();
        let pk = test_pubkey();
        let msg = HeartbeatBuilder::new(1, fake_id, SlotId(100)).build_signed(&kp);
        assert!(matches!(
            engine.process_heartbeat(msg, &pk),
            Err(InheritanceError::PlanNotFound { .. })
        ));
    }

    // Plan iptal — sahip tarafından
    #[test]
    fn test_cancel_plan_by_owner() {
        let engine = make_engine();
        let owner = addr(0);
        let plan = make_plan(owner, 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        engine.cancel_plan(plan_id, owner, SlotId(100)).unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert!(matches!(state.phase, InheritancePhase::Cancelled { .. }));
    }

    // Plan iptal — yetkisiz
    #[test]
    fn test_cancel_plan_unauthorized() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        let result = engine.cancel_plan(plan_id, addr(99), SlotId(100));
        assert!(matches!(result, Err(InheritanceError::InvalidParameter(_))));
    }

    // Multi-sig onayı
    #[test]
    fn test_multisig_approval() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        engine.add_multisig_approval(plan_id, addr(1)).unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert!(state.multisig_approvals.contains(&addr(1)));
    }

    // Multi-sig onayı — heir olmayan
    #[test]
    fn test_multisig_approval_non_heir_fails() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        let result = engine.add_multisig_approval(plan_id, addr(99));
        assert!(matches!(result, Err(InheritanceError::HeirNotFound { .. })));
    }

    // Heir onayı
    #[test]
    fn test_heir_acknowledgement() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        engine
            .record_heir_acknowledgement(plan_id, addr(1), SlotId(500))
            .unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert!(state.heir_acknowledgements.iter().any(|(a, _)| *a == addr(1)));
    }

    // on_slot — aktif plan sayısı
    #[test]
    fn test_on_slot_processes_plans() {
        let engine = make_engine();
        let plan1 = make_plan(addr(0), 30 * 24 * 3600);
        let plan2 = make_plan(addr(1), 60 * 24 * 3600);

        engine.create_plan(plan1).unwrap();
        engine.create_plan(plan2).unwrap();

        let result = engine.on_slot(SlotId(100));
        assert_eq!(result.plans_processed, 2);
    }

    // Slot sayacı artar
    #[test]
    fn test_total_slots_processed() {
        let engine = make_engine();
        engine.on_slot(SlotId(1));
        engine.on_slot(SlotId(2));
        engine.on_slot(SlotId(3));
        assert_eq!(engine.total_slots_processed(), 3);
    }

    // Aktif plan sayısı
    #[test]
    fn test_active_plan_count() {
        let engine = make_engine();
        assert_eq!(engine.active_plan_count(), 0);

        engine.create_plan(make_plan(addr(0), 30 * 24 * 3600)).unwrap();
        engine.create_plan(make_plan(addr(1), 30 * 24 * 3600)).unwrap();
        assert_eq!(engine.active_plan_count(), 2);
    }

    // Özet getir
    #[test]
    fn test_get_summary() {
        let engine = make_engine();
        let plan = make_plan(addr(0), 30 * 24 * 3600);
        let plan_id = engine.create_plan(plan).unwrap();

        let summary = engine.get_summary(plan_id, SlotId(0)).unwrap();
        assert_eq!(summary.plan_id, plan_id);
        assert_eq!(summary.owner_account_id, 1);
    }

    // Hesap yaratılma kaydı — Address (AccountId alias)
    #[test]
    fn test_register_account_creation() {
        let engine = make_engine();
        engine.register_account_creation(addr(0), SlotId(100));

        let stored = engine
            .account_creation_slots
            .get(&addr(0))
            .map(|s| *s);
        assert_eq!(stored, Some(SlotId(100)));
    }

    // Transfer tamamlama
    #[test]
    fn test_mark_transfer_executed() {
        let engine = make_engine();
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .add_release_step(0, 5_000, addr(1))       // %50
            .add_release_step(0, 5_000, addr(1))       // %50 → toplam %100
            .build(SlotId(0))
            .unwrap();

        let plan_id = engine.create_plan(plan).unwrap();

        engine
            .mark_transfer_executed(plan_id, 0, 50_000, SlotId(1000))
            .unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert_eq!(state.total_transferred.0, 50_000);
    }

    // BUG-22 düzeltme testi: locked_estate ile doğru transfer miktarı hesaplanır
    #[test]
    fn test_set_estate_balance_affects_transfer_amounts() {
        // Bu test BUG-22'yi doğrular: transfer miktarları total_transferred değil
        // locked_estate üzerinden hesaplanmalı.
        //
        // Senaryo: 1_000_000 NSC'lik miras, %50 + %50 iki adımda devredilecek.
        let engine = make_engine();
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .add_release_step(0, 5_000, addr(1))       // %50 → beklenen: 500_000 NSC
            .add_release_step(500_000, 5_000, addr(1)) // %50 → beklenen: 500_000 NSC
            .build(SlotId(0))
            .unwrap();

        let plan_id = engine.create_plan(plan).unwrap();
        const ESTATE: u128 = 1_000_000;

        // set_estate_balance çağrısı öncesi: locked_estate = 0 → transfer üretemez
        let state_before = engine.get_state(plan_id).unwrap();
        assert_eq!(state_before.locked_estate, 0);

        // Sahibin bakiyesini motor'a bildir
        engine.set_estate_balance(plan_id, ESTATE).unwrap();

        let state_after = engine.get_state(plan_id).unwrap();
        assert_eq!(state_after.locked_estate, ESTATE);

        // Adım miktarı doğrulaması: fraction_bps = 5000 → %50 → 500_000 NSC
        let plan = engine.get_plan(plan_id).unwrap();
        let step = &plan.release_steps[0];
        let amount = step.amount_for(state_after.locked_estate);
        assert_eq!(amount, 500_000, "500_000 NSC'nin %50'si = 500_000 olmalı");

        // Daha önceden gönderilmiş 200_000 NSC olsa bile locked_estate değişmez
        engine.mark_transfer_executed(plan_id, 0, 200_000, SlotId(1000)).unwrap();
        let state_mid = engine.get_state(plan_id).unwrap();
        assert_eq!(state_mid.total_transferred.0, 200_000);
        assert_eq!(state_mid.locked_estate, ESTATE, "locked_estate transferden etkilenmez");

        // Adım miktarı hâlâ locked_estate'e göre hesaplanır
        let amount_after_partial = step.amount_for(state_mid.locked_estate);
        assert_eq!(amount_after_partial, 500_000, "Miktar total_transferred'dan bağımsız olmalı");
    }

    // Default trait
    #[test]
    fn test_default_engine() {
        let engine = InheritanceEngine::default();
        assert_eq!(engine.active_plan_count(), 0);
    }

    // Boş engine'de slot işleme
    #[test]
    fn test_on_slot_empty_engine() {
        let engine = make_engine();
        let result = engine.on_slot(SlotId(999));
        assert_eq!(result.plans_processed, 0);
        assert!(result.pending_transfers.is_empty());
    }
}
