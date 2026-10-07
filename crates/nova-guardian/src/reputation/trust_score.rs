//! Trust Score hesaplama (§5.8.3)
//!
//! Cosmos SDK x/slashing signing_info.go pattern'inden ilham:
//! Her guardian için zincir üstünde ölçülen performans metrikleri.
//!
//! Trust Score = (
//!   stake_score * 30 +
//!   years_score * 25 +
//!   success_rate_score * 20 +
//!   slash_penalty_score * 15 +
//!   response_time_score * 10
//! ) / 100

use serde::{Deserialize, Serialize};

// ─── TrustScoreComponents ─────────────────────────────────────────────────────

/// Trust Score bileşenleri (her biri 0-100 arası)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScoreComponents {
    /// Stake skoru (ne kadar NSC stake edilmiş)
    pub stake_score: u32,
    /// Yıl skoru (kaç yıldır aktif)
    pub years_score: u32,
    /// Başarı oranı skoru (kurtarma başarı %)
    pub success_rate_score: u32,
    /// Slash cezası skoru (kesinti yok = 100, her kesintide düşer)
    pub slash_penalty_score: u32,
    /// Yanıt süresi skoru (hızlı = 100, yavaş = düşük)
    pub response_time_score: u32,
}

impl TrustScoreComponents {
    /// Ağırlıklı ortalama ile nihai Trust Score (0-100)
    pub fn compute(&self) -> u32 {
        (self.stake_score * 30
            + self.years_score * 25
            + self.success_rate_score * 20
            + self.slash_penalty_score * 15
            + self.response_time_score * 10)
            / 100
    }
}

// ─── TrustScoreCalculator ─────────────────────────────────────────────────────

/// Trust Score hesaplayıcı — zincir verilerinden bileşenleri türetir
pub struct TrustScoreCalculator;

impl TrustScoreCalculator {
    /// Stake'e göre skor (1M NSC = 100 puan, orantılı)
    pub fn stake_score(staked_nsc: u128) -> u32 {
        const MAX_STAKE: u128 = 1_000_000 * 1_000_000_000_000_000_000; // 1M NSC
        let score = (staked_nsc * 100 / MAX_STAKE).min(100);
        score as u32
    }

    /// Yıl skoru (10+ yıl = 100, orantılı)
    pub fn years_score(active_eras: u64, eras_per_year: u64) -> u32 {
        if eras_per_year == 0 {
            return 0;
        }
        let years = active_eras / eras_per_year;
        (years * 10).min(100) as u32 // 10 yıl = 100
    }

    /// Başarı oranı skoru (BPS: 10_000 = %100)
    pub fn success_rate_score(successful: u32, total: u32) -> u32 {
        if total == 0 {
            return 70; // Henüz deneyim yok: orta skor
        }
        (successful * 100 / total).min(100)
    }

    /// Slash cezası skoru — her slash %20 düşürür
    pub fn slash_penalty_score(slash_count: u32) -> u32 {
        if slash_count == 0 {
            return 100;
        }
        // Her slash için 20 puan düşür, minimum 0
        100u32.saturating_sub(slash_count * 20)
    }

    /// Yanıt süresi skoru — 1 saatten az = 100, 24 saatten fazla = 0
    /// Slot cinsinden (1 slot = 400ms)
    pub fn response_time_score(avg_response_slots: u64) -> u32 {
        const ONE_HOUR_SLOTS: u64 = 9_000; // 3600s / 0.4s
        const ONE_DAY_SLOTS: u64 = 216_000;

        if avg_response_slots <= ONE_HOUR_SLOTS {
            100
        } else if avg_response_slots >= ONE_DAY_SLOTS {
            0
        } else {
            // Linear interpolation: 1h → 24h → 0..100
            let excess = avg_response_slots - ONE_HOUR_SLOTS;
            let range = ONE_DAY_SLOTS - ONE_HOUR_SLOTS;
            100 - (excess * 100 / range) as u32
        }
    }

    /// Tam skor hesapla
    pub fn compute(
        staked_nsc: u128,
        active_eras: u64,
        eras_per_year: u64,
        successful_recoveries: u32,
        total_recoveries: u32,
        slash_count: u32,
        avg_response_slots: u64,
    ) -> TrustScoreComponents {
        TrustScoreComponents {
            stake_score: Self::stake_score(staked_nsc),
            years_score: Self::years_score(active_eras, eras_per_year),
            success_rate_score: Self::success_rate_score(successful_recoveries, total_recoveries),
            slash_penalty_score: Self::slash_penalty_score(slash_count),
            response_time_score: Self::response_time_score(avg_response_slots),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const ERAS_PER_YEAR: u64 = 365;

    #[test]
    fn test_stake_score() {
        assert_eq!(TrustScoreCalculator::stake_score(0), 0);
        assert_eq!(TrustScoreCalculator::stake_score(1_000_000 * 1_000_000_000_000_000_000), 100);
        assert!(TrustScoreCalculator::stake_score(500_000 * 1_000_000_000_000_000_000) < 100);
    }

    #[test]
    fn test_years_score() {
        assert_eq!(TrustScoreCalculator::years_score(0, ERAS_PER_YEAR), 0);
        assert_eq!(TrustScoreCalculator::years_score(ERAS_PER_YEAR * 10, ERAS_PER_YEAR), 100);
        assert!(TrustScoreCalculator::years_score(ERAS_PER_YEAR * 5, ERAS_PER_YEAR) < 100);
    }

    #[test]
    fn test_success_rate_score_no_history() {
        // Deneyim yoksa orta skor
        assert_eq!(TrustScoreCalculator::success_rate_score(0, 0), 70);
    }

    #[test]
    fn test_success_rate_score_perfect() {
        assert_eq!(TrustScoreCalculator::success_rate_score(10, 10), 100);
    }

    #[test]
    fn test_slash_penalty_no_slash() {
        assert_eq!(TrustScoreCalculator::slash_penalty_score(0), 100);
    }

    #[test]
    fn test_slash_penalty_with_slashes() {
        assert_eq!(TrustScoreCalculator::slash_penalty_score(1), 80);
        assert_eq!(TrustScoreCalculator::slash_penalty_score(5), 0);
        assert_eq!(TrustScoreCalculator::slash_penalty_score(10), 0); // floor at 0
    }

    #[test]
    fn test_response_time_fast() {
        assert_eq!(TrustScoreCalculator::response_time_score(0), 100);
        assert_eq!(TrustScoreCalculator::response_time_score(9_000), 100);
    }

    #[test]
    fn test_response_time_slow() {
        assert_eq!(TrustScoreCalculator::response_time_score(216_000), 0);
        assert_eq!(TrustScoreCalculator::response_time_score(999_999), 0);
    }

    #[test]
    fn test_full_trust_score() {
        let components = TrustScoreCalculator::compute(
            10_000 * 1_000_000_000_000_000_000, // 10_000 NSC
            ERAS_PER_YEAR * 3,                   // 3 yıl
            ERAS_PER_YEAR,
            9, 10,                               // %90 başarı
            0,                                    // slash yok
            5_000,                                // 33 dk yanıt
        );
        let score = components.compute();
        assert!(score > 0 && score <= 100);
    }

    #[test]
    fn test_perfect_trust_score() {
        let components = TrustScoreCalculator::compute(
            1_000_000 * 1_000_000_000_000_000_000, // max stake
            ERAS_PER_YEAR * 10,                      // 10 yıl
            ERAS_PER_YEAR,
            100, 100,                                 // %100 başarı
            0,                                         // slash yok
            1_000,                                     // hızlı yanıt
        );
        assert_eq!(components.compute(), 100);
    }
}
