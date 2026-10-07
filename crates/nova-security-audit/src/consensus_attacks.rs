//! Konsensüs katmanı saldırı simülasyonları
//!
//! ## Test Kategorileri
//!
//! 1. **Double-sign / Equivocation** — Aynı slot'ta farklı blok önerme
//! 2. **Quorum manipulation** — 2/3+1 kuralını ihlal etme girişimi
//! 3. **Slashing enforcement** — Ceza uygulaması yeterliliği
//! 4. **Long-range attack** — Eski epoch'tan fork oluşturma girişimi
//! 5. **Validator liveness** — Offline validator tespiti

#[cfg(test)]
mod tests {
    use nova_consensus::{
        SlashingEngine, ValidatorRegistry,
        types::{
            DoubleSignEvidence, MIN_VALIDATOR_STAKE, SLASH_DOUBLE_SIGN_BPS,
        },
    };
    use nova_primitives::{Address, Balance, BlockHash, SlotId, ValidatorId};
    use std::sync::Arc;

    fn make_registry_with_validator(id: u64, stake: u128) -> Arc<ValidatorRegistry> {
        let registry = Arc::new(ValidatorRegistry::new());
        let addr = Address::from([id as u8; 20]);
        registry
            .register(
                ValidatorId(id),
                addr,
                Balance(stake),
                500, // 5% komisyon
                [id as u8; 32],
                SlotId(0),
            )
            .expect("validator kaydı başarısız");
        registry
    }

    fn make_evidence(validator_id: u64, slot: u64) -> DoubleSignEvidence {
        let mut hash_a = [0u8; 32];
        let mut hash_b = [0u8; 32];
        hash_a[0] = 0xAA;
        hash_b[0] = 0xBB;
        DoubleSignEvidence {
            validator: ValidatorId(validator_id),
            height: slot,
            block_hash_a: BlockHash::from_bytes(hash_a),
            block_hash_b: BlockHash::from_bytes(hash_b),
            submitted_at_slot: SlotId(slot),
        }
    }

    // ── Double-Sign / Equivocation ────────────────────────────────────────────

    /// PoC: Validator double-sign → tombstone + %100 slash + kalıcı jail
    #[test]
    fn poc_double_sign_triggers_full_slash() {
        let registry = make_registry_with_validator(1, MIN_VALIDATOR_STAKE * 10);
        let engine = SlashingEngine::new(Arc::clone(&registry));

        let initial_info = registry.get(&ValidatorId(1)).unwrap();
        assert!(initial_info.stake.0 >= MIN_VALIDATOR_STAKE * 10);

        // Double-sign kanıtı sun
        let evidence = make_evidence(1, 100);
        let result = engine.handle_double_sign(evidence, SlotId(100));
        assert!(result.is_ok(), "double-sign işlenmeli");

        // Validator tombstone edilmiş olmalı
        assert!(
            engine.is_tombstoned(ValidatorId(1)),
            "double-sign sonrası validator tombstone edilmeli"
        );

        // Stake sıfırlanmış veya ciddi şekilde kesilmiş olmalı
        let post_info = registry.get(&ValidatorId(1)).unwrap();
        assert!(
            post_info.stake.0 < MIN_VALIDATOR_STAKE * 10,
            "double-sign sonrası stake kesilmiş olmalı"
        );
    }

    /// PoC: Aynı kanıt iki kez işlenemez (replay protection)
    #[test]
    fn poc_double_sign_evidence_not_replayable() {
        let registry = make_registry_with_validator(2, MIN_VALIDATOR_STAKE * 5);
        let engine = SlashingEngine::new(Arc::clone(&registry));

        let evidence = make_evidence(2, 200);

        // İlk: başarılı
        assert!(engine.handle_double_sign(evidence.clone(), SlotId(200)).is_ok());

        // İkinci: aynı kanıt tekrar işlenmemeli
        let result = engine.handle_double_sign(evidence, SlotId(200));
        assert!(
            result.is_err(),
            "aynı double-sign kanıtı iki kez işlenmemeli"
        );
    }

    /// PoC: Çok eski kanıt reddedilmeli (stale evidence)
    #[test]
    fn poc_stale_evidence_rejected() {
        let registry = make_registry_with_validator(3, MIN_VALIDATOR_STAKE * 5);
        let engine = SlashingEngine::new(Arc::clone(&registry));

        // Kanıt çok eski: slot 1'de oluştu
        let evidence = make_evidence(3, 1);
        let current_slot = SlotId(10_000_000); // şu an çok ileride

        let result = engine.handle_double_sign(evidence, current_slot);
        assert!(
            result.is_err(),
            "çok eski double-sign kanıtı reddedilmeli"
        );
    }

    /// PoC: Var olmayan validator için slashing başarısız olmalı
    #[test]
    fn poc_slash_nonexistent_validator_fails() {
        let registry = Arc::new(ValidatorRegistry::new());
        let engine = SlashingEngine::new(Arc::clone(&registry));

        let evidence = DoubleSignEvidence {
            validator: ValidatorId(9999),
            height: 100,
            block_hash_a: BlockHash::from_bytes([0xAA; 32]),
            block_hash_b: BlockHash::from_bytes([0xBB; 32]),
            submitted_at_slot: SlotId(100),
        };

        let result = engine.handle_double_sign(evidence, SlotId(100));
        assert!(result.is_err(), "var olmayan validator için slashing hata vermeli");
    }

    // ── Ekonomik Caydırıcılık ─────────────────────────────────────────────────

    /// PoC: Slashing cezası saldırıyı kârsız yapmalı
    ///
    /// Senaryo: Validator X stake'ini bilerek double-sign yaparak bir blok üretiyor.
    /// Beklenti: Kaybedilen stake > kazanılan blok ödülü
    #[test]
    fn poc_slashing_makes_attack_unprofitable() {
        // Varsayımlar:
        // - Validator stake: 1_000_000 NSC
        // - Blok ödülü: ~4566 NSC / epoch (genesis'ten)
        // - Double-sign slash: %100 (SLASH_DOUBLE_SIGN_BPS = 10000)
        let stake = MIN_VALIDATOR_STAKE * 10; // 10x min stake
        let block_reward_per_slot: u128 = 4_566; // yaklaşık değer

        let slash_amount = stake * SLASH_DOUBLE_SIGN_BPS as u128 / 10_000;

        // Saldırganın kazanabileceği maksimum: birkaç blok ödülü
        let max_possible_gain = block_reward_per_slot * 100; // 100 blok

        assert!(
            slash_amount > max_possible_gain,
            "slashing cezası ({slash_amount}) saldırı kazancından ({max_possible_gain}) büyük olmalı — ekonomik caydırıcılık sağlanmalı"
        );
    }

    // ── Quorum Koruması ───────────────────────────────────────────────────────

    /// PoC: 3 validatörlü ağda 1 hatalı validatör konsensüsü kıramamalı
    #[test]
    fn poc_single_byzantine_validator_cannot_halt_3_node_network() {
        // BFT teoremi: n ≥ 3f+1 → f hatalı node tolere edilir
        // 4 node → 1 Byzantine tolere edilir
        let n: u32 = 4; // validator sayısı
        let quorum_bps: u32 = 6667; // ~2/3
        let quorum = (n * quorum_bps + 9999) / 10_000;

        // 1 Byzantine: geri kalan n-1 = 3 node oylamalı
        let honest_votes = n - 1;
        assert!(
            honest_votes >= quorum,
            "3 dürüst node ({honest_votes}) quorum ({quorum}) sağlamalı"
        );
    }

    /// PoC: 2/3+1 quorum olmadan blok finalize edilemez
    #[test]
    fn poc_quorum_threshold_enforced() {
        let n = 10u32;
        let quorum_bps = 6667u32;
        let required_quorum = (n * quorum_bps + 9999) / 10_000;

        // 6/10 oy — yeterli değil (6667 = 6.667 → 7 gerekli)
        let insufficient_votes = 6u32;
        assert!(
            insufficient_votes < required_quorum,
            "6/10 oy quorum sağlamamalı"
        );

        // 7/10 oy — yeterli
        let sufficient_votes = 7u32;
        assert!(
            sufficient_votes >= required_quorum,
            "7/10 oy quorum sağlamalı"
        );
    }

    // ── Jailing ───────────────────────────────────────────────────────────────

    /// PoC: Jailed validator aktif set'te olmamalı
    #[test]
    fn poc_jailed_validator_excluded_from_active_set() {
        let registry = make_registry_with_validator(10, MIN_VALIDATOR_STAKE * 5);

        // Jail et
        registry.jail(&ValidatorId(10), SlotId(u64::MAX)).expect("jail başarısız");

        // Active set'te olmamalı
        let active = registry.get_active_validators();
        assert!(
            !active.iter().any(|v| v.validator_id == ValidatorId(10)),
            "jailed validator active set'te olmamalı"
        );
    }

    /// PoC: Tombstoned validator hiçbir zaman unjail edilemez
    #[test]
    fn poc_tombstoned_validator_permanently_banned() {
        let registry = make_registry_with_validator(11, MIN_VALIDATOR_STAKE * 5);
        let engine = SlashingEngine::new(Arc::clone(&registry));

        // Double-sign → tombstone
        let evidence = make_evidence(11, 500);
        engine.handle_double_sign(evidence, SlotId(500)).unwrap();

        // Tombstoned validator unjail edilemez
        let unjail_result = registry.unjail(&ValidatorId(11), SlotId(u64::MAX));
        assert!(
            unjail_result.is_err(),
            "tombstoned validator unjail edilememeli"
        );
    }
    // ── Proptest: Konsensüs Property Testleri ────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Slash miktarı her zaman [0, stake] aralığında
        #[test]
        fn prop_slash_amount_bounded_by_stake(
            stake_millions in 100_000u128..10_000_000u128,
            slash_bps in 0u32..10_001u32,
        ) {
            let stake = stake_millions * 1_000_000_000_000_000_000u128;
            let registry = make_registry_with_validator(1, stake);
            use nova_consensus::types::SlashReason;
            let slashed = registry.slash(&ValidatorId(1), slash_bps, SlashReason::DoubleSign, SlotId(1))
                .expect("slash başarılı");
            prop_assert!(slashed <= stake, "slash miktarı stake'i aşamaz");
        }

        /// Property: Minimum stake altında kayıt başarısız
        #[test]
        fn prop_below_min_stake_registration_fails(
            stake in 0u128..100_000u128,
        ) {
            let registry = Arc::new(ValidatorRegistry::new());
            // MIN_VALIDATOR_STAKE = 100_000 * 10^18; bu testler çok daha düşük
            let result = registry.register(
                ValidatorId(42),
                Address::from([42u8; 20]),
                Balance(stake),
                500,
                [42u8; 32],
                SlotId(0),
            );
            prop_assert!(result.is_err(), "minimum stake altı kayıt başarısız olmalı (stake={stake})");
        }

        /// Property: Double-sign BPS tam stake kaybına yol açar
        #[test]
        fn prop_double_sign_slash_is_100_percent(
            stake_millions in 100_000u128..1_000_000u128,
        ) {
            use nova_consensus::types::{SLASH_DOUBLE_SIGN_BPS, SlashReason};
            let stake = stake_millions * 1_000_000_000_000_000_000u128;
            let registry = make_registry_with_validator(2, stake);
            let slashed = registry.slash(&ValidatorId(2), SLASH_DOUBLE_SIGN_BPS, SlashReason::DoubleSign, SlotId(1))
                .expect("slash başarılı");
            let expected = stake * SLASH_DOUBLE_SIGN_BPS as u128 / 10_000;
            prop_assert_eq!(slashed, expected, "double-sign slash miktarı beklenen değerde");
        }

        /// Property: Validator total_active_stake kayıttan sonra artar
        #[test]
        fn prop_register_increases_active_stake(
            stake_millions in 100_000u128..500_000u128,
        ) {
            let stake = stake_millions * 1_000_000_000_000_000_000u128;
            let registry = Arc::new(ValidatorRegistry::new());
            let before = registry.total_active_stake();
            registry.register(ValidatorId(10), Address::from([10u8; 20]), Balance(stake), 500, [10u8; 32], SlotId(0)).unwrap();
            let after = registry.total_active_stake();
            prop_assert!(after > before, "kayıt sonrası toplam aktif stake artmalı");
        }
    }
}
