//! Otomatik ücret ödeme planlayıcısı (§5.8.4)
//!
//! Yıllık sözleşmeler için slot bazlı periyodik ödeme takibi.
//! Cosmos SDK BeginBlocker pattern'den ilham.

use crate::errors::{GuardianError, GuardianResult};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── PaymentScheduleId ────────────────────────────────────────────────────────

pub type PaymentScheduleId = u64;

// ─── PaymentStatus ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentStatus {
    /// Ödeme bekliyor
    Pending,
    /// Ödeme tamamlandı
    Completed,
    /// Bakiye yetersiz — ödeme başarısız
    Failed,
    /// İptal edildi
    Cancelled,
}

// ─── PaymentSchedule ──────────────────────────────────────────────────────────

/// Otomatik ödeme planı — her slot penceresinde kontrol edilir
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentSchedule {
    pub id: PaymentScheduleId,
    pub payer: Address,
    pub payee: Address,
    /// Her dönem ödeme miktarı (NSC)
    pub amount_per_period: u128,
    /// Dönem uzunluğu (slot) — yıllık = 78_840_000
    pub period_slots: u64,
    /// Son ödeme slot'u
    pub last_paid_slot: u64,
    /// Sonraki ödeme slot'u
    pub next_due_slot: u64,
    pub status: PaymentStatus,
}

impl PaymentSchedule {
    /// Bu slot'ta ödeme yapılmalı mı?
    pub fn is_due(&self, current_slot: SlotId) -> bool {
        self.status == PaymentStatus::Pending && current_slot.0 >= self.next_due_slot
    }

    /// Başarılı ödeme sonrası güncelle
    pub fn advance(&mut self) {
        self.last_paid_slot = self.next_due_slot;
        self.next_due_slot += self.period_slots;
        self.status = PaymentStatus::Completed;
    }

    /// Bir sonraki dönem için sıfırla
    pub fn reset_for_next_period(&mut self) {
        self.status = PaymentStatus::Pending;
    }
}

// ─── PaymentRecord ────────────────────────────────────────────────────────────

/// Tamamlanan ödeme kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub schedule_id: PaymentScheduleId,
    pub payer: Address,
    pub payee: Address,
    pub amount: u128,
    pub paid_at_slot: u64,
    pub success: bool,
}

// ─── AutoPaymentScheduler ─────────────────────────────────────────────────────

/// Cosmos BeginBlocker benzeri slot-tabanlı ödeme yöneticisi
#[derive(Debug, Default)]
pub struct AutoPaymentScheduler {
    schedules: Arc<DashMap<PaymentScheduleId, PaymentSchedule>>,
    records: Arc<DashMap<PaymentScheduleId, Vec<PaymentRecord>>>,
    next_id: Arc<AtomicU64>,
}

impl AutoPaymentScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni ödeme planı oluştur
    pub fn create_schedule(
        &self,
        payer: Address,
        payee: Address,
        amount_per_period: u128,
        period_slots: u64,
        start_slot: SlotId,
    ) -> PaymentScheduleId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let schedule = PaymentSchedule {
            id,
            payer,
            payee,
            amount_per_period,
            period_slots,
            last_paid_slot: 0,
            next_due_slot: start_slot.0 + period_slots,
            status: PaymentStatus::Pending,
        };
        self.schedules.insert(id, schedule);
        self.records.insert(id, Vec::new());
        id
    }

    /// Ödeme planını iptal et
    ///
    /// ## NOVA-RED-GUARDIAN-004 Güvenlik Düzeltmesi
    ///
    /// Yalnızca planın `payer`'ı iptal edebilir.
    /// Başka bir adres çağırırsa `Unauthorized` hatası döner.
    pub fn cancel_schedule(&self, caller: Address, id: PaymentScheduleId) -> GuardianResult<()> {
        let mut entry = self.schedules
            .get_mut(&id)
            .ok_or(GuardianError::ContractNotFound)?;
        // NOVA-RED-GUARDIAN-004: sadece payer iptal edebilir
        if entry.payer != caller {
            return Err(GuardianError::Unauthorized);
        }
        entry.status = PaymentStatus::Cancelled;
        Ok(())
    }

    /// Ödeme planını al
    pub fn get_schedule(&self, id: PaymentScheduleId) -> GuardianResult<PaymentSchedule> {
        self.schedules.get(&id).map(|s| s.clone()).ok_or(GuardianError::ContractNotFound)
    }

    /// Slot geçişinde vadesi gelen ödemeleri işle (BeginBlocker).
    ///
    /// `try_transfer`: payer → payee, amount → başarılı mı? gerçek state değişimini yapar.
    /// Ödeme başarısız olursa `PaymentRecord.success = false` olarak kaydedilir;
    /// plan `Failed` durumuna geçer (bir sonraki dönemde yeniden denenebilir).
    pub fn process_slot<F>(&self, current_slot: SlotId, mut try_transfer: F) -> Vec<PaymentRecord>
    where
        F: FnMut(&Address, &Address, u128) -> bool,
    {
        let mut processed = Vec::new();

        for mut entry in self.schedules.iter_mut() {
            let schedule = entry.value_mut();
            if !schedule.is_due(current_slot) {
                continue;
            }

            let success = try_transfer(&schedule.payer, &schedule.payee, schedule.amount_per_period);

            let record = PaymentRecord {
                schedule_id: schedule.id,
                payer: schedule.payer,
                payee: schedule.payee,
                amount: schedule.amount_per_period,
                paid_at_slot: current_slot.0,
                success,
            };

            if success {
                schedule.advance();
            } else {
                schedule.status = PaymentStatus::Failed;
            }

            if let Some(mut recs) = self.records.get_mut(&schedule.id) {
                recs.push(record.clone());
            }
            processed.push(record);
        }

        processed
    }

    /// Önceki dönem için planı yenile
    pub fn renew_schedule(&self, id: PaymentScheduleId) -> GuardianResult<()> {
        self.schedules
            .get_mut(&id)
            .ok_or(GuardianError::ContractNotFound)
            .map(|mut s| {
                if s.status == PaymentStatus::Completed {
                    s.reset_for_next_period();
                }
            })
    }

    /// Kullanıcının ödeme planları
    pub fn schedules_for_payer(&self, payer: &Address) -> Vec<PaymentSchedule> {
        self.schedules
            .iter()
            .filter(|e| &e.payer == payer && e.status == PaymentStatus::Pending)
            .map(|e| e.clone())
            .collect()
    }

    /// Ödeme geçmişi
    pub fn payment_history(&self, id: PaymentScheduleId) -> Vec<PaymentRecord> {
        self.records.get(&id).map(|r| r.clone()).unwrap_or_default()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    const NSC: u128 = 1_000_000_000_000_000_000;
    const PERIOD: u64 = 78_840_000; // 1 yıl

    #[test]
    fn test_create_schedule() {
        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        let s = scheduler.get_schedule(id).unwrap();
        assert_eq!(s.status, PaymentStatus::Pending);
        assert_eq!(s.next_due_slot, PERIOD);
    }

    #[test]
    fn test_payment_not_due_yet() {
        let scheduler = AutoPaymentScheduler::new();
        scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        let processed = scheduler.process_slot(SlotId(1_000), |_, _, _| true);
        assert!(processed.is_empty());
    }

    #[test]
    fn test_payment_due_and_processed() {
        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        let processed = scheduler.process_slot(SlotId(PERIOD), |_, _, _| true);
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].amount, 50 * NSC);
        assert!(processed[0].success);

        // Ödeme sonrası durum: Completed
        let s = scheduler.get_schedule(id).unwrap();
        assert_eq!(s.status, PaymentStatus::Completed);
        assert_eq!(s.next_due_slot, PERIOD * 2);
    }

    #[test]
    fn test_payment_fails_on_insufficient_balance() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));

        // Gerçek bakiye kontrolü: payer'ın yeterli bakiyesi yok
        let balances: Mutex<HashMap<[u8; 20], u128>> = Mutex::new(HashMap::new());
        balances.lock().unwrap().insert([1u8; 20], 10 * NSC); // yeterli değil

        let processed = scheduler.process_slot(SlotId(PERIOD), |payer, _payee, amount| {
            let mut bal = balances.lock().unwrap();
            let payer_bal = bal.get(payer.as_bytes()).copied().unwrap_or(0);
            if payer_bal < amount {
                return false;
            }
            *bal.get_mut(payer.as_bytes()).unwrap() -= amount;
            true
        });

        assert_eq!(processed.len(), 1);
        assert!(!processed[0].success, "yetersiz bakiye ile ödeme başarısız olmalı");

        // Başarısız ödeme → plan Failed durumuna geçmeli
        let s = scheduler.get_schedule(id).unwrap();
        assert_eq!(s.status, PaymentStatus::Failed);
    }

    #[test]
    fn test_payment_success_with_real_balance_check() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        let scheduler = AutoPaymentScheduler::new();
        scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));

        let balances: Mutex<HashMap<[u8; 20], u128>> = Mutex::new(HashMap::new());
        balances.lock().unwrap().insert([1u8; 20], 200 * NSC);
        balances.lock().unwrap().insert([2u8; 20], 0);

        let processed = scheduler.process_slot(SlotId(PERIOD), |payer, payee, amount| {
            let mut bal = balances.lock().unwrap();
            let payer_bal = *bal.get(payer.as_bytes()).unwrap_or(&0);
            if payer_bal < amount { return false; }
            *bal.entry(*payer.as_bytes()).or_insert(0) -= amount;
            *bal.entry(*payee.as_bytes()).or_insert(0) += amount;
            true
        });

        assert!(processed[0].success);
        let bal = balances.lock().unwrap();
        assert_eq!(*bal.get(&[1u8; 20]).unwrap(), 150 * NSC, "payer bakiyesi azalmış olmalı");
        assert_eq!(*bal.get(&[2u8; 20]).unwrap(), 50 * NSC, "payee bakiyesi artmış olmalı");
    }

    #[test]
    fn test_cancel_schedule() {
        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        // NOVA-RED-GUARDIAN-004: yalnızca payer iptal edebilir
        assert!(scheduler.cancel_schedule(addr(2), id).is_err(), "payee iptal edememeli");
        scheduler.cancel_schedule(addr(1), id).unwrap();
        let s = scheduler.get_schedule(id).unwrap();
        assert_eq!(s.status, PaymentStatus::Cancelled);

        // İptal edilmiş plan işlenmez
        let processed = scheduler.process_slot(SlotId(PERIOD * 2), |_, _, _| true);
        assert!(processed.is_empty());
    }

    #[test]
    fn test_renew_schedule() {
        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        scheduler.process_slot(SlotId(PERIOD), |_, _, _| true);
        // Completed → Pending yenileme
        scheduler.renew_schedule(id).unwrap();
        let s = scheduler.get_schedule(id).unwrap();
        assert_eq!(s.status, PaymentStatus::Pending);
    }

    #[test]
    fn test_payment_history() {
        let scheduler = AutoPaymentScheduler::new();
        let id = scheduler.create_schedule(addr(1), addr(2), 50 * NSC, PERIOD, SlotId(0));
        scheduler.process_slot(SlotId(PERIOD), |_, _, _| true);
        scheduler.renew_schedule(id).unwrap();
        scheduler.process_slot(SlotId(PERIOD * 2), |_, _, _| true);
        let history = scheduler.payment_history(id);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_schedules_for_payer() {
        let scheduler = AutoPaymentScheduler::new();
        scheduler.create_schedule(addr(1), addr(10), 50 * NSC, PERIOD, SlotId(0));
        scheduler.create_schedule(addr(1), addr(20), 30 * NSC, PERIOD, SlotId(0));
        scheduler.create_schedule(addr(2), addr(10), 25 * NSC, PERIOD, SlotId(0)); // farklı
        let plans = scheduler.schedules_for_payer(&addr(1));
        assert_eq!(plans.len(), 2);
    }
}
