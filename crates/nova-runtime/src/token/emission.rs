//! Emisyon planı — §7.3: %8 → %2 (her yıl %0.5 düşüş)
//!
//! Whitepaper §7.3:
//!   Başlangıç: %8/yıl
//!   Hedef: %2/yıl
//!   Azalma: Her yıl %0.5
//!   Net enflasyon: Yıl 12'de = %0 (yakma ile dengelenir)

use crate::token::nsc::{NSC_DECIMALS, NSC_TOTAL_SUPPLY};

/// Emisyon başlangıç oranı: 800 BPS = %8
pub const INITIAL_EMISSION_BPS: u32 = 800;

/// Minimum emisyon oranı: 200 BPS = %2
pub const MIN_EMISSION_BPS: u32 = 200;

/// Yıllık düşüş: 50 BPS = %0.5
pub const EMISSION_DECAY_PER_YEAR_BPS: u32 = 50;

/// Slot süresi 400ms → 1 yıl ≈ 78,840,000 slot
pub const SLOTS_PER_YEAR: u64 = 78_840_000;

/// Era süresi: 24 saat = 216,000 slot
pub const SLOTS_PER_ERA: u64 = 216_000;

/// Yıl başına era sayısı
pub const ERAS_PER_YEAR: u64 = SLOTS_PER_YEAR / SLOTS_PER_ERA;

/// Era sayısından yıl hesapla
pub fn era_to_year(era: u64) -> u64 {
    era / ERAS_PER_YEAR
}

/// Belirli bir yıl için emisyon oranını BPS olarak döndür.
/// Yıl 0 = 800 BPS, Yıl 1 = 750 BPS, ..., Yıl 12+ = 200 BPS
pub fn emission_rate_bps(year: u64) -> u32 {
    let decay = EMISSION_DECAY_PER_YEAR_BPS * year.min(12) as u32;
    INITIAL_EMISSION_BPS.saturating_sub(decay).max(MIN_EMISSION_BPS)
}

/// Era için toplam emission (yıllık emisyon / yıldaki era sayısı)
pub fn era_emission(total_supply: u128, year: u64) -> u128 {
    let rate_bps = emission_rate_bps(year) as u128;
    let yearly = total_supply * rate_bps / 10_000;
    yearly / ERAS_PER_YEAR as u128
}

/// Validatör başına slot ödülü (era_emission / aktif validator sayısı / slot sayısı)
pub fn slot_reward(era_emission: u128, active_validators: u64, slots_per_era: u64) -> u128 {
    if active_validators == 0 || slots_per_era == 0 {
        return 0;
    }
    era_emission / (active_validators as u128 * slots_per_era as u128)
}

/// EmissionSchedule — era bazlı emisyon takipçisi
#[derive(Debug, Clone)]
pub struct EmissionSchedule {
    current_era: u64,
    total_emitted: u128,
}

impl EmissionSchedule {
    pub fn new() -> Self {
        Self { current_era: 0, total_emitted: 0 }
    }

    pub fn current_era(&self) -> u64 {
        self.current_era
    }

    pub fn total_emitted(&self) -> u128 {
        self.total_emitted
    }

    /// Era geçişi — emisyon miktarını hesapla ve toplam emitted'i güncelle
    pub fn advance_era(&mut self) -> u128 {
        let year = era_to_year(self.current_era);
        let amount = era_emission(NSC_TOTAL_SUPPLY, year);
        self.total_emitted = self.total_emitted.saturating_add(amount);
        self.current_era += 1;
        amount
    }

    /// Belirli bir era'nın yıllık emisyon oranını döndür (BPS)
    pub fn rate_bps_at_era(&self, era: u64) -> u32 {
        emission_rate_bps(era_to_year(era))
    }
}

impl Default for EmissionSchedule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emission_rate_year_0() {
        assert_eq!(emission_rate_bps(0), 800); // %8
    }

    #[test]
    fn test_emission_rate_year_1() {
        assert_eq!(emission_rate_bps(1), 750); // %7.5
    }

    #[test]
    fn test_emission_rate_year_12_floor() {
        assert_eq!(emission_rate_bps(12), 200); // %2 — minimum
        assert_eq!(emission_rate_bps(100), 200); // hep %2
    }

    #[test]
    fn test_emission_rate_decreases_each_year() {
        for y in 0..12u64 {
            assert!(emission_rate_bps(y) > emission_rate_bps(y + 1));
        }
    }

    #[test]
    fn test_era_emission_positive() {
        let em = era_emission(NSC_TOTAL_SUPPLY, 0);
        assert!(em > 0);
    }

    #[test]
    fn test_era_emission_year_12_lower_than_year_0() {
        let y0 = era_emission(NSC_TOTAL_SUPPLY, 0);
        let y12 = era_emission(NSC_TOTAL_SUPPLY, 12);
        assert!(y0 > y12);
    }

    #[test]
    fn test_slot_reward_zero_validators() {
        assert_eq!(slot_reward(1_000 * NSC_DECIMALS, 0, SLOTS_PER_ERA), 0);
    }

    #[test]
    fn test_slot_reward_positive() {
        let em = era_emission(NSC_TOTAL_SUPPLY, 0);
        let reward = slot_reward(em, 200, SLOTS_PER_ERA);
        assert!(reward > 0);
    }

    #[test]
    fn test_emission_schedule_advance() {
        let mut sched = EmissionSchedule::new();
        let era0 = sched.advance_era();
        assert!(era0 > 0);
        assert_eq!(sched.current_era(), 1);
        assert_eq!(sched.total_emitted(), era0);
    }

    #[test]
    fn test_emission_schedule_12_years() {
        let mut sched = EmissionSchedule::new();
        for _ in 0..ERAS_PER_YEAR * 12 {
            sched.advance_era();
        }
        // Yıl 12'de oran %2 olmalı
        assert_eq!(sched.rate_bps_at_era(ERAS_PER_YEAR * 12), 200);
    }
}
