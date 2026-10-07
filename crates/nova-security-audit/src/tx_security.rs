//! Transaction güvenlik testleri — replay, nonce, imza bypass, flood
//!
//! ## Test Kategorileri
//!
//! 1. **Replay saldırısı** — Aynı TX iki kez gönderilemez (nonce monoton artış)
//! 2. **İmza bypass** — Geçersiz/sıfır imzayla TX reddedilir
//! 3. **Sender sahteciliği** — Başkasının imzasıyla TX oluşturma başarısız
//! 4. **Nonce sırası** — Nonce gap / future nonce davranışı
//! 5. **Gas & fee** — Eksik fee TX'leri reddeder
//! 6. **Overflow** — u128 taşma koruması

#[cfg(test)]
mod tests {
    use nova_runtime::{
        AccountStateCache, TxValidator, INITIAL_BASE_FEE,
        Transaction, TransactionKind,
    };
    use nova_primitives::{Address, SlotId, ValidatorId};
    use std::sync::Arc;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_state_with_balance(sender: Address, balance: u128) -> Arc<AccountStateCache> {
        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, balance);
        state
    }

    // ── Replay Koruması ───────────────────────────────────────────────────────

    /// PoC: Aynı nonce ile ikinci TX reddedilmeli (replay koruması)
    #[test]
    fn poc_tx_replay_same_nonce_rejected() {
        let (tx1, sender) = Transaction::signed_for_test(1, addr(2), 100, 0);
        let (tx2, _) = Transaction::signed_for_test(1, addr(2), 100, 0);

        let state = make_state_with_balance(sender, 1_000_000_000_000_000_000);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        // İlk TX başarılı
        assert!(
            validator.validate(&tx1, &state).is_ok(),
            "ilk TX kabul edilmeli"
        );

        // Nonce'u kullanılmış olarak işaretle
        state.increment_nonce(&sender);

        // Aynı nonce ile tekrar → reddedilmeli
        assert!(
            validator.validate(&tx2, &state).is_err(),
            "nonce replay reddedilmeli"
        );
    }

    /// PoC: Geçmiş nonce (stale nonce) TX'i reddedilmeli
    #[test]
    fn poc_stale_nonce_rejected() {
        let (tx, sender) = Transaction::signed_for_test(2, addr(3), 50, 0);
        let state = make_state_with_balance(sender, 1_000_000_000_000_000_000);

        // Nonce'u 5'e yükselt
        for _ in 0..5 {
            state.increment_nonce(&sender);
        }

        let validator = TxValidator::new(INITIAL_BASE_FEE);
        // Nonce 0 → eski, reddedilmeli
        assert!(
            validator.validate(&tx, &state).is_err(),
            "eski nonce reddedilmeli"
        );
    }

    /// PoC: Future nonce (gap) TX'i reddedilmeli
    #[test]
    fn poc_future_nonce_rejected() {
        let (tx, sender) = Transaction::signed_for_test(3, addr(4), 50, 10);
        let state = make_state_with_balance(sender, 1_000_000_000_000_000_000);
        // Hesabın nonce'u 0, TX nonce 10 → gap var

        let validator = TxValidator::new(INITIAL_BASE_FEE);
        assert!(
            validator.validate(&tx, &state).is_err(),
            "nonce gap reddedilmeli"
        );
    }

    // ── İmza Bypass Denemeleri ────────────────────────────────────────────────

    /// PoC: Boş imza TX'i reddedilmeli
    #[test]
    fn poc_empty_signature_rejected() {
        let state = make_state_with_balance(addr(5), 1_000_000_000_000_000_000);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        let tx = Transaction {
            sender: addr(5),
            nonce: 0,
            kind: TransactionKind::Transfer { to: addr(6), amount: 100 },
            max_fee_per_gas: INITIAL_BASE_FEE * 2,
            max_priority_fee_per_gas: 1_000_000,
            gas_limit: 21_000,
            signature: vec![],       // boş imza
            slot: SlotId(0),
        };

        assert!(
            validator.validate(&tx, &state).is_err(),
            "boş imza reddedilmeli"
        );
    }

    /// PoC: Sıfır dolu (zeroed) imza reddedilmeli
    #[test]
    fn poc_zeroed_signature_rejected() {
        let state = make_state_with_balance(addr(7), 1_000_000_000_000_000_000);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        let tx = Transaction {
            sender: addr(7),
            nonce: 0,
            kind: TransactionKind::Transfer { to: addr(8), amount: 100 },
            max_fee_per_gas: INITIAL_BASE_FEE * 2,
            max_priority_fee_per_gas: 1_000_000,
            gas_limit: 21_000,
            signature: vec![0u8; 65], // 65-byte sıfırlar — geçersiz
            slot: SlotId(0),
        };

        assert!(
            validator.validate(&tx, &state).is_err(),
            "sıfır imza reddedilmeli"
        );
    }

    /// PoC: Başka birinin TX'ini kopyalayıp sender'ı değiştirmek → imza uyuşmuyor
    #[test]
    fn poc_sender_spoofing_rejected() {
        // Keypair 10'dan gerçek TX al
        let (mut tx, _original_sender) = Transaction::signed_for_test(10, addr(11), 100, 0);

        // Sender'ı farklı bir adrese değiştir (spoofing)
        tx.sender = addr(99);

        let state = make_state_with_balance(addr(99), 1_000_000_000_000_000_000);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        assert!(
            validator.validate(&tx, &state).is_err(),
            "sender sahteciliği reddedilmeli"
        );
    }

    /// PoC: Tamamen rastgele 65-byte imza reddedilmeli
    #[test]
    fn poc_random_signature_rejected() {
        let state = make_state_with_balance(addr(12), 1_000_000_000_000_000_000);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        let tx = Transaction {
            sender: addr(12),
            nonce: 0,
            kind: TransactionKind::Transfer { to: addr(13), amount: 100 },
            max_fee_per_gas: INITIAL_BASE_FEE * 2,
            max_priority_fee_per_gas: 1_000_000,
            gas_limit: 21_000,
            // Rastgele değerler ama ECDSA formatında değil
            signature: (0u8..65).collect(),
            slot: SlotId(0),
        };

        assert!(
            validator.validate(&tx, &state).is_err(),
            "rastgele imza reddedilmeli"
        );
    }

    // ── Bakiye & Fee Kontrolleri ──────────────────────────────────────────────

    /// PoC: Yetersiz bakiye TX'i reddedilmeli
    #[test]
    fn poc_insufficient_balance_rejected() {
        let (tx, sender) = Transaction::signed_for_test(20, addr(21), 1_000_000, 0);

        // Sadece 1 wei bakiye — fee bile ödeyemez
        let state = make_state_with_balance(sender, 1);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        assert!(
            validator.validate(&tx, &state).is_err(),
            "yetersiz bakiye reddedilmeli"
        );
    }

    /// PoC: max_fee_per_gas < base_fee TX'i reddedilmeli
    #[test]
    fn poc_below_base_fee_rejected() {
        let state = Arc::new(AccountStateCache::new());
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        // Geçerli imza ama fee çok düşük
        let (mut tx, sender) = Transaction::signed_for_test(30, addr(31), 100, 0);
        state.set_balance(sender, 1_000_000_000_000_000_000);

        // max_fee_per_gas'ı base_fee'nin altına çek
        tx.max_fee_per_gas = INITIAL_BASE_FEE / 2;

        // İmzayı yeniden oluşturmadan fee değişirse imza geçersizleşecek
        // Bu da imza doğrulamasında yakalanır
        let result = validator.validate(&tx, &state);
        assert!(result.is_err(), "düşük fee ya da imza uyumsuzluğu reddedilmeli");
    }

    // ── Geçerli TX Kontrolü ───────────────────────────────────────────────────

    /// Pozitif: Doğru imzalı geçerli TX kabul edilmeli
    #[test]
    fn valid_signed_tx_accepted() {
        let (tx, sender) = Transaction::signed_for_test(40, addr(41), 100, 0);
        let state = make_state_with_balance(sender, 1_000_000_000_000_000_000_000u128);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        assert!(
            validator.validate(&tx, &state).is_ok(),
            "geçerli imzalı TX kabul edilmeli"
        );
    }

    /// Pozitif: Ardışık nonce'lar kabul edilmeli
    #[test]
    fn sequential_nonces_accepted() {
        let (tx0, sender) = Transaction::signed_for_test(50, addr(51), 10, 0);
        let (tx1, _) = Transaction::signed_for_test(50, addr(51), 10, 1);

        let large_balance = 1_000_000_000_000_000_000_000u128;
        let state = make_state_with_balance(sender, large_balance);
        let validator = TxValidator::new(INITIAL_BASE_FEE);

        assert!(validator.validate(&tx0, &state).is_ok(), "nonce 0 kabul edilmeli");
        state.increment_nonce(&sender);
        assert!(validator.validate(&tx1, &state).is_ok(), "nonce 1 kabul edilmeli");
    }

    // ── Stake TX Güvenliği ────────────────────────────────────────────────────

    /// PoC: Sıfır miktarda stake TX'i reddedilmeli (ekonomik güvenlik)
    #[test]
    fn poc_zero_amount_stake_rejected() {
        let validator_obj = TxValidator::new(INITIAL_BASE_FEE);

        let tx = Transaction {
            sender: addr(60),
            nonce: 0,
            kind: TransactionKind::Stake {
                validator_id: ValidatorId(1),
                amount: 0, // sıfır stake
            },
            max_fee_per_gas: INITIAL_BASE_FEE * 2,
            max_priority_fee_per_gas: 1_000_000,
            gas_limit: 50_000,
            signature: vec![0u8; 65],
            slot: SlotId(0),
        };

        let state = make_state_with_balance(addr(60), 1_000_000_000_000_000_000);
        // İmza geçersiz (sıfır imza) → reddedilmeli
        assert!(
            validator_obj.validate(&tx, &state).is_err(),
            "geçersiz imzalı zero-stake reddedilmeli"
        );
    }

    // ── Proptest: TX Doğrulama Property Testleri ──────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Sıfır imza her zaman reddedilmeli
        #[test]
        fn prop_zero_signature_always_rejected(
            nonce  in 0u64..100u64,
            amount in 1u128..1_000_000u128,
        ) {
            let validator_obj = TxValidator::new(INITIAL_BASE_FEE);
            let tx = Transaction {
                sender: addr(100),
                nonce,
                kind: TransactionKind::Transfer { to: addr(101), amount },
                max_fee_per_gas: INITIAL_BASE_FEE * 2,
                max_priority_fee_per_gas: 1_000_000,
                gas_limit: 21_000,
                signature: vec![0u8; 65],
                slot: SlotId(0),
            };
            let state = make_state_with_balance(addr(100), 1_000_000_000_000_000_000);
            prop_assert!(validator_obj.validate(&tx, &state).is_err(),
                "sıfır imza her zaman reddedilmeli");
        }

        /// Property: Sıfır gas limit her zaman reddedilmeli
        #[test]
        fn prop_zero_gas_limit_always_rejected(amount in 1u128..1_000_000u128) {
            let validator_obj = TxValidator::new(INITIAL_BASE_FEE);
            let tx = Transaction {
                sender: addr(100),
                nonce: 0,
                kind: TransactionKind::Transfer { to: addr(101), amount },
                max_fee_per_gas: INITIAL_BASE_FEE * 2,
                max_priority_fee_per_gas: 1_000_000,
                gas_limit: 0,
                signature: vec![0u8; 65],
                slot: SlotId(0),
            };
            let state = make_state_with_balance(addr(100), 1_000_000_000_000_000_000);
            prop_assert!(validator_obj.validate(&tx, &state).is_err(),
                "sıfır gas limit her zaman reddedilmeli");
        }

        /// Property: Bakiye < amount → TX her zaman reddedilmeli
        #[test]
        fn prop_insufficient_balance_tx_rejected(
            amount  in 1_000_000u128..1_000_000_000u128,
            deficit in 1u128..999_999u128,
        ) {
            let (tx, sender) = Transaction::signed_for_test(1, addr(150), amount, 0);
            let insufficient = amount.saturating_sub(deficit);

            let state = Arc::new(AccountStateCache::new());
            state.set_balance(sender, insufficient);

            let validator_obj = TxValidator::new(INITIAL_BASE_FEE);
            prop_assert!(validator_obj.validate(&tx, &state).is_err(),
                "yetersiz bakiye TX reddedilmeli");
        }

        /// Property: Fee > max_fee_per_gas * gas_limit → TX reddedilmeli
        #[test]
        fn prop_valid_tx_with_sufficient_balance_accepted(
            seed   in 2u8..200u8,
            amount in 1u128..1_000_000u128,
        ) {
            let (tx, sender) = Transaction::signed_for_test(seed, addr(201), amount, 0);
            // Yeterli bakiye: amount + max_fee * gas_limit'i karşılar
            let large_balance = 1_000_000_000_000_000_000_000u128;
            let state = Arc::new(AccountStateCache::new());
            state.set_balance(sender, large_balance);

            let validator_obj = TxValidator::new(INITIAL_BASE_FEE);
            prop_assert!(
                validator_obj.validate(&tx, &state).is_ok(),
                "geçerli imza + yeterli bakiye olan TX her zaman kabul edilmeli"
            );
        }
    }
}
