//! governance — On-chain yönetişim (§5.10, Cosmos SDK x/gov'dan ilham)

pub mod proposal;
pub mod tally;
pub mod voting;

pub use proposal::{
    Proposal, ProposalStatus, TallyResult, Vote, DEPOSIT_PERIOD_SLOTS, MIN_DEPOSIT_NSC,
    VOTING_PERIOD_SLOTS,
};
pub use tally::{
    TallyEngine, EXPEDITED_THRESHOLD_BPS, QUORUM_BPS, THRESHOLD_BPS, VETO_THRESHOLD_BPS,
};
pub use voting::ProposalStore;
