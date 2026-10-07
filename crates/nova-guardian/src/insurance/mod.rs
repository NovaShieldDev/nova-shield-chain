//! Sigorta modülü: fon, talep, ödeme

pub mod claim;
pub mod fund;
pub mod payout;

pub use claim::{ClaimId, ClaimReason, ClaimStatus, ClaimStore, InsuranceClaim};
pub use fund::{FundContribution, InsuranceFund};
pub use payout::{InsurancePayout, PayoutRecord};
