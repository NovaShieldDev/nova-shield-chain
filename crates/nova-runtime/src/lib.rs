//! nova-runtime — Zincir runtime çekirdeği (§5.9, §7, §8)
//!
//! Modüller:
//! - `errors`     — RuntimeError
//! - `token`      — NSC token, emisyon, fee dağıtımı, yakma
//! - `tx`         — Transaction tipleri, EIP-1559 fee, doğrulama, account keys
//! - `staking`    — Validator staking, delegasyon, ödül, unbonding
//! - `governance` — Öneri, oylama, tally
//! - `state`      — Hesap state, DashMap cache
//! - `executor`   — Paralel TX yürütme (Sealevel)
//! - `block`      — Blok yaşam döngüsü
//! - `upgrade`    — Forkless runtime upgrade (Polkadot)

pub mod block;
pub mod errors;
pub mod executor;
pub mod governance;
pub mod qos;
pub mod staking;
pub mod state;
pub mod storage;
pub mod token;
pub mod tx;
pub mod upgrade;
pub mod virtual_account;

// ─── Re-exports ────────────────────────────────────────────────────────────────

pub use block::{BlockPhase, RuntimeBlock, RuntimeBlockHeader};
pub use errors::{RuntimeError, RuntimeResult};
pub use executor::{ParallelExecutor, TxResult};
pub use governance::{
    Proposal, ProposalStatus, ProposalStore, TallyEngine, TallyResult, QUORUM_BPS, THRESHOLD_BPS,
    VETO_THRESHOLD_BPS,
};
pub use staking::{
    BondStatus, Delegation, DelegatorStore, EraRewardResult, Validator, ValidatorPool,
    ValidatorPointsTracker, UNBONDING_DURATION_SLOTS,
};
pub use state::{AccountState, AccountStateCache, GenesisAccount};
pub use storage::{
    AccountSlot, AppendVec, AppendVecStore, StoredAccountMeta,
    RocksDbStore, StateBackend, StorageError, StorageResult,
    StoredBlock, StoredTx, WriteBatch,
};
pub use token::{
    distribute_protocol_fee, era_emission, BurnTracker, EmissionSchedule, F1RewardTracker,
    FeeDistribution, NscLedger, NSC_DECIMALS, NSC_TOTAL_SUPPLY, PROTOCOL_FEE_BPS,
};
pub use tx::{
    calc_next_base_fee, compute_tx_hash, BaseFeeOracle, SignedTransaction, Transaction,
    TransactionKind, TxValidator, VoteOption, BLOCK_GAS_LIMIT, INITIAL_BASE_FEE,
};
pub use qos::{
    BlockCostAccumulator, QosDecision, QosService, TxCostBreakdown, TxCostModel, TxForQos,
    DATA_BYTES_UNITS, DEFAULT_COMPUTE_LIMIT, MAX_BLOCK_UNITS, MAX_COMPUTE_LIMIT,
    SIGNATURE_COST, WRITE_LOCK_UNITS,
};
pub use upgrade::{RuntimeVersion, ScheduledUpgrade, UpgradeManager, MIN_UPGRADE_DELAY_SLOTS};
pub use virtual_account::{
    AccountPermission, VirtualAccount, VirtualAccountError, VirtualAccountKind,
    VirtualAccountRegistry, BURN_MODULE_ADDR, MPC_MODULE_ADDR,
    STAKING_MODULE_ADDR, TREASURY_MODULE_ADDR,
};

// ─── Integration Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{Address, EpochId, SlotId, ValidatorId};
    use std::sync::Arc;

    // ─── Yardımcılar ──────────────────────────────────────────────────────────

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn funded_state(accounts: &[(u8, u128)]) -> Arc<AccountStateCache> {
        let state = Arc::new(AccountStateCache::new());
        for &(n, balance) in accounts {
            state.set_balance(addr(n), balance);
        }
        state
    }

    // ─── Token: NSC mint + transfer + burn ────────────────────────────────────

    #[test]
    fn test_nsc_full_lifecycle() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 1_000 * NSC_DECIMALS).unwrap();
        ledger.transfer(addr(1), addr(2), 300 * NSC_DECIMALS).unwrap();
        ledger.burn(addr(2), 100 * NSC_DECIMALS).unwrap();

        assert_eq!(ledger.balance_of(&addr(1)), 700 * NSC_DECIMALS);
        assert_eq!(ledger.balance_of(&addr(2)), 200 * NSC_DECIMALS);
        assert_eq!(ledger.total_burned(), 100 * NSC_DECIMALS);
        assert_eq!(ledger.total_supply(), 900 * NSC_DECIMALS);
    }

    // ─── Fee: EIP-1559 base fee hesabı ────────────────────────────────────────

    #[test]
    fn test_base_fee_convergence() {
        let mut oracle = BaseFeeOracle::new();
        // Tam bloklar → fee artar
        for _ in 0..10 {
            oracle.update(BLOCK_GAS_LIMIT);
        }
        assert!(oracle.current_fee() > INITIAL_BASE_FEE);

        // Boş bloklar → fee azalır
        for _ in 0..20 {
            oracle.update(0);
        }
        assert!(oracle.current_fee() < INITIAL_BASE_FEE);
    }

    // ─── Token: Fee dağıtımı ──────────────────────────────────────────────────

    #[test]
    fn test_protocol_fee_distribution_sum() {
        let dist = distribute_protocol_fee(10_000_000);
        assert_eq!(dist.total(), 10_000_000);
        assert_eq!(dist.validators, 3_000_000); // %30
        assert_eq!(dist.stakers, 3_000_000);    // %30
        assert_eq!(dist.ecosystem, 2_500_000);  // %25
        assert_eq!(dist.treasury, 1_500_000);   // %15
    }

    // ─── Staking: Validator kaydı ve aktivasyon ───────────────────────────────

    #[test]
    fn test_staking_validator_lifecycle() {
        use crate::token::nsc::MIN_VALIDATOR_STAKE_NSC;

        let pool = ValidatorPool::new();
        let v = Validator::new(
            ValidatorId(1),
            addr(1),
            MIN_VALIDATOR_STAKE_NSC,
            500,
            MIN_VALIDATOR_STAKE_NSC,
            0,
        );

        // Başlangıçta Unbonded
        assert!(v.status.is_unbonded());

        // Validator seti güncelleme → Bonded olur
        pool.register(v.clone()).unwrap();
        pool.apply_validator_set_update(SlotId(0));
        let bonded = pool.get(&ValidatorId(1)).unwrap();
        assert!(bonded.status.is_bonded());

        // Unbonding başlat
        pool.update(&ValidatorId(1), |v| { let _ = v.begin_unbonding(SlotId(1_000)); });
        let unbonding = pool.get(&ValidatorId(1)).unwrap();
        assert!(unbonding.status.is_unbonding());

        // Tamamlanıp tamamlanmadığı kontrolü
        assert!(unbonding.is_unbonding_complete(SlotId(1_000 + UNBONDING_DURATION_SLOTS)));
    }

    // ─── Staking: Delegasyon + F1 ödül ───────────────────────────────────────

    #[test]
    fn test_delegation_and_f1_reward() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), ValidatorId(1), 1_000, 0).unwrap();
        store.delegate(addr(2), ValidatorId(1), 1_000, 0).unwrap();

        let mut tracker = store.get_or_create_tracker(ValidatorId(1));
        tracker.set_total_stake(2_000);
        tracker.add_reward(2_000);
        store.update_tracker(ValidatorId(1), tracker);

        let r1 = store.pending_reward(&addr(1), &ValidatorId(1));
        let r2 = store.pending_reward(&addr(2), &ValidatorId(1));
        assert_eq!(r1, 1_000);
        assert_eq!(r2, 1_000);
    }

    // ─── Governance: Teklif → Oylama → Geçti ─────────────────────────────────

    #[test]
    fn test_governance_full_flow() {
        use crate::governance::proposal::{MIN_DEPOSIT_NSC, VOTING_PERIOD_SLOTS};
        use crate::tx::types::ProposalPayload;

        let store = ProposalStore::new();
        let pid = store.next_proposal_id();
        let p = Proposal::new(
            pid,
            addr(1),
            "Nova Upgrade".to_string(),
            "v2.0.0'a yükselt".to_string(),
            ProposalPayload::UpgradeRuntime { new_version: 0x020000 },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(p);

        // Oylar
        store.cast_vote(pid, addr(2), VoteOption::Yes, 600, 600, SlotId(100)).unwrap();
        store.cast_vote(pid, addr(3), VoteOption::No, 200, 200, SlotId(100)).unwrap();
        store.cast_vote(pid, addr(4), VoteOption::Abstain, 200, 200, SlotId(100)).unwrap();

        // Süresi dol
        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), 1_000);
        assert!(finalized.contains(&pid));

        let proposal = store.get(pid).unwrap();
        // Yes=600, Non-abstain=800, Yes% = 75% > 50% → Geçti
        assert!(matches!(proposal.status, ProposalStatus::Passed));
    }

    // ─── Governance: Veto ────────────────────────────────────────────────────

    #[test]
    fn test_governance_veto() {
        use crate::governance::proposal::{MIN_DEPOSIT_NSC, VOTING_PERIOD_SLOTS};
        use crate::tx::types::ProposalPayload;

        let store = ProposalStore::new();
        let pid = store.next_proposal_id();
        let p = Proposal::new(
            pid,
            addr(1),
            "Tartışmalı".to_string(),
            "…".to_string(),
            ProposalPayload::Text { content: "içerik".to_string() },
            MIN_DEPOSIT_NSC,
            0,
        );
        store.add_proposal(p);

        // %35 NoWithVeto → veto eşiği aşıldı
        store.cast_vote(pid, addr(2), VoteOption::NoWithVeto, 350, 350, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(3), VoteOption::Yes, 350, 350, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(4), VoteOption::No, 300, 300, SlotId(1)).unwrap();

        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), 1_000);
        assert!(!finalized.is_empty());

        let proposal = store.get(pid).unwrap();
        assert!(matches!(proposal.status, ProposalStatus::Vetoed));
    }

    // ─── Executor: Paralel TX yürütme ─────────────────────────────────────────

    #[test]
    fn test_parallel_execution_full() {
        // Gerçek ECDSA imzalı TX'ler — seed_byte farklı senderlar üretir
        let (tx1, s1) = Transaction::signed_for_test(51, addr(10), 100 * NSC_DECIMALS, 0);
        let (tx2, s2) = Transaction::signed_for_test(52, addr(11), 200 * NSC_DECIMALS, 0);
        let (tx3, s3) = Transaction::signed_for_test(53, addr(12), 300 * NSC_DECIMALS, 0);

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(s1, 1_000 * NSC_DECIMALS);
        state.set_balance(s2, 1_000 * NSC_DECIMALS);
        state.set_balance(s3, 1_000 * NSC_DECIMALS);

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx1, tx2, tx3]);
        assert!(results.iter().all(|r| r.success),
            "tüm TX başarılı olmalı: {:?}", results.iter().filter(|r| !r.success).map(|r| &r.error).collect::<Vec<_>>());
        assert_eq!(state.balance(&addr(10)), 100 * NSC_DECIMALS);
        assert_eq!(state.balance(&addr(11)), 200 * NSC_DECIMALS);
        assert_eq!(state.balance(&addr(12)), 300 * NSC_DECIMALS);
    }

    // ─── Block: Tam blok pipeline ─────────────────────────────────────────────

    #[test]
    fn test_block_full_pipeline() {
        let mut block = RuntimeBlock::open(
            1,
            SlotId(10),
            EpochId(0),
            [0u8; 32],
            ValidatorId(1),
            INITIAL_BASE_FEE,
        );

        let (tx, _sender) = Transaction::signed_for_test(60, addr(2), 100, 0);
        block.add_transaction(tx).unwrap();

        let state_root = [42u8; 32];
        block.freeze(state_root, vec![]).unwrap();
        assert_eq!(block.phase, BlockPhase::Frozen);

        block.finalize().unwrap();
        assert!(block.is_finalized());
        assert_eq!(block.header.height, 1);
        assert_eq!(block.header.state_root, state_root);
    }

    // ─── Upgrade: Forkless upgrade pipeline ───────────────────────────────────

    #[test]
    fn test_runtime_upgrade_pipeline() {
        use crate::upgrade::GOVERNANCE_MODULE_ADDRESS;

        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let gov = Address::from_bytes(GOVERNANCE_MODULE_ADDRESS);

        // Upgrade zamanla
        let activation = MIN_UPGRADE_DELAY_SLOTS + 500;
        mgr.schedule_upgrade(RuntimeVersion::new(2, 0, 0), activation, 1, 0, gov).unwrap();
        assert!(mgr.pending_upgrade().is_some());

        // Henüz aktif değil
        assert!(mgr.try_apply(100).is_none());
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 0, 0));

        // Aktivasyon slot'unda → uygula
        let new_version = mgr.try_apply(activation);
        assert!(new_version.is_some());
        assert_eq!(mgr.current_version(), RuntimeVersion::new(2, 0, 0));
        assert_eq!(mgr.applied_count(), 1);
    }

    // ─── Emisyon: Yıl 0 → Yıl 12 azalış ────────────────────────────────────

    #[test]
    fn test_emission_schedule_full() {
        let mut sched = EmissionSchedule::new();
        let era0 = sched.advance_era();
        assert!(era0 > 0);

        // Yıl 12 (era_to_year kullanalım)
        use crate::token::emission::{era_emission, emission_rate_bps, ERAS_PER_YEAR};
        let rate_y0 = emission_rate_bps(0);
        let rate_y12 = emission_rate_bps(12);
        assert_eq!(rate_y0, 800); // %8
        assert_eq!(rate_y12, 200); // %2
        assert!(era_emission(NSC_TOTAL_SUPPLY, 0) > era_emission(NSC_TOTAL_SUPPLY, 12));
    }

    // ─── Burn tracker ─────────────────────────────────────────────────────────

    #[test]
    fn test_burn_tracker_integration() {
        use crate::token::burn::split_base_fee;

        let tracker = BurnTracker::new();
        let fee = 2_000_000_000u128; // 2 Gwei
        let burned = tracker.record_burn(fee, addr(1), 100);
        let (expected_burn, _) = split_base_fee(fee);
        assert_eq!(burned, expected_burn);
        assert_eq!(tracker.total_burned(), expected_burn);
    }

    // ─── State snapshot ve restore ────────────────────────────────────────────

    #[test]
    fn test_state_snapshot_restore() {
        let state = AccountStateCache::new();
        state.set_balance(addr(1), 5_000);
        state.set_balance(addr(2), 3_000);
        let snap = state.snapshot();

        state.set_balance(addr(1), 0);
        state.restore(snap);

        assert_eq!(state.balance(&addr(1)), 5_000);
        assert_eq!(state.balance(&addr(2)), 3_000);
    }
}
