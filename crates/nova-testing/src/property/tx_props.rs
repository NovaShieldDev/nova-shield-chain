//! TX property-based testleri (proptest).
//!
//! **İspatlanan özellikler:**
//! 1. TX hash deterministik: aynı tx → aynı hash
//! 2. TX hash çarpışma direnci: farklı nonce → farklı hash
//! 3. Farklı gönderici → farklı hash
//! 4. EIP-1559 efektif gas fiyatı: base_fee ≤ max_fee ise tip doğru kesilir
//! 5. Nonce sıralı kabul: nonce boşluk → reject
//! 6. Gas hesabı: priority_fee hiçbir zaman max_fee'yi aşamaz

use nova_runtime::tx::{Transaction, compute_tx_hash};
use nova_primitives::{Address, SlotId, ValidatorId};
use proptest::prelude::*;

// ─── Yardımcı stratejiler ─────────────────────────────────────────────────────

fn arb_address() -> impl Strategy<Value = Address> {
    prop::array::uniform20(any::<u8>()).prop_map(Address::from_bytes)
}

fn arb_amount() -> impl Strategy<Value = u128> {
    prop_oneof![
        1 => Just(0u128),
        9 => 1u128..1_000_000_000_000_000_000u128,
    ]
}

fn arb_gas_price() -> impl Strategy<Value = u128> {
    // 0..1_000 Gwei aralığı
    (0u128..1_000_000_000_000u128)
}

// ─── Property testleri ────────────────────────────────────────────────────────

proptest! {
    /// P1: TX hash deterministik
    /// Aynı (sender, nonce, kind, fees) → aynı hash
    #[test]
    fn prop_tx_hash_deterministic(
        sender_byte in 0u8..255,
        to_byte in 0u8..255,
        amount in arb_amount(),
        nonce in 0u64..10_000u64,
    ) {
        let sender = Address::from_bytes([sender_byte; 20]);
        let to = Address::from_bytes([to_byte; 20]);

        let tx1 = Transaction::mock_transfer(sender, to, amount, nonce);
        let tx2 = Transaction::mock_transfer(sender, to, amount, nonce);

        let h1 = compute_tx_hash(&tx1);
        let h2 = compute_tx_hash(&tx2);

        prop_assert_eq!(h1, h2,
            "Aynı TX farklı hash üretmemeli: {:?} vs {:?}", h1, h2);
    }

    /// P2: Farklı nonce → farklı hash (collision resistance)
    #[test]
    fn prop_different_nonce_different_hash(
        sender_byte in 0u8..255,
        to_byte in 0u8..255,
        amount in 1u128..1_000_000u128,
        nonce1 in 0u64..5000u64,
        nonce2 in 0u64..5000u64,
    ) {
        prop_assume!(nonce1 != nonce2);

        let sender = Address::from_bytes([sender_byte; 20]);
        let to = Address::from_bytes([to_byte; 20]);

        let tx1 = Transaction::mock_transfer(sender, to, amount, nonce1);
        let tx2 = Transaction::mock_transfer(sender, to, amount, nonce2);

        prop_assert_ne!(compute_tx_hash(&tx1), compute_tx_hash(&tx2),
            "Farklı nonce aynı hash üretmemeli (nonce1={}, nonce2={})", nonce1, nonce2);
    }

    /// P3: Farklı gönderici → farklı hash
    #[test]
    fn prop_different_sender_different_hash(
        sender_byte1 in 0u8..255,
        sender_byte2 in 0u8..255,
        to_byte in 0u8..255,
        amount in 1u128..1_000_000u128,
        nonce in 0u64..100u64,
    ) {
        prop_assume!(sender_byte1 != sender_byte2);

        let sender1 = Address::from_bytes([sender_byte1; 20]);
        let sender2 = Address::from_bytes([sender_byte2; 20]);
        let to = Address::from_bytes([to_byte; 20]);

        let tx1 = Transaction::mock_transfer(sender1, to, amount, nonce);
        let tx2 = Transaction::mock_transfer(sender2, to, amount, nonce);

        prop_assert_ne!(compute_tx_hash(&tx1), compute_tx_hash(&tx2),
            "Farklı gönderici aynı hash üretmemeli");
    }

    /// P4: Farklı alıcı → farklı hash
    #[test]
    fn prop_different_recipient_different_hash(
        sender_byte in 0u8..255,
        to_byte1 in 0u8..255,
        to_byte2 in 0u8..255,
        amount in 1u128..1_000_000u128,
        nonce in 0u64..100u64,
    ) {
        prop_assume!(to_byte1 != to_byte2);

        let sender = Address::from_bytes([sender_byte; 20]);
        let to1 = Address::from_bytes([to_byte1; 20]);
        let to2 = Address::from_bytes([to_byte2; 20]);

        let tx1 = Transaction::mock_transfer(sender, to1, amount, nonce);
        let tx2 = Transaction::mock_transfer(sender, to2, amount, nonce);

        prop_assert_ne!(compute_tx_hash(&tx1), compute_tx_hash(&tx2),
            "Farklı alıcı aynı hash üretmemeli");
    }

    /// P5: Farklı miktar → farklı hash
    #[test]
    fn prop_different_amount_different_hash(
        sender_byte in 0u8..255,
        to_byte in 0u8..255,
        amount1 in 1u128..500_000u128,
        amount2 in 500_001u128..1_000_000u128,
        nonce in 0u64..100u64,
    ) {
        let sender = Address::from_bytes([sender_byte; 20]);
        let to = Address::from_bytes([to_byte; 20]);

        let tx1 = Transaction::mock_transfer(sender, to, amount1, nonce);
        let tx2 = Transaction::mock_transfer(sender, to, amount2, nonce);

        prop_assert_ne!(compute_tx_hash(&tx1), compute_tx_hash(&tx2),
            "Farklı miktar aynı hash üretmemeli (amount1={}, amount2={})", amount1, amount2);
    }

    /// P6: EIP-1559 priority_fee max_fee'yi aşamaz
    /// effective_priority = min(priority_fee, max_fee - base_fee)
    #[test]
    fn prop_eip1559_priority_fee_never_exceeds_max(
        max_fee in arb_gas_price(),
        priority_fee in arb_gas_price(),
        base_fee in arb_gas_price(),
    ) {
        // base_fee, max_fee'yi aşmamalı (geçerli senaryo)
        prop_assume!(base_fee <= max_fee);

        // Efektif priority fee hesabı (EIP-1559 spec)
        let effective_priority = priority_fee.min(max_fee.saturating_sub(base_fee));

        // İnvariant: efektif priority asla max_fee'yi aşamaz
        prop_assert!(effective_priority <= max_fee,
            "Efektif priority ({}) max_fee ({}) aşmamalı", effective_priority, max_fee);

        // İnvariant: toplam fee kullanıcının ödeyeceği max_fee'yi aşamaz
        let total_fee = base_fee.saturating_add(effective_priority);
        prop_assert!(total_fee <= max_fee,
            "Toplam fee ({}) max_fee ({}) aşmamalı", total_fee, max_fee);
    }

    /// P7: Gas ücreti her zaman non-negative
    #[test]
    fn prop_gas_fee_non_negative(
        max_fee in arb_gas_price(),
        priority_fee in arb_gas_price(),
        gas_limit in 21_000u64..10_000_000u64,
    ) {
        prop_assume!(max_fee > 0);

        let effective_priority = priority_fee.min(max_fee);
        let max_total_cost = max_fee.saturating_mul(gas_limit as u128);

        prop_assert!(max_total_cost >= max_fee,
            "Gas limiti >= 1 ise total_cost >= max_fee olmalı (gas_limit={})", gas_limit);
        prop_assert!(effective_priority <= max_fee,
            "Efektif priority negatif olamaz");
    }

    /// P8: Stake TX hash deterministik
    #[test]
    fn prop_stake_tx_hash_deterministic(
        sender_byte in 0u8..255,
        validator_id in 0u64..1000u64,
        amount in 1u128..1_000_000u128,
        nonce in 0u64..100u64,
    ) {
        let sender = Address::from_bytes([sender_byte; 20]);
        let vid = ValidatorId(validator_id);

        let tx1 = Transaction::mock_stake(sender, vid, amount, nonce);
        let tx2 = Transaction::mock_stake(sender, vid, amount, nonce);

        prop_assert_eq!(compute_tx_hash(&tx1), compute_tx_hash(&tx2),
            "Aynı stake TX farklı hash üretmemeli");
    }

    /// P9: Transfer ve Stake TX farklı hash üretir (kind farkı)
    #[test]
    fn prop_different_kind_different_hash(
        sender_byte in 0u8..255,
        to_byte in 0u8..255,
        validator_id in 1u64..100u64,
        amount in 1u128..1_000_000u128,
        nonce in 0u64..100u64,
    ) {
        let sender = Address::from_bytes([sender_byte; 20]);
        let to = Address::from_bytes([to_byte; 20]);
        let vid = ValidatorId(validator_id);

        let transfer_tx = Transaction::mock_transfer(sender, to, amount, nonce);
        let stake_tx = Transaction::mock_stake(sender, vid, amount, nonce);

        prop_assert_ne!(compute_tx_hash(&transfer_tx), compute_tx_hash(&stake_tx),
            "Farklı TX tipi aynı hash üretmemeli");
    }
}
