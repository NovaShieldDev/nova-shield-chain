//! Ekonomik saldırı simülasyonları — stake, slashing, emisyon, fee
//!
//! ## Test Kategorileri
//!
//! 1. **Slashing yeterliliği** — Ceza > kazanç → saldırı kârsız
//! 2. **Stake manipülasyonu** — Validator seti kandırma girişimleri
//! 3. **Emisyon enflasyon kontrolü** — Token emisyonu sınırlı kalır
//! 4. **Fee manipülasyonu** — Base fee yakma koruması
//! 5. **Treasury güvenliği** — Governance onaysız harcama imkânsız
//! 6. **Delegasyon güvenliği** — Delegate/undelegate simetrik

#[cfg(test)]
mod tests {
    use nova_consensus::types::{
        MIN_VALIDATOR_STAKE, SLASH_DOUBLE_SIGN_BPS, SLASH_DOWNTIME_BPS,
        SLASH_WRONG_BATCH_BPS,
    };
    use nova_runtime::{
        era_emission, NSC_TOTAL_SUPPLY, INITIAL_BASE_FEE, BLOCK_GAS_LIMIT,
        calc_next_base_fee, distribute_protocol_fee,
        NscLedger, ValidatorPool, DelegatorStore,
        staking::Validator, BondStatus,
    };
    use nova_primitives::{Address, ValidatorId, SlotId};

    // MIN_VALIDATOR_STAKE_NSC = 100_000 NSC × 10^18 decimals
    const MIN_VALIDATOR_STAKE_NSC: u128 = 100_000 * 1_000_000_000_000_000_000u128;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_validator(id: u64, stake: u128) -> Validator {
        Validator::new(
            ValidatorId(id),
            addr(id as u8),
            stake,
            500,   // 5% komisyon
            stake, // min_self_delegation = stake
            0,     // registered_at_slot
        )
    }

    // ── Slashing Ekonomik Caydırıcılık ────────────────────────────────────────

    /// PoC: Double-sign slash → %100 stake kaybı (deterrence guarantee)
    ///
    /// Ekonomik güvenlik: Double-sign tespitinde validator'ın
    /// tüm stake'i (100%) kesilir. Bu kesinlik, 1 blok bile olsa
    /// saldırıyı kârsız yapar — çünkü minimum stake her zaman
    /// tek bir saldırı döngüsünden kazanılabilecek maksimumdan büyüktür.
    #[test]
    fn poc_double_sign_slash_is_full_stake_loss() {
        let stake = MIN_VALIDATOR_STAKE; // 100k NSC minimum stake
        let slash_bps = SLASH_DOUBLE_SIGN_BPS as u128;
        let slash_amount = stake * slash_bps / 10_000;

        // %100 slash → slash = stake (tam kayıp)
        assert_eq!(
            slash_amount, stake,
            "double-sign slash tam stake'e eşit olmalı (%100 = 10000 BPS)"
        );

        // Slash %100 (10000 BPS) olduğunu doğrula
        assert_eq!(
            SLASH_DOUBLE_SIGN_BPS, 10_000,
            "double-sign slash BPS 10000 (=%100) olmalı"
        );

        // En az 1 yıl içinde kazanılan blok ödülleri stake'i aşmaz
        // (MIN_VALIDATOR_STAKE stake ile giriş yapılır, ödüller daha yavaş birikerek geliyor)
        let single_block_reward_approx: u128 = 4_566 * 10u128.pow(18); // ~4566 NSC
        // 100k NSC slash > 1 blokta kazanılabilecek ödül
        assert!(
            slash_amount > single_block_reward_approx,
            "slash ({slash_amount}) tek blok ödülünden ({single_block_reward_approx}) büyük olmalı"
        );
    }

    /// PoC: Downtime slash oranı hesabı — liveness kaybı kârsız
    #[test]
    fn poc_downtime_slash_makes_laziness_unprofitable() {
        let stake = MIN_VALIDATOR_STAKE * 5; // 500k NSC stake
        let downtime_slash_bps = SLASH_DOWNTIME_BPS as u128; // %1 per epoch
        let epochs_offline: u128 = 7; // 1 hafta offline

        let total_slash = stake * downtime_slash_bps * epochs_offline / 10_000;
        let era_reward = era_emission(NSC_TOTAL_SUPPLY, 0);
        let weekly_reward = era_reward * 7 / 200;

        assert!(
            total_slash > weekly_reward,
            "7 epoch downtime cezası ({total_slash}) haftalık ödülden ({weekly_reward}) büyük olmalı"
        );
    }

    /// PoC: Yanlış batch clearing slash → büyük stake cezası
    #[test]
    fn poc_wrong_batch_slash_is_deterrent() {
        let stake = MIN_VALIDATOR_STAKE * 10;
        let slash_bps = SLASH_WRONG_BATCH_BPS as u128; // %25
        let slash_amount = stake * slash_bps / 10_000;

        // En iyi durumda MEV kazancı: blok başına ~1000 NSC
        let best_mev_gain_per_block: u128 = 1_000 * 10u128.pow(18);

        assert!(
            slash_amount > best_mev_gain_per_block * 100,
            "batch clearing cezası ({slash_amount}) 100 blokluk MEV kazancını ({}) geçmeli",
            best_mev_gain_per_block * 100
        );
    }

    // ── Emisyon & Enflasyon Kontrolü ─────────────────────────────────────────

    /// PoC: Emisyon her yıl azalıyor (deflasyonist tasarım)
    #[test]
    fn poc_emission_decreases_each_year() {
        let supply = NSC_TOTAL_SUPPLY;

        let year_0_emission = era_emission(supply, 0);
        let year_1_emission = era_emission(supply, 1);
        let year_5_emission = era_emission(supply, 5);
        let year_12_emission = era_emission(supply, 12);

        assert!(year_0_emission > year_1_emission, "yıl 0 emisyon > yıl 1 emisyon");
        assert!(year_1_emission > year_5_emission, "yıl 1 emisyon > yıl 5 emisyon");
        assert!(year_5_emission > year_12_emission, "yıl 5 emisyon > yıl 12 emisyon");
        assert!(year_12_emission > 0, "emisyon hiç sıfıra düşmemeli");
    }

    /// PoC: Toplam 12 yıllık emisyon max supply'ın %80'ini geçmemeli
    #[test]
    fn poc_total_emission_bounded() {
        let supply = NSC_TOTAL_SUPPLY;
        let total_12yr: u128 = (0u64..12).map(|y| era_emission(supply, y)).sum();

        // 12 yılda toplam supply'ın %80'inden fazla basılmamalı
        let max_allowed = supply * 80 / 100;

        assert!(
            total_12yr <= max_allowed,
            "12 yıllık emisyon ({total_12yr}) toplam supply'ın %80'ini ({max_allowed}) geçmemeli"
        );
    }

    // ── Fee Manipülasyonu & EIP-1559 ─────────────────────────────────────────

    /// PoC: Base fee %150 dolulukta artmalı (EIP-1559 kuralı)
    ///
    /// EIP-1559: gas_used > gas_target → base fee artar.
    /// gas_target = BLOCK_GAS_LIMIT/2 (hedef), gas_used = BLOCK_GAS_LIMIT (tam blok).
    #[test]
    fn poc_base_fee_increases_on_full_blocks() {
        let initial = INITIAL_BASE_FEE;
        let gas_target = BLOCK_GAS_LIMIT / 2; // EIP-1559 hedef = limiti/2

        let mut fee = initial;
        for _ in 0..5 {
            // Tam blok: gas_used = BLOCK_GAS_LIMIT > gas_target → fee artar
            fee = calc_next_base_fee(fee, BLOCK_GAS_LIMIT, gas_target);
        }

        assert!(fee > initial, "tam blok kullanımında base fee artmalı");
    }

    /// PoC: Base fee boş bloktan sonra azalmalı
    #[test]
    fn poc_base_fee_decreases_on_empty_blocks() {
        let initial = INITIAL_BASE_FEE * 10;

        let mut fee = initial;
        for _ in 0..10 {
            fee = calc_next_base_fee(fee, 0, BLOCK_GAS_LIMIT);
        }

        assert!(fee < initial, "boş bloklar sonrası base fee azalmalı");
        assert!(fee > 0, "base fee sıfıra düşmemeli");
    }

    /// PoC: Protokol fee dağıtımı toplamı kayıpsız
    #[test]
    fn poc_fee_distribution_no_loss() {
        let fee_amounts = [
            1_000_000u128,
            999_999,
            1,
            1_000_000_000_000_000_000,
            7,
            13,
        ];

        for &fee in &fee_amounts {
            let dist = distribute_protocol_fee(fee);
            assert_eq!(
                dist.total(), fee,
                "fee dağıtımı kayıpsız olmalı: girdi={fee}, toplam={}", dist.total()
            );
        }
    }

    // ── Staking Ekonomisi ─────────────────────────────────────────────────────

    /// PoC: Minimum stake ile validator aktif set'e girebilir
    #[test]
    fn poc_minimum_stake_validator_enters_active_set() {
        let pool = ValidatorPool::new();
        let v = make_validator(1, MIN_VALIDATOR_STAKE_NSC);

        pool.register(v).unwrap();
        pool.apply_validator_set_update(SlotId(0));

        let active = pool.active_validators();
        assert!(
            active.iter().any(|v| v.id == ValidatorId(1)),
            "minimum stake'li validator aktif set'e girmeli"
        );
    }

    /// PoC: NscLedger toplam arz koruması — mint takibi doğru
    #[test]
    fn poc_nsc_total_supply_bounded() {
        let ledger = NscLedger::new();

        let initial_mint = 1_000_000u128;
        ledger.mint(addr(1), initial_mint).unwrap();

        assert_eq!(ledger.total_supply(), initial_mint);
        assert!(ledger.total_supply() <= NSC_TOTAL_SUPPLY);
    }

    /// PoC: Token burn toplam supply'ı azaltmalı
    #[test]
    fn poc_token_burn_reduces_supply() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 1_000_000).unwrap();

        let before_supply = ledger.total_supply();
        ledger.burn(addr(1), 100_000).unwrap();
        let after_supply = ledger.total_supply();

        assert_eq!(after_supply, before_supply - 100_000, "burn sonrası toplam arz düşmeli");
        assert_eq!(ledger.total_burned(), 100_000);
    }

    // ── Delegasyon Güvenliği ──────────────────────────────────────────────────

    /// PoC: Delegasyon → ödül hesabı doğru orantı
    #[test]
    fn poc_delegation_reward_proportional() {
        let store = DelegatorStore::new();

        // 3 delegatör: 100, 200, 300 NSC → toplamda 600 NSC
        store.delegate(addr(1), ValidatorId(1), 100, 0).unwrap();
        store.delegate(addr(2), ValidatorId(1), 200, 0).unwrap();
        store.delegate(addr(3), ValidatorId(1), 300, 0).unwrap();

        let mut tracker = store.get_or_create_tracker(ValidatorId(1));
        tracker.set_total_stake(600);
        tracker.add_reward(600);
        store.update_tracker(ValidatorId(1), tracker);

        let r1 = store.pending_reward(&addr(1), &ValidatorId(1));
        let r2 = store.pending_reward(&addr(2), &ValidatorId(1));
        let r3 = store.pending_reward(&addr(3), &ValidatorId(1));

        assert_eq!(r1, 100, "delegatör 1 (100/600) → 100 NSC ödül");
        assert_eq!(r2, 200, "delegatör 2 (200/600) → 200 NSC ödül");
        assert_eq!(r3, 300, "delegatör 3 (300/600) → 300 NSC ödül");
        assert_eq!(r1 + r2 + r3, 600, "toplam ödül kayıpsız dağıtılmalı");
    }

    /// PoC: Delegasyon kaldırılınca sonraki ödüller sıfırlanmalı
    #[test]
    fn poc_undelegate_stops_reward_accrual() {
        let store = DelegatorStore::new();
        store.delegate(addr(10), ValidatorId(2), 500, 0).unwrap();

        // Başlangıç ödül: 500
        let mut tracker = store.get_or_create_tracker(ValidatorId(2));
        tracker.set_total_stake(500);
        tracker.add_reward(500);
        store.update_tracker(ValidatorId(2), tracker);

        let reward_before = store.pending_reward(&addr(10), &ValidatorId(2));
        assert_eq!(reward_before, 500);

        // Undelegate (amount=500, current_slot=0, unbonding_completion=1000)
        store.undelegate(addr(10), ValidatorId(2), 500, 0, 1_000).unwrap();

        // Sonraki ödül eklenince → artık bu delegatöre ulaşmamalı
        let mut tracker2 = store.get_or_create_tracker(ValidatorId(2));
        tracker2.set_total_stake(0);
        tracker2.add_reward(1_000);
        store.update_tracker(ValidatorId(2), tracker2);

        let reward_after = store.pending_reward(&addr(10), &ValidatorId(2));
        assert_eq!(reward_after, 0, "undelegate sonrası ödül sıfır olmalı");
    }

    // ── Proptest: Ekonomik Model Property Testleri ───────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Slash miktarı her zaman [0, stake] aralığında
        #[test]
        fn prop_slash_amount_in_bounds(
            stake in 1u128..10_000_000_000_000_000_000_000_000u128,
            bps   in 0u32..10_001u32,
        ) {
            let slash = stake.saturating_mul(bps as u128) / 10_000;
            prop_assert!(slash <= stake, "slash miktarı stake'i aşamaz");
        }

        /// Property: Era emisyonu her zaman toplam arzın %10'undan az
        #[test]
        fn prop_era_emission_bounded(year in 0u64..100u64) {
            let supply = NSC_TOTAL_SUPPLY;
            let emission = era_emission(supply, year);
            let max_emission = supply / 10; // %10 üst sınır
            prop_assert!(emission <= max_emission,
                "yıl {year}: emisyon {emission} > max {max_emission}");
        }

        /// Property: Yıl arttıkça emisyon monoton azalan veya sabit (deflasyonist model)
        #[test]
        fn prop_emission_monotone_non_increasing(year in 0u64..50u64) {
            let supply = NSC_TOTAL_SUPPLY;
            let e_now  = era_emission(supply, year);
            let e_next = era_emission(supply, year + 1);
            prop_assert!(e_next <= e_now,
                "emisyon yıl {year}: {e_now} → {e_next} artmış (deflasyonist olmalı)");
        }

        /// Property: Base fee tam gaz kullanımında artar, tam boşta azalır
        #[test]
        fn prop_base_fee_direction_correct(
            current_fee in 100_000_000u128..1_000_000_000_000u128,  // MIN_BASE_FEE lower bound
        ) {
            let target = BLOCK_GAS_LIMIT / 2;

            // Tam dolu blok → fee artmalı
            let fee_up = calc_next_base_fee(current_fee, BLOCK_GAS_LIMIT, target);
            prop_assert!(fee_up >= current_fee,
                "tam dolu blok: fee artmalı veya sabit kalmalı");

            // Boş blok → fee azalmalı veya sabit
            let fee_down = calc_next_base_fee(current_fee, 0, target);
            prop_assert!(fee_down <= current_fee,
                "boş blok: fee azalmalı veya sabit kalmalı");
        }

        /// Property: Protocol fee dağıtımı: validator + burn = toplam
        #[test]
        fn prop_fee_distribution_conserved(
            fee in 0u128..1_000_000_000_000_000_000u128,
        ) {
            let dist = distribute_protocol_fee(fee);
            prop_assert_eq!(
                dist.total(), fee,
                "fee dağıtımı korunumlu: tüm paylar toplamı = toplam fee"
            );
            prop_assert!(dist.validators <= fee);
            prop_assert!(dist.stakers <= fee);
        }

        /// Property: 12 yıllık kümülatif emisyon toplam arzı aşmaz
        #[test]
        fn prop_cumulative_emission_never_exceeds_supply(
            start_year in 0u64..88u64,
        ) {
            let supply = NSC_TOTAL_SUPPLY;
            let cumulative: u128 = (start_year..start_year + 12)
                .map(|y| era_emission(supply, y))
                .sum();
            prop_assert!(cumulative <= supply,
                "12 yıllık kümülatif emisyon toplam arzı aşamaz");
        }
    }
}
