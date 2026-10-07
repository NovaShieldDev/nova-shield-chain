//! staking — Validator staking, delegasyon, ödül, unbonding (§5.10, Cosmos/Polkadot'tan ilham)

pub mod delegator;
pub mod historical_cons_addr;
pub mod reward;
pub mod validator;

pub use delegator::{Delegation, DelegatorStore, UnbondingEntry};
pub use historical_cons_addr::{
    derive_cons_addr, ConsAddr, ConsAddrRecord, HistoricalConsAddrRegistry,
};
pub use reward::{
    compute_era_rewards, EraEvent, EraManager, EraRewardDistributor, EraRewardResult,
    ValidatorPointsTracker, ERA_DURATION_SLOTS,
};
pub use validator::{
    BondStatus, ConsKeyRotationManager, KeyRotationError, KeyRotationFeePool, KeyRotationRecord,
    NominatorVote, PhragmenElection, PhragmenResult, Validator, ValidatorPool,
    MAX_ACTIVE_VALIDATORS, MAX_COMMISSION_BPS, MIN_COMMISSION_BPS, UNBONDING_DURATION_SLOTS,
};
