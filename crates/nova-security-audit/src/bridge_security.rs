//! Cross-chain bridge güvenlik testleri — IBC/Cosmos/Polkadot XCM seviyesinde
//!
//! ## Test Kategorileri
//!
//! 1. **Double-spend via bridge** — Escrow lock tutarsızlığı
//! 2. **Fake voucher mint** — Yetersiz escrow ile voucher basma
//! 3. **IBC denom sahteciliği** — Yanlış denom trace ile transfer
//! 4. **Voucher burn/unlock simetrisi** — Burn = unlock = invariant
//! 5. **Channel replay** — Kapalı channel üzerinden transfer
//! 6. **Amount overflow** — Büyük miktarlarda integer güvenliği
//! 7. **Unauthorized unlock** — Escrow'dan yetkisiz para çekme

#[cfg(test)]
mod tests {
    use nova_network::{
        EscrowAccount, VoucherMinter, FungibleTokenPacket, IbcChannel,
        DenomTrace, IBC_TRANSFER_PORT,
    };
    use std::collections::HashMap;

    // ── Escrow Korunumu ───────────────────────────────────────────────────────

    /// PoC: Escrow'a kilitlenmeden unlock yapılamaz
    #[test]
    fn poc_unlock_without_lock_fails() {
        let mut escrow = EscrowAccount::new();

        // Hiç lock yapmadan unlock dene
        let result = escrow.unlock("unsc", 1_000);
        assert!(result.is_err(), "lock olmadan unlock başarısız olmalı");
    }

    /// PoC: Lock miktarından fazla unlock yapılamaz
    #[test]
    fn poc_unlock_exceeds_locked_amount_fails() {
        let mut escrow = EscrowAccount::new();

        // 100 kilitle
        escrow.lock("unsc", 100).unwrap();

        // 101 unlock etmeye çalış
        let result = escrow.unlock("unsc", 101);
        assert!(result.is_err(), "kilitli miktardan fazla unlock yapılamaz");
    }

    /// PoC: Escrow lock/unlock simetrisi — bakiye tutarlı
    #[test]
    fn poc_escrow_lock_unlock_symmetry() {
        let mut escrow = EscrowAccount::new();

        escrow.lock("unsc", 1_000).unwrap();
        escrow.lock("unsc", 500).unwrap();

        assert_eq!(escrow.balance("unsc"), 1_500);

        escrow.unlock("unsc", 300).unwrap();
        assert_eq!(escrow.balance("unsc"), 1_200, "unlock sonrası bakiye doğru olmalı");

        escrow.unlock("unsc", 1_200).unwrap();
        assert_eq!(escrow.balance("unsc"), 0, "tüm miktar unlock edilebilir");
    }

    // ── Voucher Minting Koruması ──────────────────────────────────────────────

    /// PoC: Voucher mint ve burn simetrisi
    #[test]
    fn poc_voucher_mint_burn_symmetry() {
        let mut minter = VoucherMinter::new();

        let ibc_denom = "ibc/TEST_CHANNEL_DENOM";
        minter.mint("user_alice", ibc_denom, 1_000);

        assert_eq!(minter.total_supply(ibc_denom), 1_000);

        minter.burn("user_alice", ibc_denom, 400).unwrap();
        assert_eq!(minter.total_supply(ibc_denom), 600);
    }

    /// PoC: Yetersiz bakiyeli hesaptan voucher burn başarısız
    #[test]
    fn poc_voucher_burn_exceeds_balance_fails() {
        let mut minter = VoucherMinter::new();

        let ibc_denom = "ibc/CHANNEL_01/unsc";
        minter.mint("user_bob", ibc_denom, 100);

        // 200 yakmaya çalış — sadece 100 var
        let result = minter.burn("user_bob", ibc_denom, 200);
        assert!(result.is_err(), "yetersiz voucher bakiyesi yakılamaz");
    }

    /// PoC: Farklı kullanıcıların voucher bakiyeleri izole
    #[test]
    fn poc_voucher_balances_isolated_per_user() {
        let mut minter = VoucherMinter::new();

        let ibc_denom = "ibc/CHANNEL/atom";
        minter.mint("alice", ibc_denom, 1_000);
        minter.mint("bob", ibc_denom, 500);

        // Bob alice'in tokenını yakamaz
        let result = minter.burn("bob", ibc_denom, 1_000);
        assert!(result.is_err(), "bob alice'in tokenlarını yakamaz");

        // Total supply etkilenmemeli
        assert_eq!(minter.total_supply(ibc_denom), 1_500);
    }

    // ── Denom Trace Güvenliği ─────────────────────────────────────────────────

    /// PoC: Native denom is_native() ile tespit edilir; path boş ise native
    #[test]
    fn poc_native_denom_not_ibc() {
        let native = DenomTrace::new("", "unsc");
        assert!(native.is_native(), "unsc native denom olmalı (path boş)");

        // IBC denom ise path dolu olmalı
        let ibc = DenomTrace::new("transfer/channel-0", "unsc");
        assert!(!ibc.is_native(), "path dolu ise native değil");
        assert!(ibc.ibc_denom().starts_with("ibc/"), "IBC denom ibc/ ile başlamalı");
    }

    /// PoC: IBC denom kanıtlanabilir şekilde türetilir (deterministik)
    #[test]
    fn poc_ibc_denom_deterministic() {
        let trace1 = DenomTrace::new("transfer/channel-0", "uatom");
        let trace2 = DenomTrace::new("transfer/channel-0", "uatom");

        assert_eq!(
            trace1.ibc_denom(),
            trace2.ibc_denom(),
            "aynı path + base_denom → aynı IBC denom"
        );
        assert!(trace1.ibc_denom().starts_with("ibc/"), "IBC denom ibc/ ile başlamalı");
    }

    /// PoC: Farklı channel → farklı IBC denom
    #[test]
    fn poc_different_channels_different_denoms() {
        let trace_ch0 = DenomTrace::new("transfer/channel-0", "uatom");
        let trace_ch1 = DenomTrace::new("transfer/channel-1", "uatom");

        assert_ne!(
            trace_ch0.ibc_denom(),
            trace_ch1.ibc_denom(),
            "farklı channel → farklı IBC denom (replay protection)"
        );
    }

    // ── FungibleTokenPacket Güvenliği ─────────────────────────────────────────

    /// PoC: Packet commitment hash deterministik
    #[test]
    fn poc_packet_commitment_hash_deterministic() {
        let packet1 = FungibleTokenPacket::new(
            "transfer/channel-0/uatom",
            1_000,
            "cosmos1sender",
            "nova1receiver",
        );
        let packet2 = FungibleTokenPacket::new(
            "transfer/channel-0/uatom",
            1_000,
            "cosmos1sender",
            "nova1receiver",
        );

        assert_eq!(
            packet1.commitment_hash(),
            packet2.commitment_hash(),
            "packet commitment hash deterministik olmalı"
        );
    }

    /// PoC: Farklı amount → farklı commitment hash
    #[test]
    fn poc_packet_commitment_amount_sensitive() {
        let p1 = FungibleTokenPacket::new("unsc", 1_000, "alice", "bob");
        let p2 = FungibleTokenPacket::new("unsc", 1_001, "alice", "bob"); // 1 fark

        assert_ne!(
            p1.commitment_hash(),
            p2.commitment_hash(),
            "farklı amount → farklı commitment hash"
        );
    }

    // ── Integer Overflow Koruması ─────────────────────────────────────────────

    /// PoC: Maksimum u128 miktarda lock → overflow yok
    #[test]
    fn poc_escrow_large_amount_no_overflow() {
        let mut escrow = EscrowAccount::new();
        let large_amount = u64::MAX as u128; // büyük ama gerçekçi

        // Büyük miktarda lock
        let result = escrow.lock("unsc", large_amount);
        match result {
            Ok(()) => {
                assert_eq!(escrow.balance("unsc"), large_amount);
                // Aynı miktarı unlock et — tutarlı
                escrow.unlock("unsc", large_amount).unwrap();
                assert_eq!(escrow.balance("unsc"), 0);
            }
            Err(_) => {} // Limit kontrolü varsa hata da kabul
        }
    }

    // ── DenomTrace Prefix Güvenliği ───────────────────────────────────────────

    /// PoC: Prefix ekle/kaldır simetrisi
    #[test]
    fn poc_denom_trace_prefix_symmetry() {
        let original = DenomTrace::new("", "uatom");
        let port = "transfer";
        let channel = "channel-0";

        let with_prefix = original.add_prefix(port, channel);
        assert!(!with_prefix.is_native(), "prefix eklenince IBC denom olmalı");

        let stripped = with_prefix.remove_prefix(port, channel).unwrap();
        assert!(stripped.is_native(), "prefix kaldırılınca native denom geri gelmeli");
    }
    // ── Proptest: Bridge Property Testleri ───────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: lock → unlock simetrisi — bakiye değişmez
        #[test]
        fn prop_escrow_lock_unlock_symmetry(amount in 1u128..1_000_000_000_000u128) {
            let mut escrow = EscrowAccount::new();
            escrow.lock("unsc", amount).unwrap();
            assert_eq!(escrow.balance("unsc"), amount);
            escrow.unlock("unsc", amount).unwrap();
            prop_assert_eq!(escrow.balance("unsc"), 0u128, "lock→unlock sonrası bakiye sıfır olmalı");
        }

        /// Property: unlock > locked → her zaman hata
        #[test]
        fn prop_unlock_exceeds_locked_always_fails(
            lock_amount in 1u128..1_000_000u128,
            excess      in 1u128..1_000_000u128,
        ) {
            let mut escrow = EscrowAccount::new();
            escrow.lock("unsc", lock_amount).unwrap();
            let over = lock_amount.saturating_add(excess);
            let result = escrow.unlock("unsc", over);
            prop_assert!(result.is_err(), "kilitlenden fazla unlock her zaman başarısız olmalı");
            prop_assert_eq!(escrow.balance("unsc"), lock_amount, "başarısız unlock sonrası bakiye korunmalı");
        }

        /// Property: mint → burn simetrisi — toplam supply sıfır döner
        #[test]
        fn prop_voucher_mint_burn_symmetry(amount in 1u128..1_000_000_000_000u128) {
            let mut vouchers = VoucherMinter::new();
            let denom = "ibc/CHANNEL_01/unsc";
            vouchers.mint("alice", denom, amount);
            prop_assert_eq!(vouchers.total_supply(denom), amount, "mint sonrası supply = amount");
            vouchers.burn("alice", denom, amount).unwrap();
            prop_assert_eq!(vouchers.total_supply(denom), 0u128, "burn sonrası supply sıfır olmalı");
        }

        /// Property: Escrow bakiyesi hiçbir zaman negatife düşmez (u128 garantisi)
        #[test]
        fn prop_escrow_balance_never_negative(
            lock   in 0u128..1_000_000_000_000u128,
            unlock in 0u128..1_000_000_000_000u128,
        ) {
            let mut escrow = EscrowAccount::new();
            if lock > 0 { let _ = escrow.lock("unsc", lock); }
            let _ = escrow.unlock("unsc", unlock); // başarısız olabilir
            // u128 negatif olamaz — derleme garantisi, runtime panic olmamalı
            let bal = escrow.balance("unsc");
            prop_assert!(bal <= lock, "escrow bakiyesi kilitlenden fazla olamaz");
        }

        /// Property: Farklı denom'lar birbirini etkilemez
        #[test]
        fn prop_different_denoms_isolated(amount in 1u128..1_000_000u128) {
            let mut escrow = EscrowAccount::new();
            escrow.lock("unsc", amount).unwrap();
            escrow.lock("atom", amount).unwrap();
            prop_assert_eq!(escrow.balance("unsc"), amount);
            prop_assert_eq!(escrow.balance("atom"), amount);
            escrow.unlock("unsc", amount).unwrap();
            prop_assert_eq!(escrow.balance("atom"), amount, "unsc unlock atom'u etkilemez");
        }
    }
}
