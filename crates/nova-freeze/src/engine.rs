//! FreezeEngine — Ana orkestrasyon katmanı
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing `Keeper { storeService, sk }`:
//!   Sub-keeper pattern. FreezeEngine, guardian_registry + anomaly_detector +
//!   window_manager alt sistemlerini orkestre eder.
//! - Avalanchego `snow/engine/snowman/engine.go`:
//!   `on_initialize(context, lastAcceptedID)` → `process_slot(current_slot)` kalıbı.
//! - Polkadot Scheduler `on_initialize(now)`:
//!   Her blok başında çalışan periyodik görev. Bizde: `process_slot()`.
//! - nova-inheritance `InheritanceEngine`:
//!   `DashMap<Address, State>` + `Arc<Mutex<Tracker>>` concurrent storage pattern.
//! - Avalanchego common/halter `Halter { halted: AtomicU32 }`:
//!   Thread-safe atomic flag. Bizde: `total_slots_processed: AtomicU64`.
//!
//! İşlem akışı:
//! ```text
//! freeze_user()   ──► TriggerEvaluator  ──► FreezeState güncelle ──► AuditLog ekle
//! freeze_guardian()──► GuardianRegistry ──► TriggerEvaluator     ──► WindowManager
//! notify_anomaly() ──► PendingAnomaly   ──► confirm_slots sonra  ──► freeze_anomaly()
//! process_slot()  ──► PendingAnomaly?  ──► WindowManager expired ──► Inheritance handoff
//! unfreeze()      ──► UnfreezeProcessor──► FreezeState sıfırla   ──► AuditLog güncelle
//! check_operation()──► is_frozen?       ──► REJECT/ALLOW
//! ```

use crate::anomaly::{AnomalyDetector, TxRecord};
use crate::errors::{FreezeError, FreezeResult};
use crate::guardian::{FreezeAuthorizedGuardians, GuardianFreezeRegistry};
use crate::trigger::TriggerEvaluator;
use crate::types::{
    AnomalyType, FreezeAuditEntry, FreezeConfig, FreezeEvent, FreezeEventType, FreezeId,
    FreezeOutcomeRecord, FreezePhase, FreezeRecord, FreezeState, FreezeSummary, OperationType,
    PendingAnomaly, ThresholdProof, UnfreezeOutcome, freeze_id_to_hex,
};
use crate::unfreeze::UnfreezeProcessor;
use crate::window::WindowManager;
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

// ─── FreezeEngine ─────────────────────────────────────────────────────────────

/// Nova Shield Chain Emergency Freeze Protocol ana motoru.
///
/// Thread-safe: tüm dahili state DashMap/AtomicU64 ile korunur.
/// Kilitsiz: sık erişilen path'lerde Mutex kullanılmaz.
pub struct FreezeEngine {
    /// Her hesap için freeze durumu.
    freeze_states: Arc<DashMap<Address, FreezeState>>,
    /// Guardian yetkilendirme ve ihlal kayıtları.
    guardian_registry: Arc<GuardianFreezeRegistry>,
    /// Davranış anomali dedektörü.
    anomaly_detector: Arc<AnomalyDetector>,
    /// Soruşturma penceresi yöneticisi.
    window_manager: Arc<WindowManager>,
    /// Değiştirilemez denetim logu.
    audit_log: Arc<Mutex<VecDeque<FreezeAuditEntry>>>,
    /// Onay bekleyen anomaliler (hesap → PendingAnomaly).
    anomaly_pending: Arc<DashMap<Address, PendingAnomaly>>,
    /// Engine yapılandırması.
    config: FreezeConfig,
    /// Toplam işlenen slot sayısı (Avalanchego Halter atomic flag pattern).
    total_slots_processed: AtomicU64,
    /// Audit log sıra numarası sayacı.
    seq_counter: AtomicU64,
    /// Toplam freeze sayacı (istatistik).
    total_freeze_events: AtomicU64,
    /// Toplam yanlış freeze sayacı.
    total_wrong_freezes: AtomicU64,
}

impl FreezeEngine {
    /// Yeni FreezeEngine oluştur.
    pub fn new(config: FreezeConfig) -> Self {
        Self {
            freeze_states: Arc::new(DashMap::new()),
            guardian_registry: Arc::new(GuardianFreezeRegistry::new()),
            anomaly_detector: Arc::new(AnomalyDetector::new()),
            window_manager: Arc::new(WindowManager::new()),
            audit_log: Arc::new(Mutex::new(VecDeque::new())),
            anomaly_pending: Arc::new(DashMap::new()),
            config,
            total_slots_processed: AtomicU64::new(0),
            seq_counter: AtomicU64::new(0),
            total_freeze_events: AtomicU64::new(0),
            total_wrong_freezes: AtomicU64::new(0),
        }
    }

    /// Varsayılan yapılandırmayla oluştur.
    pub fn with_defaults() -> Self {
        Self::new(FreezeConfig::default())
    }

    // ─── Freeze İşlemleri ────────────────────────────────────────────────────

    /// Kullanıcı kendi hesabını dondurur — tek imza yeterli.
    /// §5.5.2 Tetikleyici 1.
    pub fn freeze_user(
        &self,
        account: Address,
        initiator: Address,
        slot: SlotId,
        timestamp_secs: u64,
    ) -> FreezeResult<FreezeId> {
        // Zaten dondurulmuş mu?
        if self.is_frozen(&account) {
            return Err(FreezeError::AlreadyFrozen);
        }

        let record = TriggerEvaluator::evaluate_user_freeze(account, initiator, slot, timestamp_secs)?;
        let freeze_id = record.freeze_id;

        self.apply_freeze(record, slot)?;

        // Kullanıcı freeze → hemen soruşturma penceresi aç
        self.window_manager.open_investigation_window(account, freeze_id, slot);

        info!(
            target: "nova_freeze",
            account = %hex::encode(account.as_bytes()),
            freeze_id = %freeze_id_to_hex(&freeze_id),
            slot = slot.0,
            "kullanıcı freeze uygulandı"
        );

        Ok(freeze_id)
    }

    /// Guardian threshold ile hesabı dondurur.
    /// §5.5.2 Tetikleyici 2.
    pub fn freeze_guardian(
        &self,
        account: Address,
        signers: Vec<Address>,
        slot: SlotId,
        timestamp_secs: u64,
    ) -> FreezeResult<FreezeId> {
        if self.is_frozen(&account) {
            return Err(FreezeError::AlreadyFrozen);
        }

        let authorized = self
            .guardian_registry
            .get_authorized_guardians(&account)
            .ok_or(FreezeError::NoAuthorizedGuardians)?;

        let registry = &self.guardian_registry;
        let jailed_check = |a: &Address| registry.is_guardian_jailed(a, slot);

        let record = TriggerEvaluator::evaluate_guardian_threshold(
            account,
            signers,
            &authorized,
            jailed_check,
            slot,
            timestamp_secs,
        )?;
        let freeze_id = record.freeze_id;

        self.apply_freeze(record, slot)?;

        // Guardian freeze → 24 saatlik kullanıcı iptal penceresi aç
        self.window_manager.open_user_cancellation_window(account, freeze_id, slot);

        info!(
            target: "nova_freeze",
            account = %hex::encode(account.as_bytes()),
            freeze_id = %freeze_id_to_hex(&freeze_id),
            slot = slot.0,
            "guardian threshold freeze uygulandı"
        );

        Ok(freeze_id)
    }

    /// Anomali bildirimi — onay süreci başlatır.
    /// §5.5.2 Tetikleyici 3 ilk aşama: "Kullanıcıya bildirim → 10 dakika içinde
    /// onay gelmezse geçici dondurma."
    pub fn notify_anomaly(
        &self,
        account: Address,
        score_bps: u32,
        anomaly_type: AnomalyType,
        slot: SlotId,
    ) -> FreezeResult<()> {
        if score_bps < self.config.anomaly_threshold_bps {
            return Err(FreezeError::AnomalyScoreTooLow {
                score_bps,
                threshold_bps: self.config.anomaly_threshold_bps,
            });
        }

        let confirm_deadline = SlotId(slot.0 + self.config.anomaly_confirm_slots);
        self.anomaly_pending.insert(
            account,
            PendingAnomaly {
                score_bps,
                anomaly_type: anomaly_type.clone(),
                detected_at_slot: slot,
                confirm_deadline_slot: confirm_deadline,
            },
        );

        debug!(
            target: "nova_freeze",
            account = %hex::encode(account.as_bytes()),
            score_bps = score_bps,
            slot = slot.0,
            "anomali tespit edildi, onay bekleniyor"
        );

        Ok(())
    }

    // ─── Unfreeze İşlemleri ───────────────────────────────────────────────────

    /// Hesabı dondurulmuş durumdan çıkar.
    /// ThresholdProof ile kimlik kanıtı veya saldırı teyidi.
    pub fn unfreeze(
        &self,
        account: Address,
        proof: ThresholdProof,
        new_guardians: Option<Vec<Address>>,
        slot: SlotId,
    ) -> FreezeResult<UnfreezeOutcome> {
        // Dondurulmuş mu?
        if !self.is_frozen(&account) {
            return Err(FreezeError::NotFrozen);
        }

        let freeze_id = {
            let state = self.freeze_states.get(&account).ok_or(FreezeError::AccountNotFound {
                address: hex::encode(account.as_bytes()),
            })?;
            *state.active_freeze_id().ok_or(FreezeError::NotFrozen)?
        };

        let required_threshold = self
            .guardian_registry
            .get_authorized_guardians(&account)
            .map(|c| c.required_threshold)
            .unwrap_or(self.config.guardian_freeze_threshold);

        let outcome = if let Some(new_g) = new_guardians {
            // Saldırı teyidi
            UnfreezeProcessor::process_attack_confirmed(
                &account, &freeze_id, proof, new_g, required_threshold, slot,
            )?
        } else {
            // Kullanıcı kimlik kanıtı
            UnfreezeProcessor::process_user_unfreeze(
                &account, &freeze_id, proof, required_threshold, slot,
            )?
        };

        self.apply_unfreeze(account, freeze_id, outcome.clone(), false, slot);

        info!(
            target: "nova_freeze",
            account = %hex::encode(account.as_bytes()),
            freeze_id = %freeze_id_to_hex(&freeze_id),
            slot = slot.0,
            "hesap unfreeze edildi"
        );

        Ok(outcome)
    }

    /// Guardian freeze'ini kullanıcı 24 saat içinde iptal eder.
    pub fn cancel_guardian_freeze(
        &self,
        account: Address,
        proof: ThresholdProof,
        slot: SlotId,
    ) -> FreezeResult<()> {
        if !self.is_frozen(&account) {
            return Err(FreezeError::NotFrozen);
        }

        // Sadece guardian freeze iptal edilebilir
        {
            let state = self.freeze_states.get(&account).ok_or(FreezeError::AccountNotFound {
                address: hex::encode(account.as_bytes()),
            })?;
            if !state.active_freeze.as_ref().map_or(false, |r| r.is_guardian_freeze()) {
                return Err(FreezeError::Internal(
                    "sadece guardian freeze iptal edilebilir".to_string(),
                ));
            }
            // Kullanıcı iptal penceresi açık mı?
            if !state.phase.is_cancellation_open(slot) {
                return Err(FreezeError::InvestigationWindowExpired {
                    expired_at_slot: match &state.phase {
                        FreezePhase::UserCancellationWindow { expires_at_slot } => expires_at_slot.0,
                        _ => 0,
                    },
                });
            }
        }

        let freeze_id = {
            let state = self.freeze_states.get(&account).unwrap();
            *state.active_freeze_id().unwrap()
        };

        let outcome = UnfreezeProcessor::process_user_cancellation(
            &account, &freeze_id, proof, slot,
        )?;
        self.apply_unfreeze(account, freeze_id, outcome, false, slot);

        Ok(())
    }

    // ─── İşlem Kontrolü ─────────────────────────────────────────────────────

    /// Hesap dondurulmuş mu?
    /// Hot-path: DashMap get, lock yok.
    /// Pattern: Avalanchego `Halter.Halted()` — atomic check.
    pub fn is_frozen(&self, account: &Address) -> bool {
        self.freeze_states
            .get(account)
            .map_or(false, |s| s.is_frozen)
    }

    /// İşlemi kontrol et: dondurulmuş hesaptan reddedilmeli mi?
    ///
    /// Pattern: go-ethereum txpool `ValidateTransaction()` + `ValidationOptions.Accept` bitmap.
    /// Cheap check first: `is_frozen()` → expensive işleme gerek kalmaz.
    ///
    /// Whitepaper §5.5.3:
    /// - transfer() → REJECT
    /// - swap()     → REJECT
    /// - receive()  → ALLOW
    /// - unfreeze() → ALLOW
    pub fn check_operation(
        &self,
        account: &Address,
        op_type: OperationType,
        _slot: SlotId,
    ) -> FreezeResult<()> {
        if !self.is_frozen(account) {
            return Ok(());
        }

        if op_type.is_allowed_when_frozen() {
            return Ok(());
        }

        // Freeze ID'yi al (hata mesajı için)
        let freeze_id_hex = self
            .freeze_states
            .get(account)
            .and_then(|s| s.active_freeze_id().map(freeze_id_to_hex))
            .unwrap_or_default();

        Err(FreezeError::OperationRejected {
            freeze_id: freeze_id_hex,
        })
    }

    // ─── Yapılandırma ────────────────────────────────────────────────────────

    /// Hesap için freeze yetkili guardian listesini ayarla.
    pub fn set_authorized_guardians(
        &self,
        account: Address,
        config: FreezeAuthorizedGuardians,
        _slot: SlotId,
    ) {
        self.guardian_registry.set_authorized_guardians(config);
        debug!(
            target: "nova_freeze",
            account = %hex::encode(account.as_bytes()),
            "freeze yetkili guardian listesi güncellendi"
        );
    }

    // ─── Davranış Takibi ────────────────────────────────────────────────────

    /// İşlem kaydını anomali dedektörüne bildir.
    pub fn record_transaction(&self, account: Address, record: TxRecord) {
        self.anomaly_detector.record_transaction(account, record);
    }

    // ─── Yanlış Freeze Bildirimi ────────────────────────────────────────────

    /// Bir freeze'in yanlış olduğunu bildir → guardian slash et.
    ///
    /// Pattern: Cosmos SDK slashing `HandleValidatorSignature()`:
    /// ihlal kaydı ekle, eşik aşıldıysa slash + jail.
    pub fn report_wrong_freeze(
        &self,
        freeze_id: FreezeId,
        _reporter: Address,
        slot: SlotId,
    ) -> FreezeResult<Vec<FreezeEvent>> {
        // Audit log'da freeze_id'yi bul
        let initiator = {
            let log = self.audit_log.lock();
            log.iter()
                .find(|e| e.freeze_id == freeze_id)
                .map(|e| e.initiator)
                .ok_or(FreezeError::WrongFreezeReportInvalid)?
        };

        self.total_wrong_freezes.fetch_add(1, Ordering::Relaxed);

        let events = self.guardian_registry.record_wrong_freeze(initiator, freeze_id, slot);

        // Audit log'u güncelle
        {
            let mut log = self.audit_log.lock();
            if let Some(entry) = log.iter_mut().find(|e| e.freeze_id == freeze_id) {
                if let Some(ref mut outcome) = entry.outcome {
                    outcome.was_wrong_freeze = true;
                }
            }
        }

        warn!(
            target: "nova_freeze",
            guardian = %hex::encode(initiator.as_bytes()),
            freeze_id = %freeze_id_to_hex(&freeze_id),
            slot = slot.0,
            "yanlış freeze bildirildi, guardian slashlandı"
        );

        Ok(events)
    }

    // ─── Slot İşleme ─────────────────────────────────────────────────────────

    /// Periyodik slot işleme — her slot'ta çağrılır.
    ///
    /// Polkadot Scheduler `on_initialize(now)` pattern'i:
    /// 1. Onay bekleyen anomalileri kontrol et → süresi doldu mu? → freeze uygula
    /// 2. Soruşturma pencereleri doldu mu? → inheritance handoff
    ///
    /// Pattern: nova-inheritance `PhaseManager::process_slot()`.
    pub fn process_slot(&self, current_slot: SlotId) -> Vec<FreezeEvent> {
        let mut events = Vec::new();
        self.total_slots_processed.fetch_add(1, Ordering::Relaxed);

        // 1. Onay bekleyen anomaliler
        let confirmed_accounts: Vec<(Address, PendingAnomaly)> = self
            .anomaly_pending
            .iter()
            .filter(|entry| entry.is_deadline_passed(current_slot))
            .map(|entry| (*entry.key(), entry.clone()))
            .collect();

        for (account, pending) in confirmed_accounts {
            self.anomaly_pending.remove(&account);

            // Henüz dondurulmamış ise freeze uygula
            if !self.is_frozen(&account) {
                if let Ok(record) = TriggerEvaluator::evaluate_anomaly_trigger(
                    account,
                    pending.score_bps,
                    pending.anomaly_type.clone(),
                    current_slot,
                    current_slot.0 * 400 / 1000, // yaklaşık timestamp
                    self.config.anomaly_threshold_bps,
                ) {
                    let freeze_id = record.freeze_id;
                    if self.apply_freeze(record, current_slot).is_ok() {
                        self.window_manager.open_investigation_window(account, freeze_id, current_slot);
                        events.push(FreezeEvent {
                            account,
                            event_type: FreezeEventType::AnomalyConfirmed {
                                score_bps: pending.score_bps,
                            },
                            slot: current_slot,
                        });
                        events.push(FreezeEvent {
                            account,
                            event_type: FreezeEventType::AccountFrozen {
                                freeze_id,
                                reason_summary: format!("anomali: {} bps", pending.score_bps),
                            },
                            slot: current_slot,
                        });
                    }
                }
            }
        }

        // 2. Soruşturma pencereleri — dolmuşları işle
        let expired_windows = self.window_manager.process_slot(current_slot);
        for expired in expired_windows {
            // Kullanıcı iptal penceresi doldu → tam soruşturma penceresi aç
            use crate::window::WindowType;
            if expired.window_type == WindowType::UserCancellation {
                // Kullanıcı iptal etmedi → tam soruşturma penceresi aç
                if self.is_frozen(&expired.account) {
                    self.window_manager.open_investigation_window(
                        expired.account,
                        expired.freeze_id,
                        current_slot,
                    );
                    events.push(FreezeEvent {
                        account: expired.account,
                        event_type: FreezeEventType::InvestigationWindowOpened {
                            freeze_id: expired.freeze_id,
                            expires_at_slot: SlotId(
                                current_slot.0 + self.config.investigation_window_slots,
                            ),
                        },
                        slot: current_slot,
                    });
                }
            } else {
                // Tam soruşturma penceresi doldu → miras protokolü handoff
                if self.is_frozen(&expired.account) {
                    let outcome = UnfreezeProcessor::process_timeout(current_slot);
                    self.apply_unfreeze(expired.account, expired.freeze_id, outcome, false, current_slot);

                    events.push(FreezeEvent {
                        account: expired.account,
                        event_type: FreezeEventType::InvestigationWindowExpired {
                            freeze_id: expired.freeze_id,
                        },
                        slot: current_slot,
                    });
                }
            }
        }

        events
    }

    // ─── Sorgulama ───────────────────────────────────────────────────────────

    /// Hesap freeze durumunu al.
    pub fn get_freeze_state(&self, account: &Address) -> Option<FreezeState> {
        self.freeze_states.get(account).map(|s| s.clone())
    }

    /// Toplam dondurulmuş hesap sayısı.
    pub fn total_frozen_accounts(&self) -> usize {
        self.freeze_states.iter().filter(|e| e.is_frozen).count()
    }

    /// Audit log son N girişini al.
    pub fn get_audit_log_tail(&self, n: usize) -> Vec<FreezeAuditEntry> {
        let log = self.audit_log.lock();
        log.iter().rev().take(n).cloned().collect()
    }

    /// Engine istatistik özeti.
    pub fn get_summary(&self) -> FreezeSummary {
        FreezeSummary {
            total_frozen_accounts: self.total_frozen_accounts(),
            total_freeze_events: self.total_freeze_events.load(Ordering::Relaxed),
            total_guardian_slashes: 0, // guardian_registry'den hesaplanabilir
            total_wrong_freezes: self.total_wrong_freezes.load(Ordering::Relaxed),
            slots_processed: self.total_slots_processed.load(Ordering::Relaxed),
        }
    }

    // ─── Dahili Yardımcılar ───────────────────────────────────────────────────

    /// Freeze uygula: state'i güncelle, audit log'a ekle.
    fn apply_freeze(&self, record: FreezeRecord, _slot: SlotId) -> FreezeResult<()> {
        let account = record.account;
        let freeze_id = record.freeze_id;
        let initiator = record.initiator;
        let reason = record.reason.clone();
        let slot = record.initiated_at_slot;

        let mut state = self
            .freeze_states
            .entry(account)
            .or_insert_with(|| FreezeState::new(account));
        state.apply_freeze(record);
        drop(state);

        self.total_freeze_events.fetch_add(1, Ordering::Relaxed);

        // Audit log
        let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
        self.append_audit_log(FreezeAuditEntry {
            seq,
            freeze_id,
            account,
            initiator,
            reason,
            slot,
            outcome: None,
        });

        Ok(())
    }

    /// Unfreeze uygula: state'i güncelle, audit log'u tamamla.
    fn apply_unfreeze(
        &self,
        account: Address,
        freeze_id: FreezeId,
        outcome: UnfreezeOutcome,
        was_wrong: bool,
        slot: SlotId,
    ) {
        if let Some(mut state) = self.freeze_states.get_mut(&account) {
            state.resolve(outcome.clone(), slot);
        }

        // Soruşturma penceresini kapat
        self.window_manager.close_window(&account);

        // Audit log'u tamamla
        let mut log = self.audit_log.lock();
        if let Some(entry) = log.iter_mut().find(|e| e.freeze_id == freeze_id) {
            entry.outcome = Some(FreezeOutcomeRecord {
                resolved_at_slot: slot,
                outcome,
                was_wrong_freeze: was_wrong,
            });
        }
    }

    /// Audit log'a giriş ekle, maksimum boyutu koru.
    fn append_audit_log(&self, entry: FreezeAuditEntry) {
        let mut log = self.audit_log.lock();
        if log.len() >= crate::types::MAX_AUDIT_LOG_SIZE {
            log.pop_front(); // En eski girişi çıkar
        }
        log.push_back(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{USER_CANCELLATION_WINDOW_SLOTS, ANOMALY_CONFIRM_SLOTS};

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_engine() -> FreezeEngine {
        FreezeEngine::with_defaults()
    }

    fn make_engine_with_guardians(account: Address, guardians: Vec<Address>, threshold: u8) -> FreezeEngine {
        let engine = FreezeEngine::with_defaults();
        engine.set_authorized_guardians(
            account,
            FreezeAuthorizedGuardians::new(account, guardians, threshold),
            SlotId(0),
        );
        engine
    }

    fn valid_proof(engine: &FreezeEngine, account: Address, slot: SlotId) -> ThresholdProof {
        let state = engine.get_freeze_state(&account).unwrap();
        let freeze_id = *state.active_freeze_id().unwrap();
        ThresholdProof::mock_valid(&account, &freeze_id, vec![addr(10), addr(11)], slot)
    }

    // ─── freeze_user ──────────────────────────────────────────────────────────

    #[test]
    fn test_freeze_user_success() {
        let engine = make_engine();
        let account = addr(1);
        let result = engine.freeze_user(account, addr(1), SlotId(100), 10000);
        assert!(result.is_ok());
        assert!(engine.is_frozen(&account));
    }

    #[test]
    fn test_freeze_user_already_frozen() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(100), 10000).unwrap();
        let err = engine.freeze_user(account, addr(1), SlotId(101), 10001).unwrap_err();
        assert!(matches!(err, FreezeError::AlreadyFrozen));
    }

    #[test]
    fn test_freeze_user_opens_investigation_window() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(100), 10000).unwrap();
        assert!(engine.window_manager.is_open(&account, SlotId(1000)));
    }

    // ─── freeze_guardian ──────────────────────────────────────────────────────

    #[test]
    fn test_freeze_guardian_success() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);

        let result = engine.freeze_guardian(account, vec![g1, g2], SlotId(200), 20000);
        assert!(result.is_ok());
        assert!(engine.is_frozen(&account));
    }

    #[test]
    fn test_freeze_guardian_opens_cancellation_window() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        engine.freeze_guardian(account, vec![g1, g2], SlotId(200), 20000).unwrap();

        // 24 saatlik kullanıcı iptal penceresi açılmalı
        let window = engine.window_manager.get_window(&account).unwrap();
        use crate::window::WindowType;
        assert_eq!(window.window_type, WindowType::UserCancellation);
        assert_eq!(window.expires_at_slot.0, 200 + USER_CANCELLATION_WINDOW_SLOTS);
    }

    #[test]
    fn test_freeze_guardian_no_authorized_list() {
        let engine = make_engine();
        let err = engine.freeze_guardian(addr(1), vec![addr(10)], SlotId(0), 0).unwrap_err();
        assert!(matches!(err, FreezeError::NoAuthorizedGuardians));
    }

    #[test]
    fn test_freeze_guardian_threshold_not_met() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);

        let err = engine.freeze_guardian(account, vec![g1], SlotId(0), 0).unwrap_err();
        assert!(matches!(err, FreezeError::ThresholdNotMet { .. }));
    }

    // ─── check_operation ──────────────────────────────────────────────────────

    #[test]
    fn test_check_operation_not_frozen_allows_all() {
        let engine = make_engine();
        let account = addr(1);
        assert!(engine.check_operation(&account, OperationType::Transfer, SlotId(0)).is_ok());
        assert!(engine.check_operation(&account, OperationType::Swap, SlotId(0)).is_ok());
        assert!(engine.check_operation(&account, OperationType::Receive, SlotId(0)).is_ok());
    }

    #[test]
    fn test_check_operation_frozen_rejects_transfer() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(0), 0).unwrap();

        let err = engine.check_operation(&account, OperationType::Transfer, SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::OperationRejected { .. }));
    }

    #[test]
    fn test_check_operation_frozen_rejects_swap() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(0), 0).unwrap();

        let err = engine.check_operation(&account, OperationType::Swap, SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::OperationRejected { .. }));
    }

    #[test]
    fn test_check_operation_frozen_allows_receive() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(0), 0).unwrap();

        // Whitepaper §5.5.3: receive() → ALLOW
        assert!(engine.check_operation(&account, OperationType::Receive, SlotId(0)).is_ok());
    }

    #[test]
    fn test_check_operation_frozen_allows_unfreeze() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(0), 0).unwrap();

        assert!(engine.check_operation(&account, OperationType::Unfreeze, SlotId(0)).is_ok());
    }

    // ─── unfreeze ─────────────────────────────────────────────────────────────

    #[test]
    fn test_unfreeze_user_identity_success() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        engine.freeze_user(account, addr(1), SlotId(100), 10000).unwrap();

        let freeze_id = *engine.get_freeze_state(&account).unwrap().active_freeze_id().unwrap();
        let proof = ThresholdProof::mock_valid(&account, &freeze_id, vec![g1, g2], SlotId(500));

        let outcome = engine.unfreeze(account, proof, None, SlotId(500)).unwrap();
        assert!(matches!(outcome, UnfreezeOutcome::UserIdentityProven { .. }));
        assert!(!engine.is_frozen(&account));
    }

    #[test]
    fn test_unfreeze_not_frozen_fails() {
        let engine = make_engine();
        let account = addr(1);
        let proof = ThresholdProof::mock_valid(&account, &[0u8; 32], vec![addr(10)], SlotId(0));
        let err = engine.unfreeze(account, proof, None, SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::NotFrozen));
    }

    #[test]
    fn test_unfreeze_attack_confirmed() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        engine.freeze_user(account, addr(1), SlotId(100), 10000).unwrap();

        let freeze_id = *engine.get_freeze_state(&account).unwrap().active_freeze_id().unwrap();
        let proof = ThresholdProof::mock_valid(&account, &freeze_id, vec![g1, g2], SlotId(500));
        let new_guardians = vec![addr(20), addr(21)];

        let outcome = engine.unfreeze(account, proof, Some(new_guardians.clone()), SlotId(500)).unwrap();
        if let UnfreezeOutcome::AttackConfirmed { new_guardian_set, .. } = outcome {
            assert_eq!(new_guardian_set, new_guardians);
        } else {
            panic!("AttackConfirmed beklendi");
        }
    }

    // ─── cancel_guardian_freeze ───────────────────────────────────────────────

    #[test]
    fn test_cancel_guardian_freeze_success() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        engine.freeze_guardian(account, vec![g1, g2], SlotId(0), 0).unwrap();

        let freeze_id = *engine.get_freeze_state(&account).unwrap().active_freeze_id().unwrap();
        let proof = ThresholdProof::mock_valid(&account, &freeze_id, vec![addr(1)], SlotId(100));

        let result = engine.cancel_guardian_freeze(account, proof, SlotId(100));
        assert!(result.is_ok());
        assert!(!engine.is_frozen(&account));
    }

    #[test]
    fn test_cancel_guardian_freeze_expired_window() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        engine.freeze_guardian(account, vec![g1, g2], SlotId(0), 0).unwrap();

        let freeze_id = *engine.get_freeze_state(&account).unwrap().active_freeze_id().unwrap();
        let proof = ThresholdProof::mock_valid(&account, &freeze_id, vec![addr(1)], SlotId(0));

        // 24 saat sonrasına atla
        let err = engine
            .cancel_guardian_freeze(account, proof, SlotId(USER_CANCELLATION_WINDOW_SLOTS + 1))
            .unwrap_err();
        assert!(matches!(err, FreezeError::InvestigationWindowExpired { .. }));
    }

    // ─── notify_anomaly + process_slot ────────────────────────────────────────

    #[test]
    fn test_anomaly_notification_pending() {
        let engine = make_engine();
        let account = addr(1);
        let result = engine.notify_anomaly(account, 8_000, AnomalyType::RapidMultipleWithdrawals, SlotId(0));
        assert!(result.is_ok());
        assert!(engine.anomaly_pending.contains_key(&account));
        assert!(!engine.is_frozen(&account), "henüz dondurulmadı");
    }

    #[test]
    fn test_anomaly_score_too_low_rejected() {
        let engine = make_engine();
        let err = engine
            .notify_anomaly(addr(1), 3_000, AnomalyType::UnusualTransferVolume, SlotId(0))
            .unwrap_err();
        assert!(matches!(err, FreezeError::AnomalyScoreTooLow { .. }));
    }

    #[test]
    fn test_process_slot_applies_confirmed_anomaly() {
        let engine = make_engine();
        let account = addr(1);
        let slot = SlotId(0);

        engine.notify_anomaly(account, 8_000, AnomalyType::RapidMultipleWithdrawals, slot).unwrap();
        assert!(!engine.is_frozen(&account));

        // Onay süresi geçince process_slot freeze uygular
        let confirm_slot = SlotId(ANOMALY_CONFIRM_SLOTS);
        let events = engine.process_slot(confirm_slot);
        assert!(engine.is_frozen(&account), "anomali onaylandıktan sonra freeze edilmeli");

        let has_frozen_event = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::AccountFrozen { .. })
        });
        assert!(has_frozen_event);
    }

    #[test]
    fn test_process_slot_investigation_window_expired_handoff() {
        let engine = make_engine();
        let account = addr(1);
        engine.freeze_user(account, addr(1), SlotId(0), 0).unwrap();

        // 72 saat sonrasına atla
        use crate::types::INVESTIGATION_WINDOW_SLOTS;
        let events = engine.process_slot(SlotId(INVESTIGATION_WINDOW_SLOTS));

        assert!(!engine.is_frozen(&account), "72 saat sonra otomatik unfreeze + inheritance handoff");
        let has_expired = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::InvestigationWindowExpired { .. })
        });
        assert!(has_expired);
    }

    // ─── report_wrong_freeze ─────────────────────────────────────────────────

    #[test]
    fn test_report_wrong_freeze_slashes_guardian() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = make_engine_with_guardians(account, vec![g1, g2], 2);
        let freeze_id = engine.freeze_guardian(account, vec![g1, g2], SlotId(0), 0).unwrap();

        let events = engine.report_wrong_freeze(freeze_id, addr(99), SlotId(100)).unwrap();
        let has_slash = events.iter().any(|e| matches!(e.event_type, FreezeEventType::GuardianSlashed { .. }));
        assert!(has_slash);
    }

    #[test]
    fn test_report_wrong_freeze_not_found() {
        let engine = make_engine();
        let err = engine.report_wrong_freeze([0xAB; 32], addr(1), SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::WrongFreezeReportInvalid));
    }

    // ─── get_summary ──────────────────────────────────────────────────────────

    #[test]
    fn test_get_summary() {
        let engine = make_engine();
        engine.freeze_user(addr(1), addr(1), SlotId(0), 0).unwrap();
        engine.freeze_user(addr(2), addr(2), SlotId(0), 0).unwrap();
        engine.process_slot(SlotId(1));

        let summary = engine.get_summary();
        assert_eq!(summary.total_frozen_accounts, 2);
        assert_eq!(summary.total_freeze_events, 2);
        assert_eq!(summary.slots_processed, 1);
    }

    // ─── audit_log ────────────────────────────────────────────────────────────

    #[test]
    fn test_audit_log_records_freeze() {
        let engine = make_engine();
        engine.freeze_user(addr(1), addr(1), SlotId(100), 10000).unwrap();

        let log = engine.get_audit_log_tail(10);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].account, addr(1));
        assert_eq!(log[0].slot, SlotId(100));
    }
}
