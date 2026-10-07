//! Miras planı test senaryosu — §5.4 StagedRelease

use crate::fixtures::accounts::{alice, bob, charlie};
use crate::mocks::clock::MockClock;

/// Mirasçı payı
#[derive(Debug, Clone)]
pub struct HeirAllocation {
    pub address: [u8; 20],
    pub name: &'static str,
    pub fraction_bps: u32, // 10000 = 100%
    pub delay_slots: u64,
}

/// Kademeli serbest bırakma durumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseStatus {
    Pending,
    Released { amount: u64 },
    TooEarly { remaining_slots: u64 },
}

/// Miras planı test senaryosu
pub struct InheritanceScenario {
    pub owner: [u8; 20],
    pub total_estate: u64,
    pub heirs: Vec<HeirAllocation>,
    pub trigger_slot: Option<u64>,
    clock: MockClock,
}

impl InheritanceScenario {
    pub fn new(owner: [u8; 20], total_estate: u64) -> Self {
        Self {
            owner,
            total_estate,
            heirs: vec![],
            trigger_slot: None,
            clock: MockClock::nova_default(),
        }
    }

    pub fn add_heir(mut self, address: [u8; 20], name: &'static str, fraction_bps: u32, delay_slots: u64) -> Self {
        self.heirs.push(HeirAllocation { address, name, fraction_bps, delay_slots });
        self
    }

    /// Toplam pay 10000 bps mi?
    pub fn validate_fractions(&self) -> bool {
        let total: u32 = self.heirs.iter().map(|h| h.fraction_bps).sum();
        total == 10_000
    }

    /// Recovery tetikle
    pub fn trigger(&mut self) {
        self.trigger_slot = Some(self.clock.current_slot());
    }

    /// Slot ilerlet
    pub fn advance_slots(&mut self, slots: u64) {
        self.clock.advance_slots(slots);
    }

    /// Belirli mirasçı için serbest bırakma durumu
    pub fn check_release(&self, heir_address: &[u8; 20]) -> ReleaseStatus {
        let trigger = match self.trigger_slot {
            Some(s) => s,
            None => return ReleaseStatus::Pending,
        };

        let heir = match self.heirs.iter().find(|h| &h.address == heir_address) {
            Some(h) => h,
            None => return ReleaseStatus::Pending,
        };

        let current = self.clock.current_slot();
        let release_slot = trigger + heir.delay_slots;

        if current >= release_slot {
            let amount = (self.total_estate as u64 * heir.fraction_bps as u64) / 10_000;
            ReleaseStatus::Released { amount }
        } else {
            ReleaseStatus::TooEarly { remaining_slots: release_slot - current }
        }
    }

    pub fn current_slot(&self) -> u64 { self.clock.current_slot() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scenario() -> InheritanceScenario {
        let alice_acc = alice();
        let bob_acc = bob();
        let charlie_acc = charlie();

        InheritanceScenario::new(alice_acc.address, 1_000_000)
            .add_heir(bob_acc.address, "Bob", 6_000, 100)     // 60%, 100 slot delay
            .add_heir(charlie_acc.address, "Charlie", 4_000, 200) // 40%, 200 slot delay
    }

    #[test]
    fn test_inheritance_valid_fractions() {
        let scenario = make_scenario();
        assert!(scenario.validate_fractions());
    }

    #[test]
    fn test_inheritance_invalid_fractions() {
        let alice_acc = alice();
        let bob_acc = bob();
        let scenario = InheritanceScenario::new(alice_acc.address, 1_000_000)
            .add_heir(bob_acc.address, "Bob", 5_000, 100); // sadece %50
        assert!(!scenario.validate_fractions());
    }

    #[test]
    fn test_inheritance_pending_before_trigger() {
        let scenario = make_scenario();
        let bob_acc = bob();
        assert_eq!(scenario.check_release(&bob_acc.address), ReleaseStatus::Pending);
    }

    #[test]
    fn test_inheritance_too_early_after_trigger() {
        let mut scenario = make_scenario();
        scenario.trigger();
        scenario.advance_slots(50); // 50 slot — bob 100 slot delay bekliyor

        let bob_acc = bob();
        match scenario.check_release(&bob_acc.address) {
            ReleaseStatus::TooEarly { remaining_slots } => {
                assert_eq!(remaining_slots, 50);
            }
            other => panic!("Beklenen TooEarly, alınan {:?}", other),
        }
    }

    #[test]
    fn test_inheritance_bob_released_at_100_slots() {
        let mut scenario = make_scenario();
        scenario.trigger();
        scenario.advance_slots(100);

        let bob_acc = bob();
        match scenario.check_release(&bob_acc.address) {
            ReleaseStatus::Released { amount } => {
                assert_eq!(amount, 600_000); // %60 of 1_000_000
            }
            other => panic!("Beklenen Released, alınan {:?}", other),
        }
    }

    #[test]
    fn test_inheritance_charlie_released_at_200_slots() {
        let mut scenario = make_scenario();
        scenario.trigger();
        scenario.advance_slots(200);

        let charlie_acc = charlie();
        match scenario.check_release(&charlie_acc.address) {
            ReleaseStatus::Released { amount } => {
                assert_eq!(amount, 400_000); // %40 of 1_000_000
            }
            other => panic!("Beklenen Released, alınan {:?}", other),
        }
    }

    #[test]
    fn test_inheritance_charlie_still_pending_at_100_slots() {
        let mut scenario = make_scenario();
        scenario.trigger();
        scenario.advance_slots(150);

        let charlie_acc = charlie();
        // Charlie 200 slot bekliyor, 150 geçti
        assert!(matches!(
            scenario.check_release(&charlie_acc.address),
            ReleaseStatus::TooEarly { .. }
        ));
    }

    #[test]
    fn test_inheritance_staged_release_complete() {
        let mut scenario = make_scenario();
        scenario.trigger();
        scenario.advance_slots(200); // tüm delay'ler geçti

        let bob_acc = bob();
        let charlie_acc = charlie();
        let bob_release = scenario.check_release(&bob_acc.address);
        let charlie_release = scenario.check_release(&charlie_acc.address);

        assert!(matches!(bob_release, ReleaseStatus::Released { amount: 600_000 }));
        assert!(matches!(charlie_release, ReleaseStatus::Released { amount: 400_000 }));
    }
}
