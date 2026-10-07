//! Mock saat — deterministik zaman simülasyonu

use parking_lot::Mutex;
use std::sync::Arc;

/// Mock saat — slot zamanlaması test için
#[derive(Clone)]
pub struct MockClock {
    inner: Arc<Mutex<ClockState>>,
}

struct ClockState {
    current_slot: u64,
    slot_duration_ms: u64,
    elapsed_ms: u64,
}

impl MockClock {
    pub fn new(slot_duration_ms: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ClockState {
                current_slot: 0,
                slot_duration_ms,
                elapsed_ms: 0,
            })),
        }
    }

    /// 400ms slot ile Nova Shield varsayılan saati
    pub fn nova_default() -> Self {
        Self::new(400)
    }

    /// Belirtilen ms ilerle
    pub fn advance_ms(&self, ms: u64) {
        let mut state = self.inner.lock();
        state.elapsed_ms += ms;
        state.current_slot = state.elapsed_ms / state.slot_duration_ms;
    }

    /// N slot ilerle
    pub fn advance_slots(&self, slots: u64) {
        let duration = {
            let state = self.inner.lock();
            state.slot_duration_ms
        };
        self.advance_ms(slots * duration);
    }

    pub fn current_slot(&self) -> u64 {
        self.inner.lock().current_slot
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.inner.lock().elapsed_ms
    }

    pub fn slot_duration_ms(&self) -> u64 {
        self.inner.lock().slot_duration_ms
    }

    /// Saat sıfırla
    pub fn reset(&self) {
        let mut state = self.inner.lock();
        state.current_slot = 0;
        state.elapsed_ms = 0;
    }

    /// Belirli bir slota git
    pub fn set_slot(&self, slot: u64) {
        let mut state = self.inner.lock();
        state.current_slot = slot;
        state.elapsed_ms = slot * state.slot_duration_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_clock_initial() {
        let clock = MockClock::nova_default();
        assert_eq!(clock.current_slot(), 0);
        assert_eq!(clock.elapsed_ms(), 0);
    }

    #[test]
    fn test_mock_clock_advance_slots() {
        let clock = MockClock::nova_default();
        clock.advance_slots(10);
        assert_eq!(clock.current_slot(), 10);
        assert_eq!(clock.elapsed_ms(), 4_000);
    }

    #[test]
    fn test_mock_clock_advance_ms_partial_slot() {
        let clock = MockClock::nova_default();
        clock.advance_ms(600); // 1.5 slot
        assert_eq!(clock.current_slot(), 1);
    }

    #[test]
    fn test_mock_clock_reset() {
        let clock = MockClock::nova_default();
        clock.advance_slots(100);
        clock.reset();
        assert_eq!(clock.current_slot(), 0);
    }

    #[test]
    fn test_mock_clock_set_slot() {
        let clock = MockClock::nova_default();
        clock.set_slot(500);
        assert_eq!(clock.current_slot(), 500);
        assert_eq!(clock.elapsed_ms(), 200_000);
    }

    #[test]
    fn test_mock_clock_clone_shared_state() {
        let clock = MockClock::nova_default();
        let clock2 = clock.clone();
        clock.advance_slots(5);
        assert_eq!(clock2.current_slot(), 5); // paylaşılan durum
    }
}
