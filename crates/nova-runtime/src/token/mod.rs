//! token — NOVA token modülleri (§7, §10)

pub mod burn;
pub mod emission;
pub mod era_payout;
pub mod fee_dist;
pub mod nsc;
pub mod vesting;

pub use burn::{split_base_fee, BurnEvent, BurnTracker, BURN_RATE_BPS, VALIDATOR_FEE_RATE_BPS};
pub use era_payout::{
    EraPayoutEngine, EraPayoutInput, EraPayoutResult, ValidatorEraReward,
    FALLOFF_RATE_BPS, IDEAL_APY_BPS, IDEAL_STAKE_RATIO_BPS, MAX_COMMISSION_BPS,
};
pub use emission::{
    era_emission, era_to_year, slot_reward, EmissionSchedule, ERAS_PER_YEAR,
    INITIAL_EMISSION_BPS, MIN_EMISSION_BPS, SLOTS_PER_ERA, SLOTS_PER_YEAR,
};
pub use fee_dist::{
    distribute_protocol_fee, validator_fee_share, DelegatorRewardInfo, F1RewardTracker,
    FeeDistribution, ECOSYSTEM_SHARE_BPS, PROTOCOL_FEE_BPS, STAKER_SHARE_BPS,
    TREASURY_SHARE_BPS, VALIDATOR_SHARE_BPS,
};
pub use nsc::{
    genesis_allocation, NscLedger, ALLOC_ECOSYSTEM_BPS, ALLOC_INVESTORS_BPS, ALLOC_STAKING_BPS,
    ALLOC_TEAM_BPS, ALLOC_TREASURY_BPS, MIN_DELEGATION_NSC, MIN_GOV_DEPOSIT_NSC,
    MIN_VALIDATOR_STAKE_NSC, NSC_DECIMALS, NSC_TOTAL_SUPPLY,
};
pub use vesting::{
    VestingSchedule, VestingState, EPOCHS_PER_MONTH, EPOCHS_PER_YEAR,
};
