//! Cross-modül entegrasyon saldırı testleri
//!
//! Gerçek A+++ audit: tek modül değil, modüller arası kombinasyon saldırıları.
//!
//! ## Senaryolar
//!
//! 1. **Bridge + Governance** — Bridge drain önerisi governance'ı atlayamaz
//! 2. **ZK + Bridge** — ZK mint amount ile bridge escrow tutarlı olmalı
//! 3. **TX + Nonce manipulation** — Aynı nonce'lu TX replay koruması
//! 4. **Consensus + Slashing** — Double-sign evidence doğru işlenir
//! 5. **ZK nullifier + Bridge** — Nullifier ile çift çekme önlenir
//! 6. **Voucher inflation** — Escrow olmadan voucher basma işe yaramaz
//! 7. **Paralel TX state corruption** — Bakiye tutarlılığı korunur

#[cfg(test)]
mod tests {
    use nova_runtime::{
        AccountStateCache, ParallelExecutor, INITIAL_BASE_FEE,
        NscLedger, Transaction, Proposal, ProposalStore, ProposalStatus,
        VoteOption, governance::proposal::{MIN_DEPOSIT_NSC, VOTING_PERIOD_SLOTS},
        tx::types::ProposalPayload,
    };
    use nova_primitives::{Address, Balance, SlotId, ValidatorId, BlockHash};
    use nova_network::{EscrowAccount, VoucherMinter};
    use nova_zk::{MintWitness, Note, NullifierKey, NullifierSet, NSC_ASSET_ID};
    use nova_consensus::{
        ValidatorRegistry, SlashingEngine, DoubleSignEvidence,
    };
    use std::sync::Arc;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_note(value: u128, r: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [r; 32], [0u8; 32])
    }

    // ── Senaryo 1: Bridge + Governance ───────────────────────────────────────

    /// Saldırı: Bridge drain önerisi → governance quorum'suz geçemez
    /// Gerçek dünya: Ronin Bridge hack — governance zayıflığı + bridge birlikte
    #[test]
    fn attack_bridge_drain_via_governance_blocked() {
        let store = ProposalStore::new();
        let pid = store.next_proposal_id();

        let drain = Proposal::new(
            pid,
            addr(99),
            "Emergency Bridge Drain".to_string(),
            "security vulnerability".to_string(),
            ProposalPayload::TreasurySpend {
                recipient: addr(99),
                amount: 1_000_000_000_000_000_000_000u128,
            },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(drain);

        // Saldırgan tek başına oy kullanıyor — quorum yetmez
        let total_bonded = 10_000_000u128;
        store.cast_vote(pid, addr(99), VoteOption::Yes, 1_000, 1_000, SlotId(1)).unwrap();

        let _ = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), total_bonded);
        let proposal = store.get(pid).unwrap();

        assert!(
            !matches!(proposal.status, ProposalStatus::Passed),
            "bridge drain önergesi quorum olmadan geçemez"
        );
    }

    // ── Senaryo 2: ZK + Bridge Uyumu ─────────────────────────────────────────

    /// Invariant: ZK mint kanıtı değeri bridge escrow ile eşleşmeli
    /// Gerçek dünya: Wormhole exploit — shielded pool ile bridge amount mismatch
    #[test]
    fn attack_zk_bridge_amount_consistency() {
        let mut escrow = EscrowAccount::new();
        let mut vouchers = VoucherMinter::new();

        let bridge_amount = 1_000u128;
        escrow.lock("unsc", bridge_amount).unwrap();

        let ibc_denom = "ibc/CHANNEL_00/unsc";
        vouchers.mint("alice", ibc_denom, bridge_amount);

        // ZK mint: aynı miktarı shielded pool'a aktar
        let note = make_note(bridge_amount, 0x11);
        let witness = MintWitness::new(note);
        let proof = witness.prove(SlotId(1)).expect("ZK mint kanıtı üretilmeli");
        assert!(proof.verify().unwrap(), "ZK mint kanıtı doğrulanmalı");

        // Üç taraf tutarlı: escrow = voucher supply = ZK note value
        assert_eq!(escrow.balance("unsc"), bridge_amount);
        assert_eq!(vouchers.total_supply(ibc_denom), bridge_amount);
        assert_eq!(proof.statement.value, bridge_amount);
    }

    /// Saldırı: Escrow'dan kilitlenden fazla unlock yapılamaz
    #[test]
    fn attack_bridge_over_unlock_blocked() {
        let mut escrow = EscrowAccount::new();
        escrow.lock("unsc", 500).unwrap();

        // 500'den fazla unlock dene
        let result = escrow.unlock("unsc", 1_000);
        assert!(result.is_err(), "kilitlenden fazla unlock yapılamaz");
        assert_eq!(escrow.balance("unsc"), 500, "bakiye korunmalı");
    }

    // ── Senaryo 3: TX Replay / Nonce Manipulation ────────────────────────────

    /// Saldırı: Aynı nonce'lu iki TX → birden fazlası geçemez (replay koruması)
    #[test]
    fn attack_nonce_replay_double_spend_blocked() {
        let state = Arc::new(AccountStateCache::new());
        let (tx_nonce0, sender) = Transaction::signed_for_test(1, addr(10), 100_000, 0);
        let (tx_nonce0_dup, _) = Transaction::signed_for_test(1, addr(11), 100_000, 0);

        state.set_balance(sender, 500_000u128);

        let executor = ParallelExecutor::new(Arc::clone(&state), INITIAL_BASE_FEE);
        let results = executor.execute_block(&[tx_nonce0, tx_nonce0_dup]);
        let successes = results.iter().filter(|r| r.success).count();

        assert!(
            successes <= 1,
            "aynı nonce'lu iki TX birden geçemez: {successes}"
        );

        // İki alıcı aynı anda para alamaz
        let got_10 = state.balance(&addr(10));
        let got_11 = state.balance(&addr(11));
        assert!(
            got_10 == 0 || got_11 == 0,
            "iki hedef aynı anda 100k alamaz: got_10={got_10}, got_11={got_11}"
        );
    }

    // ── Senaryo 4: Consensus + Slashing ──────────────────────────────────────

    /// Invariant: Double-sign evidence doğru işlenir, sistem çökmez
    #[test]
    fn attack_double_sign_evidence_processed_safely() {
        let registry = Arc::new(ValidatorRegistry::new());
        // Minimum stake: 100_000 NSC × 10^18
        let min_stake = Balance::from(100_000u128 * 1_000_000_000_000_000_000u128);
        registry.register(
            ValidatorId(1),
            addr(1),
            min_stake,
            500,
            [1u8; 32],
            SlotId(0),
        ).unwrap();

        let engine = SlashingEngine::new(Arc::clone(&registry));

        let evidence = DoubleSignEvidence {
            validator: ValidatorId(1),
            height: 100,
            block_hash_a: BlockHash::from([1u8; 32]),
            block_hash_b: BlockHash::from([2u8; 32]),
            submitted_at_slot: SlotId(100),
        };

        // Evidence işlendiğinde ValidatorId(1) tombstone edilmeli
        let result = engine.handle_double_sign(evidence, SlotId(100));
        assert!(result.is_ok(), "double-sign evidence işlenmeli: {:?}", result.err());

        // Saldırgan validator tombstone edilmeli
        assert!(engine.is_tombstoned(ValidatorId(1)), "double-sign sonrası validator tombstone edilmeli");

        // Alakasız validator etkilenmemeli
        assert!(!engine.is_tombstoned(ValidatorId(99)));
    }

    // ── Senaryo 5: ZK Nullifier + Bridge Çift Çekme ──────────────────────────

    /// Saldırı: Aynı ZK nota iki kez bridge üzerinden withdraw edilemez
    /// Gerçek dünya: Zcash nullifier bypass — cryptographic double-spend
    #[test]
    fn attack_zk_nullifier_prevents_bridge_double_withdraw() {
        let nullifier_set = NullifierSet::new();
        let nk = NullifierKey([0xABu8; 32]);
        let note = make_note(1_000, 0x55);
        let commitment = note.commit();
        let nullifier = nk.derive_nullifier(&commitment);

        // İlk withdraw: nullifier kayıt et
        nullifier_set.insert(nullifier, SlotId(1))
            .expect("ilk nullifier başarılı");

        // İkinci withdraw: aynı nullifier → reddedilmeli
        let second = nullifier_set.insert(nullifier, SlotId(2));
        assert!(second.is_err(), "aynı nota iki kez çekilemez");
    }

    // ── Senaryo 6: Voucher Inflation ──────────────────────────────────────────

    /// Saldırı: Escrow kilitlemeden voucher basıp çekme girişimi (Mango exploit benzeri)
    #[test]
    fn attack_voucher_inflation_without_escrow_blocked() {
        let mut escrow = EscrowAccount::new();
        let mut vouchers = VoucherMinter::new();

        let ibc_denom = "ibc/FAKE/unsc";

        // Escrow'a HİÇ para kilitlemeden sahte voucher bas
        vouchers.mint("attacker", ibc_denom, 1_000_000_000);

        // Escrow boş — unlock yapılamaz
        let unlock = escrow.unlock("unsc", 1_000_000_000);
        assert!(unlock.is_err(), "escrow boşken unlock yapılamaz");
        assert_eq!(escrow.balance("unsc"), 0, "escrow sıfır kalmalı");

        // Voucher burn da escrow'u değiştirmez
        vouchers.burn("attacker", ibc_denom, 1_000_000_000).unwrap();
        assert_eq!(escrow.balance("unsc"), 0, "voucher burn escrow'u etkilememeli");
    }

    // ── Senaryo 7: Paralel TX State Bütünlüğü ────────────────────────────────

    /// Invariant: Paralel yürütme sonrası bakiye negatife düşmez, toplam korunur
    #[test]
    fn attack_parallel_tx_no_state_corruption() {
        let state = Arc::new(AccountStateCache::new());

        // 5 hesap, her biri 200k
        let mut txs = Vec::new();
        let mut senders = Vec::new();
        for i in 1u8..=5 {
            let (tx, s) = Transaction::signed_for_test(i, addr(i + 10), 50_000, 0);
            state.set_balance(s, 200_000u128);
            txs.push(tx);
            senders.push(s);
        }

        let executor = ParallelExecutor::new(Arc::clone(&state), INITIAL_BASE_FEE);
        let results = executor.execute_block(&txs);
        assert_eq!(results.len(), 5, "5 TX için 5 sonuç olmalı");

        // Hiçbir gönderici başlangıç bakiyesini aşmamalı (u128 — negatif olamaz)
        for (i, s) in senders.iter().enumerate() {
            let bal = state.balance(s);
            assert!(
                bal <= 200_000,
                "gönderici {} bakiyesi başlangıcı aşmamalı: {bal}", i + 1
            );
        }

        // Alıcılar gönderilenden fazla alamaz
        for i in 11u8..=15 {
            let bal = state.balance(&addr(i));
            assert!(bal <= 50_000, "alıcı {i} fazla almış: {bal}");
        }
    }

    // ── Senaryo 8: NscLedger + TX Pipeline Tutarlılığı ────────────────────────

    /// Invariant: Ledger ve state cache aynı miktarı izlemeli (çift kayıt tutarlılığı)
    #[test]
    fn invariant_ledger_and_state_consistent() {
        let ledger = NscLedger::new();
        let state = Arc::new(AccountStateCache::new());

        let alice = addr(1);
        let bob = addr(2);
        let amount = 500_000u128;

        // İki sistemi senkron tut
        ledger.mint(alice, amount).unwrap();
        state.set_balance(alice, amount);

        assert_eq!(ledger.balance_of(&alice), state.balance(&alice));

        // Transfer — ikisini de güncelle
        let transfer = 200_000u128;
        ledger.transfer(alice, bob, transfer).unwrap();
        let new_alice = state.balance(&alice) - transfer;
        let new_bob = state.balance(&bob) + transfer;
        state.set_balance(alice, new_alice);
        state.set_balance(bob, new_bob);

        // İkisi tutarlı olmalı
        assert_eq!(ledger.balance_of(&alice), state.balance(&alice),
            "alice ledger ve state tutarlı");
        assert_eq!(ledger.balance_of(&bob), state.balance(&bob),
            "bob ledger ve state tutarlı");

        // Toplam değişmedi
        assert_eq!(
            ledger.balance_of(&alice) + ledger.balance_of(&bob),
            amount,
            "transfer sonrası toplam korunmalı"
        );
    }
}
