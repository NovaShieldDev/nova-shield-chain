//! Deadman switch test senaryosu — §5.4

use crate::fixtures::accounts::{alice, bob, charlie};
use crate::mocks::clock::MockClock;

/// Deadman switch senaryo sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadmanOutcome {
    /// Heartbeat başarılı, plan aktif
    HeartbeatAccepted,
    /// İnaktivite uyarısı
    InactivityWarning,
    /// Recovery tetiklendi
    RecoveryTriggered,
    /// Duress heartbeat — gizli uyarı
    DuressDetected,
}

/// Deadman switch test senaryosu
pub struct DeadmanScenario {
    clock: MockClock,
    inactivity_slots: u64,
    warning_threshold_slots: u64,
    last_heartbeat_slot: u64,
    recovery_triggered: bool,
    duress_detected: bool,
}

impl DeadmanScenario {
    /// Varsayılan: 2000 slot inaktivite, %80'de uyarı
    pub fn new(inactivity_slots: u64) -> Self {
        Self {
            clock: MockClock::nova_default(),
            inactivity_slots,
            warning_threshold_slots: inactivity_slots * 4 / 5,
            last_heartbeat_slot: 0,
            recovery_triggered: false,
            duress_detected: false,
        }
    }

    /// Normal heartbeat gönder
    pub fn send_heartbeat(&mut self) -> DeadmanOutcome {
        self.last_heartbeat_slot = self.clock.current_slot();
        DeadmanOutcome::HeartbeatAccepted
    }

    /// Duress heartbeat (baskı altında)
    pub fn send_duress_heartbeat(&mut self) -> DeadmanOutcome {
        self.last_heartbeat_slot = self.clock.current_slot();
        self.duress_detected = true;
        DeadmanOutcome::DuressDetected
    }

    /// Slot ilerlet ve durum kontrol et
    pub fn advance_and_check(&mut self, slots: u64) -> DeadmanOutcome {
        self.clock.advance_slots(slots);
        let elapsed = self.clock.current_slot().saturating_sub(self.last_heartbeat_slot);

        if elapsed >= self.inactivity_slots && !self.recovery_triggered {
            self.recovery_triggered = true;
            return DeadmanOutcome::RecoveryTriggered;
        }
        if elapsed >= self.warning_threshold_slots {
            return DeadmanOutcome::InactivityWarning;
        }
        DeadmanOutcome::HeartbeatAccepted
    }

    pub fn current_slot(&self) -> u64 { self.clock.current_slot() }
    pub fn is_recovery_triggered(&self) -> bool { self.recovery_triggered }
    pub fn is_duress_detected(&self) -> bool { self.duress_detected }
    pub fn elapsed_since_heartbeat(&self) -> u64 {
        self.clock.current_slot().saturating_sub(self.last_heartbeat_slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadman_heartbeat_keeps_plan_active() {
        let mut scenario = DeadmanScenario::new(1000);

        // Normal heartbeat
        let result = scenario.send_heartbeat();
        assert_eq!(result, DeadmanOutcome::HeartbeatAccepted);

        // 500 slot geç — henüz uyarı yok (threshold=800)
        let result = scenario.advance_and_check(500);
        assert_eq!(result, DeadmanOutcome::HeartbeatAccepted);
    }

    #[test]
    fn test_deadman_inactivity_warning() {
        let mut scenario = DeadmanScenario::new(1000);

        // 850 slot geç (threshold=%80=800)
        let result = scenario.advance_and_check(850);
        assert_eq!(result, DeadmanOutcome::InactivityWarning);
    }

    #[test]
    fn test_deadman_recovery_triggered() {
        let mut scenario = DeadmanScenario::new(1000);

        // 1100 slot geç — recovery tetiklenmeli
        let result = scenario.advance_and_check(1100);
        assert_eq!(result, DeadmanOutcome::RecoveryTriggered);
        assert!(scenario.is_recovery_triggered());
    }

    #[test]
    fn test_deadman_heartbeat_resets_timer() {
        let mut scenario = DeadmanScenario::new(1000);

        // 900 slot geç — uyarı
        scenario.advance_and_check(900);

        // Heartbeat gönder
        scenario.send_heartbeat();

        // 500 daha geç — artık uyarı olmamalı (timer sıfırlandı)
        let result = scenario.advance_and_check(500);
        assert_eq!(result, DeadmanOutcome::HeartbeatAccepted);
        assert!(!scenario.is_recovery_triggered());
    }

    #[test]
    fn test_deadman_duress_detected() {
        let mut scenario = DeadmanScenario::new(1000);
        let result = scenario.send_duress_heartbeat();
        assert_eq!(result, DeadmanOutcome::DuressDetected);
        assert!(scenario.is_duress_detected());
    }

    #[test]
    fn test_deadman_multiple_heartbeats() {
        let mut scenario = DeadmanScenario::new(5000);

        for _ in 0..5 {
            scenario.advance_and_check(500); // 500 slot
            scenario.send_heartbeat();
        }

        assert!(!scenario.is_recovery_triggered());
        assert_eq!(scenario.elapsed_since_heartbeat(), 0);
    }

    #[test]
    fn test_deadman_alice_bob_scenario() {
        // Alice'in hesabı, Bob ve Charlie mirasçı
        let _alice = alice();
        let _bob = bob();
        let _charlie = charlie();

        let mut scenario = DeadmanScenario::new(2000);

        // 3 ay (6 milyon ms / 400ms = 15000 slot) boyunca heartbeat yok
        let result = scenario.advance_and_check(2100);
        assert_eq!(result, DeadmanOutcome::RecoveryTriggered);
    }
}
