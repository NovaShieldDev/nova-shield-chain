//! Guardian servisi — heartbeat izleme ve recovery koordinasyonu

use super::{NodeService, ServiceStatus};
use crate::errors::{NodeError, NodeResult};
use dashmap::DashMap;
use std::sync::Arc;

// ─── GuardianTask ─────────────────────────────────────────────────────────────

/// Guardian'ın izlediği hesap
#[derive(Debug, Clone)]
pub struct MonitoredAccount {
    pub account: [u8; 20],
    pub plan_id: u64,
    /// Son heartbeat slotu
    pub last_heartbeat_slot: u64,
    /// İnaktivite eşiği (slot)
    pub inactivity_threshold_slots: u64,
    /// Uyarı gönderildi mi?
    pub warning_sent: bool,
    /// Recovery başlatıldı mı?
    pub recovery_initiated: bool,
}

impl MonitoredAccount {
    pub fn new(account: [u8; 20], plan_id: u64, threshold_slots: u64, current_slot: u64) -> Self {
        Self {
            account,
            plan_id,
            last_heartbeat_slot: current_slot,
            inactivity_threshold_slots: threshold_slots,
            warning_sent: false,
            recovery_initiated: false,
        }
    }

    /// İnaktif mi?
    pub fn is_inactive(&self, current_slot: u64) -> bool {
        current_slot > self.last_heartbeat_slot + self.inactivity_threshold_slots
    }

    /// Uyarı gerekli mi? (eşiğin %80'inde)
    pub fn needs_warning(&self, current_slot: u64) -> bool {
        let elapsed = current_slot.saturating_sub(self.last_heartbeat_slot);
        elapsed >= self.inactivity_threshold_slots * 4 / 5
    }
}

// ─── GuardianEvent ────────────────────────────────────────────────────────────

/// Guardian olayı
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardianEvent {
    HeartbeatReceived { account: [u8; 20], slot: u64 },
    InactivityWarning { account: [u8; 20], slot: u64 },
    RecoveryInitiated { account: [u8; 20], plan_id: u64, slot: u64 },
}

// ─── GuardianService ──────────────────────────────────────────────────────────

/// Guardian servisi — hesapları izler, recovery tetikler
pub struct GuardianService {
    status: ServiceStatus,
    monitored: Arc<DashMap<[u8; 20], MonitoredAccount>>,
    guardian_id: [u8; 20],
    events: Vec<GuardianEvent>,
    current_slot: u64,
}

impl GuardianService {
    pub fn new(guardian_id: [u8; 20]) -> Self {
        Self {
            status: ServiceStatus::Stopped,
            monitored: Arc::new(DashMap::new()),
            guardian_id,
            events: vec![],
            current_slot: 0,
        }
    }

    /// Hesap izlemeye başla
    pub fn monitor_account(
        &mut self,
        account: [u8; 20],
        plan_id: u64,
        threshold_slots: u64,
    ) -> NodeResult<()> {
        if !self.status.is_running() {
            return Err(NodeError::ServiceStartError {
                service: "GuardianService".into(),
                reason: "servis çalışmıyor".into(),
            });
        }
        let entry = MonitoredAccount::new(account, plan_id, threshold_slots, self.current_slot);
        self.monitored.insert(account, entry);
        Ok(())
    }

    /// Heartbeat işle
    ///
    /// NOVA-RED-NODE-010: caller parametresi eklendi — sadece hesap sahibi kendi heartbeat'ini gönderebilir.
    pub fn process_heartbeat(&mut self, caller: [u8; 20], account: [u8; 20], slot: u64) -> NodeResult<()> {
        if !self.status.is_running() {
            return Err(NodeError::ServiceStartError {
                service: "GuardianService".into(),
                reason: "servis çalışmıyor".into(),
            });
        }
        // Yetki kontrolü: sadece hesap sahibi heartbeat gönderebilir
        if caller != account {
            return Err(NodeError::Unauthorized);
        }
        if let Some(mut entry) = self.monitored.get_mut(&account) {
            entry.last_heartbeat_slot = slot;
            entry.warning_sent = false;
            self.events.push(GuardianEvent::HeartbeatReceived { account, slot });
        }
        Ok(())
    }

    /// Slot ilerlet ve inaktivite kontrolü yap
    pub fn process_slot(&mut self, slot: u64) -> Vec<GuardianEvent> {
        self.current_slot = slot;
        let mut new_events = vec![];

        for mut entry in self.monitored.iter_mut() {
            let acc = *entry.key();

            if !entry.warning_sent && entry.needs_warning(slot) {
                entry.warning_sent = true;
                new_events.push(GuardianEvent::InactivityWarning { account: acc, slot });
            }

            if !entry.recovery_initiated && entry.is_inactive(slot) {
                entry.recovery_initiated = true;
                new_events.push(GuardianEvent::RecoveryInitiated {
                    account: acc,
                    plan_id: entry.plan_id,
                    slot,
                });
            }
        }

        self.events.extend(new_events.clone());
        new_events
    }

    pub fn monitored_count(&self) -> usize { self.monitored.len() }
    pub fn guardian_id(&self) -> [u8; 20] { self.guardian_id }
    pub fn events(&self) -> &[GuardianEvent] { &self.events }
    pub fn current_slot(&self) -> u64 { self.current_slot }
}

impl NodeService for GuardianService {
    fn name(&self) -> &'static str { "GuardianService" }

    fn start(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Starting;
        self.status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Stopping;
        self.monitored.clear();
        self.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> &ServiceStatus { &self.status }

    fn health_check(&self) -> bool {
        self.status.is_running()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn guardian() -> [u8; 20] { [0x42u8; 20] }
    fn account(n: u8) -> [u8; 20] { [n; 20] }

    #[test]
    fn test_guardian_service_lifecycle() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();
        assert!(svc.health_check());
        svc.stop().unwrap();
        assert!(!svc.health_check());
    }

    #[test]
    fn test_guardian_monitor_account() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        svc.monitor_account(account(1), 100, 5000).unwrap();
        assert_eq!(svc.monitored_count(), 1);
    }

    #[test]
    fn test_guardian_heartbeat_resets_inactivity() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        svc.monitor_account(account(1), 1, 1000).unwrap();
        svc.process_heartbeat(account(1), account(1), 500).unwrap();

        // Slot 600 — henüz inaktif değil (son heartbeat 500, threshold 1000)
        let events = svc.process_slot(600);
        assert!(!events.iter().any(|e| matches!(e, GuardianEvent::RecoveryInitiated { .. })));
    }

    #[test]
    fn test_guardian_inactivity_warning() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        // threshold=100 slot, uyarı %80'de = slot 80'de
        svc.monitor_account(account(1), 1, 100).unwrap();

        let events = svc.process_slot(85);
        assert!(events.iter().any(|e| matches!(e, GuardianEvent::InactivityWarning { .. })));
    }

    #[test]
    fn test_guardian_recovery_initiated() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        svc.monitor_account(account(2), 42, 100).unwrap();

        let events = svc.process_slot(110);
        assert!(events.iter().any(|e| matches!(e, GuardianEvent::RecoveryInitiated { plan_id: 42, .. })));
    }

    #[test]
    fn test_guardian_heartbeat_received_event() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        svc.monitor_account(account(3), 1, 1000).unwrap();
        svc.process_heartbeat(account(3), account(3), 50).unwrap();

        assert!(svc.events().iter().any(|e| matches!(e, GuardianEvent::HeartbeatReceived { slot: 50, .. })));
    }

    #[test]
    fn test_guardian_multiple_accounts() {
        let mut svc = GuardianService::new(guardian());
        svc.start().unwrap();

        svc.monitor_account(account(1), 1, 200).unwrap();
        svc.monitor_account(account(2), 2, 300).unwrap();
        svc.monitor_account(account(3), 3, 500).unwrap();

        assert_eq!(svc.monitored_count(), 3);
    }
}
