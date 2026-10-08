//! Vesting modülü — token kilit ve zaman bazlı serbest bırakma (§10.2, §10.3)
//!
//! # Vesting Türleri
//!
//! | Kategori          | Tür             | Cliff | Süre    |
//! |-------------------|-----------------|-------|---------|
//! | Ecosystem         | Linear          | 0     | 48 ay   |
//! | Team & Founders   | CliffLinear     | 12 ay | 48 ay   |
//! | Investors         | CliffLinear     | 12 ay | 24 ay   |
//! | Staking Rewards   | HalvingSchedule | —     | 10 yıl  |
//! | Treasury          | CliffGovernance | 6 ay  | gov     |
//! | Public Sale       | Unlocked        | 0     | 0       |
//! | Liquidity         | Unlocked        | 0     | 0       |
//!
//! # Zaman Birimi
//! Epoch = 216_000 slot × 400ms ≈ 24 saat.
//! 1 ay = 30 epoch (30 gün).
//!
//! # Kullanım
//! ```rust
//! use nova_runtime::token::vesting::{VestingSchedule, VestingState};
//!
//! let schedule = VestingSchedule::cliff_linear(
//!     38_000_000_000_000_000_000_000_000u128, // 38M NOVA
//!     12,  // 12 ay cliff
//!     48,  // 48 ay linear
//! );
//! let unlocked = schedule.vested_amount(18); // 18. ayda ne kadar açılmış?
//! assert!(unlocked > 0);
//! ```

use nova_primitives::{constants::NOVA_DECIMALS_FACTOR, Address};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Zaman sabitleri ──────────────────────────────────────────────────────────

/// Bir aydaki epoch sayısı (30 gün × 1 epoch/gün).
pub const EPOCHS_PER_MONTH: u64 = 30;

/// Bir yıldaki epoch sayısı (12 ay × 30 epoch).
pub const EPOCHS_PER_YEAR: u64 = 360;

// ─── VestingSchedule ─────────────────────────────────────────────────────────

/// Vesting takvimi türü
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VestingSchedule {
    /// TGE'de tamamen açık — kilit yok.
    Unlocked {
        total: u128,
    },

    /// Saf linear vesting — cliff yok, başından itibaren açılır.
    /// Ecosystem & Community (%22) için.
    Linear {
        total: u128,
        /// Toplam vesting süresi (ay)
        duration_months: u64,
    },

    /// Cliff + linear vesting.
    /// Team (%19, 12+48) ve Investors (%17, 12+24) için.
    CliffLinear {
        total: u128,
        /// Kilit süresi (ay) — bu süre dolmadan hiçbir şey açılmaz
        cliff_months: u64,
        /// Cliff sonrası linear vesting süresi (ay)
        linear_months: u64,
    },

    /// Cliff + governance kararıyla açılma.
    /// Treasury (%12) için.
    CliffGovernance {
        total: u128,
        /// Kilit süresi (ay)
        cliff_months: u64,
        /// Governance kararı verildi mi?
        governance_approved: bool,
    },

    /// Yarılanma takvimi — Staking Rewards (%16) için.
    /// Yıl 1-2: %8 APY, Yıl 3-4: %4, Yıl 5-6: %2, Yıl 7-10: %1
    HalvingSchedule {
        total: u128,
        /// Şimdiye kadar dağıtılan miktar
        distributed: u128,
    },
}

impl VestingSchedule {
    /// Unlocked vesting oluştur
    pub fn unlocked(total: u128) -> Self {
        Self::Unlocked { total }
    }

    /// Linear vesting oluştur
    pub fn linear(total: u128, duration_months: u64) -> Self {
        Self::Linear { total, duration_months }
    }

    /// CliffLinear vesting oluştur
    pub fn cliff_linear(total: u128, cliff_months: u64, linear_months: u64) -> Self {
        Self::CliffLinear { total, cliff_months, linear_months }
    }

    /// CliffGovernance vesting oluştur
    pub fn cliff_governance(total: u128, cliff_months: u64) -> Self {
        Self::CliffGovernance { total, cliff_months, governance_approved: false }
    }

    /// HalvingSchedule vesting oluştur
    pub fn halving(total: u128) -> Self {
        Self::HalvingSchedule { total, distributed: 0 }
    }

    /// Toplam token miktarı
    pub fn total(&self) -> u128 {
        match self {
            Self::Unlocked { total } => *total,
            Self::Linear { total, .. } => *total,
            Self::CliffLinear { total, .. } => *total,
            Self::CliffGovernance { total, .. } => *total,
            Self::HalvingSchedule { total, .. } => *total,
        }
    }

    /// `elapsed_months` geçtikten sonra açılmış (vested) toplam miktar.
    ///
    /// Dönen değer her zaman `[0, total]` aralığındadır.
    pub fn vested_amount(&self, elapsed_months: u64) -> u128 {
        match self {
            Self::Unlocked { total } => *total,

            Self::Linear { total, duration_months } => {
                if *duration_months == 0 || elapsed_months >= *duration_months {
                    return *total; // Süre bitti — rounding kayıpsız tam toplam
                }
                (*total / *duration_months as u128) * elapsed_months as u128
            }

            Self::CliffLinear { total, cliff_months, linear_months } => {
                if elapsed_months < *cliff_months {
                    return 0; // Cliff dolmadı
                }
                if *linear_months == 0 {
                    return *total;
                }
                let post_cliff = elapsed_months - cliff_months;
                if post_cliff >= *linear_months {
                    return *total; // Tam vested — rounding kayıpsız
                }
                (*total / *linear_months as u128) * post_cliff as u128
            }

            Self::CliffGovernance { total, cliff_months, governance_approved } => {
                if elapsed_months < *cliff_months {
                    return 0;
                }
                if *governance_approved {
                    *total
                } else {
                    0
                }
            }

            Self::HalvingSchedule { total, distributed } => {
                // NOVA-RED-RUNTIME-006: Her yılı ayrı ayrı biriktirir — yıl sınırında
                // gerileme (regression) olmaz.
                //
                // Halving takvimi:
                // Yıl 0-1 (ay  0-23): %8 APY → yılda 5_120_000 NOVA
                // Yıl 2-3 (ay 24-47): %4 APY → yılda 2_560_000 NOVA
                // Yıl 4-5 (ay 48-71): %2 APY → yılda 1_280_000 NOVA
                // Yıl ≥6  (ay   72+): %1 APY → yılda   640_000 NOVA
                let year = (elapsed_months / 12) as usize;
                let annual_for = |y: usize| -> u128 {
                    match y {
                        0 | 1 => 5_120_000 * NOVA_DECIMALS_FACTOR,
                        2 | 3 => 2_560_000 * NOVA_DECIMALS_FACTOR,
                        4 | 5 => 1_280_000 * NOVA_DECIMALS_FACTOR,
                        _     =>   640_000 * NOVA_DECIMALS_FACTOR,
                    }
                };
                // Tamamlanmış yılları topla
                let mut vested: u128 = 0;
                for y in 0..year {
                    vested = vested.saturating_add(annual_for(y));
                }
                // Mevcut yılın kalan aylarını ekle
                let remaining = elapsed_months % 12;
                if remaining > 0 {
                    let monthly = annual_for(year) / 12;
                    vested = vested.saturating_add(monthly * remaining as u128);
                }
                vested.min(*total).saturating_sub(*distributed)
            }
        }
    }

    /// `elapsed_months` sonunda hâlâ kilitli olan miktar
    pub fn locked_amount(&self, elapsed_months: u64) -> u128 {
        self.total().saturating_sub(self.vested_amount(elapsed_months))
    }

    /// Vesting tamamlandı mı?
    pub fn is_fully_vested(&self, elapsed_months: u64) -> bool {
        self.vested_amount(elapsed_months) >= self.total()
    }

    /// CliffGovernance için governance onayı ver
    pub fn approve_governance(&mut self) {
        if let Self::CliffGovernance { governance_approved, .. } = self {
            *governance_approved = true;
        }
    }

    /// HalvingSchedule için dağıtım güncelle
    pub fn record_distribution(&mut self, amount: u128) {
        if let Self::HalvingSchedule { distributed, total } = self {
            *distributed = (*distributed + amount).min(*total);
        }
    }
}

// ─── VestingState ─────────────────────────────────────────────────────────────

/// Tüm adreslerin vesting durumunu tutan state
///
/// Genesis'te yüklenir, her epoch'ta `tick()` çağrısıyla güncellenir.
#[derive(Debug, Default)]
pub struct VestingState {
    schedules: HashMap<Address, VestingSchedule>,
    /// Mevcut epoch (genesis'te 0)
    current_epoch: u64,
}

impl VestingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Genesis tokenomics dağılımını yükle
    ///
    /// Her kategori için whitepaper §10.2 dağılımı.
    pub fn load_genesis_allocations(
        &mut self,
        ecosystem_addr: Address,
        team_addr: Address,
        investors_addr: Address,
        staking_addr: Address,
        treasury_addr: Address,
        public_sale_addr: Address,
        liquidity_addr: Address,
    ) {
        use nova_primitives::constants::*;

        self.schedules.insert(
            ecosystem_addr,
            VestingSchedule::linear(ALLOC_ECOSYSTEM, 48),
        );
        self.schedules.insert(
            team_addr,
            VestingSchedule::cliff_linear(ALLOC_TEAM, 12, 48),
        );
        self.schedules.insert(
            investors_addr,
            VestingSchedule::cliff_linear(ALLOC_INVESTORS, 12, 24),
        );
        self.schedules.insert(
            staking_addr,
            VestingSchedule::halving(ALLOC_STAKING_REWARDS),
        );
        self.schedules.insert(
            treasury_addr,
            VestingSchedule::cliff_governance(ALLOC_TREASURY, 6),
        );
        self.schedules.insert(
            public_sale_addr,
            VestingSchedule::unlocked(ALLOC_PUBLIC_SALE),
        );
        self.schedules.insert(
            liquidity_addr,
            VestingSchedule::unlocked(ALLOC_LIQUIDITY),
        );
    }

    /// Vesting takvimini kaydet
    pub fn register(&mut self, addr: Address, schedule: VestingSchedule) {
        self.schedules.insert(addr, schedule);
    }

    /// Verilen adres için açılmış (claim edilebilir) miktar
    ///
    /// `elapsed_months` = current_epoch / EPOCHS_PER_MONTH
    pub fn claimable(&self, addr: &Address) -> u128 {
        let elapsed = self.elapsed_months();
        self.schedules
            .get(addr)
            .map(|s| s.vested_amount(elapsed))
            .unwrap_or(0)
    }

    /// Verilen adres için kilitli miktar
    pub fn locked(&self, addr: &Address) -> u128 {
        let elapsed = self.elapsed_months();
        self.schedules
            .get(addr)
            .map(|s| s.locked_amount(elapsed))
            .unwrap_or(0)
    }

    /// Epoch ilerlet
    pub fn advance_epoch(&mut self, epochs: u64) {
        self.current_epoch = self.current_epoch.saturating_add(epochs);
    }

    /// Treasury governance onayı
    pub fn approve_treasury(&mut self, addr: &Address) -> bool {
        if let Some(schedule) = self.schedules.get_mut(addr) {
            schedule.approve_governance();
            return true;
        }
        false
    }

    /// Staking dağıtımı kaydet
    pub fn record_staking_distribution(&mut self, addr: &Address, amount: u128) -> bool {
        if let Some(schedule) = self.schedules.get_mut(addr) {
            schedule.record_distribution(amount);
            return true;
        }
        false
    }

    /// Mevcut epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Geçen ay sayısı
    pub fn elapsed_months(&self) -> u64 {
        self.current_epoch / EPOCHS_PER_MONTH
    }

    /// Kayıtlı adres sayısı
    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::constants::*;

    fn month(n: u64) -> u64 { n }

    // ── VestingSchedule::Unlocked ────────────────────────────────────────────

    #[test]
    fn test_unlocked_always_full() {
        let s = VestingSchedule::unlocked(1_000);
        assert_eq!(s.vested_amount(month(0)), 1_000);
        assert_eq!(s.vested_amount(month(100)), 1_000);
        assert!(s.is_fully_vested(0));
    }

    // ── VestingSchedule::Linear ──────────────────────────────────────────────

    #[test]
    fn test_linear_zero_at_start() {
        let s = VestingSchedule::linear(48_000, 48);
        assert_eq!(s.vested_amount(month(0)), 0);
    }

    #[test]
    fn test_linear_full_at_end() {
        let s = VestingSchedule::linear(48_000, 48);
        assert_eq!(s.vested_amount(month(48)), 48_000);
    }

    #[test]
    fn test_linear_halfway() {
        let s = VestingSchedule::linear(48_000, 48);
        assert_eq!(s.vested_amount(month(24)), 24_000);
    }

    #[test]
    fn test_linear_locked_decreases() {
        let s = VestingSchedule::linear(48_000, 48);
        assert_eq!(s.locked_amount(month(0)), 48_000);
        assert_eq!(s.locked_amount(month(24)), 24_000);
        assert_eq!(s.locked_amount(month(48)), 0);
    }

    #[test]
    fn test_linear_does_not_exceed_total() {
        let s = VestingSchedule::linear(1_000, 12);
        assert_eq!(s.vested_amount(month(999)), 1_000);
    }

    // ── VestingSchedule::CliffLinear ─────────────────────────────────────────

    #[test]
    fn test_cliff_linear_zero_during_cliff() {
        let s = VestingSchedule::cliff_linear(38_000, 12, 48);
        assert_eq!(s.vested_amount(month(0)), 0);
        assert_eq!(s.vested_amount(month(6)), 0);
        assert_eq!(s.vested_amount(month(11)), 0);
    }

    #[test]
    fn test_cliff_linear_starts_after_cliff() {
        let s = VestingSchedule::cliff_linear(38_000, 12, 48);
        // Cliff bitti, 0 ay geçti → hâlâ sıfır
        assert_eq!(s.vested_amount(month(12)), 0);
        // 1 ay sonra
        assert!(s.vested_amount(month(13)) > 0);
    }

    #[test]
    fn test_cliff_linear_full_at_end() {
        let s = VestingSchedule::cliff_linear(48_000, 12, 48);
        // 12 cliff + 48 linear = 60 ay sonra tam açık
        assert_eq!(s.vested_amount(month(60)), 48_000);
    }

    #[test]
    fn test_cliff_linear_team_schedule() {
        // Team: 38M NOVA, 12+48 ay
        let s = VestingSchedule::cliff_linear(ALLOC_TEAM, 12, 48);
        assert_eq!(s.vested_amount(month(12)), 0); // cliff bitti, linear henüz başlamadı
        let monthly = ALLOC_TEAM / 48;
        assert_eq!(s.vested_amount(month(13)), monthly);       // 1 ay
        assert_eq!(s.vested_amount(month(36)), monthly * 24);  // 24 ay = %50
        assert_eq!(s.vested_amount(month(60)), ALLOC_TEAM);    // tam
    }

    #[test]
    fn test_cliff_linear_investor_schedule() {
        // Investors: 34M NOVA, 12+24 ay
        let s = VestingSchedule::cliff_linear(ALLOC_INVESTORS, 12, 24);
        assert_eq!(s.vested_amount(month(11)), 0);
        assert_eq!(s.vested_amount(month(36)), ALLOC_INVESTORS); // tam 24 ay sonra
    }

    // ── VestingSchedule::CliffGovernance ─────────────────────────────────────

    #[test]
    fn test_cliff_governance_zero_without_approval() {
        let s = VestingSchedule::cliff_governance(24_000, 6);
        assert_eq!(s.vested_amount(month(0)), 0);
        assert_eq!(s.vested_amount(month(12)), 0); // cliff geçti ama onay yok
        assert_eq!(s.vested_amount(month(100)), 0);
    }

    #[test]
    fn test_cliff_governance_zero_before_cliff_with_approval() {
        let mut s = VestingSchedule::cliff_governance(24_000, 6);
        s.approve_governance();
        assert_eq!(s.vested_amount(month(5)), 0); // cliff dolmadı
    }

    #[test]
    fn test_cliff_governance_full_after_cliff_and_approval() {
        let mut s = VestingSchedule::cliff_governance(ALLOC_TREASURY, 6);
        s.approve_governance();
        assert_eq!(s.vested_amount(month(6)), ALLOC_TREASURY);
        assert_eq!(s.vested_amount(month(100)), ALLOC_TREASURY);
    }

    // ── VestingSchedule::HalvingSchedule ─────────────────────────────────────

    #[test]
    fn test_halving_year1_monthly_rate() {
        let s = VestingSchedule::halving(ALLOC_STAKING_REWARDS);
        let year1_monthly = 5_120_000 * NOVA_DECIMALS_FACTOR / 12;
        assert_eq!(s.vested_amount(month(1)), year1_monthly);
        // Ay 12 = 1 tam yıl — integer bölme hatası olmadan tam yıllık döner.
        assert_eq!(s.vested_amount(month(12)), 5_120_000 * NOVA_DECIMALS_FACTOR);
    }

    #[test]
    fn test_halving_year3_rate_halved() {
        let s = VestingSchedule::halving(ALLOC_STAKING_REWARDS);
        // Ay 25 = yıl 2 tamamlandı (0,1) + 1 ay kaldı (yıl 2)
        // NOVA-RED-RUNTIME-006: Doğru birikim: 2 tam yıl (5_120_000×2) + 1 ay year2 rate
        let year1_annual = 5_120_000 * NOVA_DECIMALS_FACTOR;
        let year3_monthly = 2_560_000 * NOVA_DECIMALS_FACTOR / 12;
        let expected = year1_annual * 2 + year3_monthly * 1; // 2 tam yıl + 1 ay
        assert_eq!(s.vested_amount(month(25)), expected);
    }

    #[test]
    fn test_halving_never_exceeds_total() {
        let s = VestingSchedule::halving(ALLOC_STAKING_REWARDS);
        // 100 yıl sonra da toplam aşılmamalı
        assert!(s.vested_amount(month(1200)) <= ALLOC_STAKING_REWARDS);
    }

    #[test]
    fn test_halving_record_distribution() {
        let mut s = VestingSchedule::halving(ALLOC_STAKING_REWARDS);
        let monthly = 5_120_000 * NOVA_DECIMALS_FACTOR / 12;
        s.record_distribution(monthly);
        // distributed artık monthly kadar
        // vested_amount = vested - distributed
        assert_eq!(s.vested_amount(month(1)), 0); // tam 1 ay vested = monthly, distributed = monthly
    }

    // ── VestingState ─────────────────────────────────────────────────────────

    fn make_addr(n: u8) -> Address {
        let mut b = [0u8; 20];
        b[19] = n;
        Address::from_bytes(b)
    }

    #[test]
    fn test_vesting_state_genesis_allocations() {
        let mut state = VestingState::new();
        state.load_genesis_allocations(
            make_addr(1), make_addr(2), make_addr(3),
            make_addr(4), make_addr(5), make_addr(6), make_addr(7),
        );
        assert_eq!(state.len(), 7);
    }

    #[test]
    fn test_vesting_state_claimable_tge() {
        let mut state = VestingState::new();
        state.load_genesis_allocations(
            make_addr(1), make_addr(2), make_addr(3),
            make_addr(4), make_addr(5), make_addr(6), make_addr(7),
        );
        // TGE'de (epoch 0):
        // Public Sale (addr 6) ve Liquidity (addr 7) tamamen açık
        assert_eq!(state.claimable(&make_addr(6)), ALLOC_PUBLIC_SALE);
        assert_eq!(state.claimable(&make_addr(7)), ALLOC_LIQUIDITY);
        // Ecosystem (addr 1): linear, henüz 0
        assert_eq!(state.claimable(&make_addr(1)), 0);
        // Team (addr 2): cliff 12 ay, henüz 0
        assert_eq!(state.claimable(&make_addr(2)), 0);
        // Investors (addr 3): cliff 12 ay, henüz 0
        assert_eq!(state.claimable(&make_addr(3)), 0);
        // Treasury (addr 5): cliff + governance, henüz 0
        assert_eq!(state.claimable(&make_addr(5)), 0);
    }

    #[test]
    fn test_vesting_state_advance_epoch() {
        let mut state = VestingState::new();
        state.load_genesis_allocations(
            make_addr(1), make_addr(2), make_addr(3),
            make_addr(4), make_addr(5), make_addr(6), make_addr(7),
        );
        // 13 ay ilerle (13 * 30 epoch)
        state.advance_epoch(13 * EPOCHS_PER_MONTH);
        assert_eq!(state.elapsed_months(), 13);

        // Ecosystem artık 13/48 oranında açık
        let eco = state.claimable(&make_addr(1));
        let expected = ALLOC_ECOSYSTEM / 48 * 13;
        assert_eq!(eco, expected);

        // Team: 12 ay cliff + 1 ay geçti → 1 ay unlocked
        let team_monthly = ALLOC_TEAM / 48;
        assert_eq!(state.claimable(&make_addr(2)), team_monthly);

        // Investors: 12 ay cliff + 1 ay geçti → 1 ay unlocked
        let inv_monthly = ALLOC_INVESTORS / 24;
        assert_eq!(state.claimable(&make_addr(3)), inv_monthly);
    }

    #[test]
    fn test_vesting_state_treasury_governance_approval() {
        let mut state = VestingState::new();
        state.load_genesis_allocations(
            make_addr(1), make_addr(2), make_addr(3),
            make_addr(4), make_addr(5), make_addr(6), make_addr(7),
        );
        // 6 ay geç
        state.advance_epoch(6 * EPOCHS_PER_MONTH);
        // Governance onayı olmadan treasury 0
        assert_eq!(state.claimable(&make_addr(5)), 0);
        // Governance onayı ver
        state.approve_treasury(&make_addr(5));
        // Artık tamamen açık
        assert_eq!(state.claimable(&make_addr(5)), ALLOC_TREASURY);
    }

    #[test]
    fn test_vesting_state_locked_decreases_over_time() {
        let mut state = VestingState::new();
        state.load_genesis_allocations(
            make_addr(1), make_addr(2), make_addr(3),
            make_addr(4), make_addr(5), make_addr(6), make_addr(7),
        );
        let initial_locked = state.locked(&make_addr(1));
        assert_eq!(initial_locked, ALLOC_ECOSYSTEM);

        state.advance_epoch(24 * EPOCHS_PER_MONTH); // 24 ay
        let mid_locked = state.locked(&make_addr(1));
        assert!(mid_locked < initial_locked);
        // 24/48 = %50 kalmış — integer bölme hassasiyeti ile yaklaşık eşit
        // actual = ALLOC_ECOSYSTEM - (ALLOC_ECOSYSTEM / 48 * 24)
        let expected = ALLOC_ECOSYSTEM - (ALLOC_ECOSYSTEM / 48 * 24);
        assert_eq!(mid_locked, expected);
        assert!(mid_locked <= ALLOC_ECOSYSTEM / 2 + NOVA_DECIMALS_FACTOR); // tolerans
    }

    #[test]
    fn test_fully_vested_flags() {
        let s_unlocked = VestingSchedule::unlocked(1_000);
        assert!(s_unlocked.is_fully_vested(0));

        let s_linear = VestingSchedule::linear(1_000, 12);
        assert!(!s_linear.is_fully_vested(11));
        assert!(s_linear.is_fully_vested(12));

        let s_cliff = VestingSchedule::cliff_linear(1_000, 12, 24);
        assert!(!s_cliff.is_fully_vested(35));
        assert!(s_cliff.is_fully_vested(36));
    }

    #[test]
    fn test_tge_circulating_supply_correct() {
        // TGE dolaşımı = Public Sale + Liquidity = 28M NOVA
        assert_eq!(ALLOC_PUBLIC_SALE + ALLOC_LIQUIDITY, TGE_CIRCULATING_SUPPLY);
        assert_eq!(TGE_CIRCULATING_SUPPLY, 28_000_000 * NOVA_DECIMALS_FACTOR);
    }
}
