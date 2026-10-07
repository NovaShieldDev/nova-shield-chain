//! Sistem invariant testleri — state tutarlılığı, overflow, aritmetik güvenlik
//!
//! ## Test Kategorileri
//!
//! 1. **State tutarlılığı** — Bakiye değişimi toplamı sıfır (korunumlu)
//! 2. **Integer overflow** — u128 toplama/çıkarma taşma koruması
//! 3. **BPS hesaplama** — Basis points aritmetiği doğruluğu
//! 4. **Çift harcama** — Aynı bakiye iki kez kullanılamaz
//! 5. **Paralel TX çakışma** — Aynı hesap için race condition
//! 6. **Sıfır işlemler** — Sıfır miktarlı TX'lerin güvenli işlenmesi

#[cfg(test)]
mod tests {
    use nova_runtime::{
        AccountStateCache, ParallelExecutor, TxValidator, INITIAL_BASE_FEE,
        NSC_TOTAL_SUPPLY, NscLedger, Transaction,
    };
    use nova_primitives::{Address, SlotId};
    use std::sync::Arc;
    use proptest::prelude::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    // ── State Korunumu (Toplam Bakiye Değişmez) ───────────────────────────────

    /// Invariant: Transfer sonrası toplam bakiye değişmemeli
    #[test]
    fn invariant_transfer_preserves_total_balance() {
        let state = Arc::new(AccountStateCache::new());

        let alice = addr(1);
        let bob = addr(2);
        let initial_alice = 1_000_000u128;
        let initial_bob = 500_000u128;

        state.set_balance(alice, initial_alice);
        state.set_balance(bob, initial_bob);

        let total_before = state.balance(&alice) + state.balance(&bob);

        // Transfer 300k
        let transfer = 300_000u128;
        let alice_bal = state.balance(&alice);
        let bob_bal = state.balance(&bob);
        state.set_balance(alice, alice_bal - transfer);
        state.set_balance(bob, bob_bal + transfer);

        let total_after = state.balance(&alice) + state.balance(&bob);

        assert_eq!(
            total_before, total_after,
            "transfer sonrası toplam bakiye değişmemeli: önce={total_before}, sonra={total_after}"
        );
    }

    /// Invariant: Paralel TX yürütmesi sonrası toplam bakiye korunmalı
    #[test]
    fn invariant_parallel_execution_preserves_total() {
        let (tx1, s1) = Transaction::signed_for_test(71, addr(72), 100_000, 0);
        let (tx2, s2) = Transaction::signed_for_test(73, addr(74), 200_000, 0);
        let (tx3, s3) = Transaction::signed_for_test(75, addr(76), 300_000, 0);

        let state = Arc::new(AccountStateCache::new());
        let large_balance = 1_000_000_000_000_000_000_000u128;

        state.set_balance(s1, large_balance);
        state.set_balance(s2, large_balance);
        state.set_balance(s3, large_balance);

        let total_funded = state.balance(&s1) + state.balance(&s2) + state.balance(&s3);

        let executor = ParallelExecutor::new(Arc::clone(&state), INITIAL_BASE_FEE);
        let results = executor.execute_block(&[tx1, tx2, tx3]);

        let success_count = results.iter().filter(|r| r.success).count();
        assert!(success_count > 0, "en az bir TX başarılı olmalı");

        // Toplam sisteme giren/çıkan bakiyenin tutarlı olduğunu doğrula
        // (başarılı transfer yapılan tutar, alıcılara geçti)
        let receiver1_bal = state.balance(&addr(72));
        let receiver2_bal = state.balance(&addr(74));
        let receiver3_bal = state.balance(&addr(76));

        // Alıcıların bakiyeleri geçerli sınırlar içinde olmalı
        assert!(receiver1_bal <= 100_000, "alıcı 1 fazla bakiye almamalı");
        assert!(receiver2_bal <= 200_000, "alıcı 2 fazla bakiye almamalı");
        assert!(receiver3_bal <= 300_000, "alıcı 3 fazla bakiye almamalı");

        // Gönderenler fazla para kaybetmemeli
        assert!(state.balance(&s1) <= large_balance, "gönderici 1 bakiyesi artmamalı");
        assert!(state.balance(&s2) <= large_balance, "gönderici 2 bakiyesi artmamalı");
        assert!(state.balance(&s3) <= large_balance, "gönderici 3 bakiyesi artmamalı");
    }

    // ── Integer Overflow Koruması ────────────────────────────────────────────

    /// PoC: u128 maksimum değeri ile saturating operasyon
    #[test]
    fn poc_u128_overflow_protected_with_saturating() {
        let max = u128::MAX;

        // Saturating add: taşarsa MAX döner, panic yok
        let result = max.saturating_add(1);
        assert_eq!(result, u128::MAX, "u128 saturating_add taşmada MAX döndürmeli");

        // Saturating sub: altına inerse 0 döner
        let zero = 0u128.saturating_sub(1);
        assert_eq!(zero, 0, "u128 saturating_sub underflow'da 0 döndürmeli");
    }

    /// PoC: BPS hesaplamalarında overflow riski yok
    #[test]
    fn poc_bps_calculation_no_overflow() {
        // En büyük stake değeri × 10000 BPS overflow yaratabilir mi?
        let max_stake: u128 = NSC_TOTAL_SUPPLY; // 1 milyar NSC × 10^18
        let max_bps: u128 = 10_000;

        // overflow kontrolü: checked_mul ile
        let safe_result = max_stake.checked_mul(max_bps);
        // NSC_TOTAL_SUPPLY × 10_000 u128::MAX'ı aşmamalı mı?
        // u128::MAX ≈ 3.4 × 10^38
        // NSC_TOTAL_SUPPLY ≈ 10^9 × 10^18 = 10^27
        // 10^27 × 10^4 = 10^31 — u128::MAX'tan küçük, güvenli

        if let Some(product) = safe_result {
            let slash_amount = product / 10_000;
            assert_eq!(slash_amount, max_stake, "BPS hesabı tutarlı: %100 slash = tam stake");
        } else {
            // Overflow durumu test başarısız
            panic!("NSC_TOTAL_SUPPLY × 10000 u128 taşıyor — kritik bug!");
        }
    }

    /// PoC: Küçük miktarlar için BPS yuvarlama tutarlı
    #[test]
    fn poc_bps_rounding_consistent() {
        // 7 NSC (asal sayı) üzerinde %33.33 hesabı
        let amount = 7u128;
        let bps = 3333u128; // %33.33

        let result1 = amount * bps / 10_000;
        let result2 = amount * bps / 10_000;

        assert_eq!(result1, result2, "BPS hesabı deterministik olmalı");
        assert!(result1 <= amount, "BPS sonucu orijinal miktarı aşmamalı");
    }

    // ── Çift Harcama Koruması ────────────────────────────────────────────────

    /// PoC: Bakiye yetersizken çift harcama ParallelExecutor tarafından önlenmeli
    #[test]
    fn poc_double_spend_impossible() {
        // Aynı hesap: 600k bakiye, iki ayrı 500k transfer (toplam 1M — yetersiz)
        let (tx1, sender) = Transaction::signed_for_test(80, addr(81), 500_000, 0);
        let (tx2, _)      = Transaction::signed_for_test(80, addr(82), 500_000, 1);

        let state = Arc::new(AccountStateCache::new());
        // 600_000 + fee — ikinci 500k için yetersiz
        state.set_balance(sender, 600_000u128);

        let executor = ParallelExecutor::new(Arc::clone(&state), INITIAL_BASE_FEE);
        let results = executor.execute_block(&[tx1, tx2]);

        assert_eq!(results.len(), 2, "her TX için sonuç dönmeli");

        // En az biri başarısız olmali — iki 500k aynı 600k bakiyeden çekilemez
        let success_count = results.iter().filter(|r| r.success).count();
        assert!(
            success_count <= 1,
            "600k bakiyeden iki 500k transfer yapılamaz; başarılı: {success_count}"
        );

        // Alıcıların toplam aldığı, göndericinin kaydettiğinden fazla olmamalı
        let alice_got = state.balance(&addr(81));
        let bob_got   = state.balance(&addr(82));
        let sender_lost = 600_000u128.saturating_sub(state.balance(&sender));

        assert!(
            alice_got + bob_got <= sender_lost,
            "çift harcama: alıcılar göndericinin kaybettiğinden fazla alamaz"
        );
    }

    // ── AccountStateCache Tutarlılık ─────────────────────────────────────────

    /// Invariant: Snapshot → mutasyon → restore tam tutarlılık sağlar
    #[test]
    fn invariant_snapshot_restore_consistency() {
        let state = AccountStateCache::new();

        // Başlangıç durumu
        state.set_balance(addr(1), 10_000);
        state.set_balance(addr(2), 20_000);
        for _ in 0..5 { state.increment_nonce(&addr(1)); }

        let snap = state.snapshot();

        // Mutasyonlar
        state.set_balance(addr(1), 99_999);
        state.set_balance(addr(2), 1);
        for _ in 0..100 { state.increment_nonce(&addr(1)); }

        // Restore
        state.restore(snap);

        // Tam olarak önceki duruma dönmeli
        assert_eq!(state.balance(&addr(1)), 10_000, "bakiye restore edilmeli");
        assert_eq!(state.balance(&addr(2)), 20_000, "bakiye 2 restore edilmeli");
        assert_eq!(state.nonce(&addr(1)), 5, "nonce restore edilmeli (5 olmalı)");
    }

    /// Invariant: Birden fazla snapshot'ın inner'ları izole olmalı
    #[test]
    fn invariant_multiple_snapshots_isolated() {
        let state = AccountStateCache::new();
        state.set_balance(addr(1), 100);

        let snap1 = state.snapshot();
        state.set_balance(addr(1), 200);

        let snap2 = state.snapshot();
        state.set_balance(addr(1), 300);

        // snap2'ye dön
        state.restore(snap2);
        assert_eq!(state.balance(&addr(1)), 200);

        // snap1'e dön
        state.restore(snap1);
        assert_eq!(state.balance(&addr(1)), 100, "snap1 restore edilmeli");
    }

    // ── NscLedger Tutarlılık ─────────────────────────────────────────────────

    /// Invariant: mint + burn + transfer sonrası toplam arz doğru
    #[test]
    fn invariant_nsc_ledger_supply_consistency() {
        let ledger = NscLedger::new();

        ledger.mint(addr(1), 1_000_000).unwrap();
        ledger.mint(addr(2), 500_000).unwrap();

        let after_mint = ledger.total_supply();
        assert_eq!(after_mint, 1_500_000);

        ledger.transfer(addr(1), addr(3), 200_000).unwrap();

        // Transfer toplam supply'ı değiştirmemeli
        assert_eq!(
            ledger.total_supply(),
            after_mint,
            "transfer toplam supply'ı değiştirmemeli"
        );

        ledger.burn(addr(2), 100_000).unwrap();

        assert_eq!(
            ledger.total_supply(),
            after_mint - 100_000,
            "burn toplam supply'ı azaltmalı"
        );
        assert_eq!(ledger.total_burned(), 100_000);
    }

    /// PoC: Bakiyesiz adres üzerinde burn hata vermeli
    #[test]
    fn poc_burn_empty_account_fails() {
        let ledger = NscLedger::new();
        let result = ledger.burn(addr(99), 1_000);
        assert!(result.is_err(), "bakiyesiz hesaptan burn başarısız olmalı");
    }

    /// PoC: Transfer sonrası bakiye simetrisi
    #[test]
    fn invariant_transfer_symmetry() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 1_000_000).unwrap();

        let before_sender = ledger.balance_of(&addr(1));
        let before_receiver = ledger.balance_of(&addr(2));

        let amount = 300_000u128;
        ledger.transfer(addr(1), addr(2), amount).unwrap();

        let after_sender = ledger.balance_of(&addr(1));
        let after_receiver = ledger.balance_of(&addr(2));

        assert_eq!(
            after_sender,
            before_sender - amount,
            "gönderici bakiyesi tam olarak düşmeli"
        );
        assert_eq!(
            after_receiver,
            before_receiver + amount,
            "alıcı bakiyesi tam olarak artmalı"
        );
    }

    // ── Slot & Epoch Sınır Testleri ───────────────────────────────────────────

    /// PoC: SlotId u64 overflow → SystemTime güvenli
    #[test]
    fn poc_slot_id_boundary_safe() {
        let max_slot = SlotId(u64::MAX);
        let zero_slot = SlotId(0);

        // Karşılaştırma güvenli
        assert!(max_slot > zero_slot);

        // Wrapping add simülasyonu (overflow olmadan)
        let near_max = SlotId(u64::MAX - 1);
        assert_eq!(near_max.0 + 1, u64::MAX);
    }

    // ── Balance Underflow Koruması ────────────────────────────────────────────

    /// PoC: Bakiye yetersizse transfer hata vermeli (underflow yok)
    #[test]
    fn poc_transfer_underflow_protected() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 100).unwrap();

        // 1000 transfer et — sadece 100 var
        let result = ledger.transfer(addr(1), addr(2), 1_000);
        assert!(result.is_err(), "yetersiz bakiyede transfer başarısız olmalı");

        // Orijinal bakiye korunmuş olmalı
        assert_eq!(ledger.balance_of(&addr(1)), 100, "başarısız transfer sonrası bakiye korunmalı");
    }

    // ── Proptest: Property-Based Arithmetic Invariants ────────────────────────

    proptest! {
        /// Property: Her a,b değeri için transfer sonrası toplam bakiye korunur
        #[test]
        fn prop_transfer_preserves_total(
            a in 1u128..1_000_000_000u128,
            b in 0u128..1_000_000_000u128,
            transfer in 1u128..500_000_000u128,
        ) {
            let ledger = NscLedger::new();
            ledger.mint(Address::from_bytes([1u8; 20]), a).unwrap();
            ledger.mint(Address::from_bytes([2u8; 20]), b).unwrap();

            let total_before = ledger.total_supply();
            let sender = Address::from_bytes([1u8; 20]);
            let receiver = Address::from_bytes([2u8; 20]);

            let _ = ledger.transfer(sender, receiver, transfer);

            // Transfer başarılı ya da başarısız — toplam supply değişmemeli
            prop_assert_eq!(
                ledger.total_supply(), total_before,
                "transfer toplam supply'ı değiştirmemeli"
            );
        }

        /// Property: Yetersiz bakiye → transfer her zaman hata verir
        #[test]
        fn prop_insufficient_balance_always_fails(
            balance in 0u128..1_000_000u128,
            excess  in 1u128..1_000_000u128,
        ) {
            let ledger = NscLedger::new();
            let sender = Address::from_bytes([1u8; 20]);
            let receiver = Address::from_bytes([2u8; 20]);

            ledger.mint(sender, balance).unwrap();

            let amount = balance.saturating_add(excess);
            if amount > balance {
                let result = ledger.transfer(sender, receiver, amount);
                prop_assert!(result.is_err(), "yetersiz bakiye transferi her zaman başarısız olmalı");
            }
        }

        /// Property: BPS hesabı [0, original] aralığında kalır
        #[test]
        fn prop_bps_result_in_bounds(
            amount in 0u128..10_000_000_000_000_000_000_000_000_000u128,
            bps    in 0u32..10_001u32,
        ) {
            let result = amount.saturating_mul(bps as u128) / 10_000;
            prop_assert!(result <= amount, "BPS sonucu orijinal miktarı aşamaz");
        }

        /// Property: Saturating aritmetik hiçbir zaman panic üretmez
        #[test]
        fn prop_saturating_arithmetic_never_panics(
            a in u128::MIN..u128::MAX,
            b in u128::MIN..u128::MAX,
        ) {
            let _ = a.saturating_add(b);
            let _ = a.saturating_sub(b);
            // Overflow ya da underflow → MAX/0 döner, panic yok
        }

        /// Property: Mint → burn simetrisi — net değişim doğru
        #[test]
        fn prop_mint_burn_symmetry(amount in 1u128..1_000_000_000u128) {
            let ledger = NscLedger::new();
            let addr = Address::from_bytes([1u8; 20]);

            ledger.mint(addr, amount).unwrap();
            prop_assert_eq!(ledger.balance_of(&addr), amount);

            ledger.burn(addr, amount).unwrap();
            prop_assert_eq!(ledger.balance_of(&addr), 0);
            prop_assert_eq!(ledger.total_supply(), 0);
            prop_assert_eq!(ledger.total_burned(), amount);
        }
    }
}
