//! İtibar modülü: Trust Score, kurtarma geçmişi, slash kaydı

pub mod history;
pub mod slash_record;
pub mod trust_score;

pub use history::{RecoveryHistory, RecoveryHistoryStore, RecoveryOutcome, RecoveryRecord};
pub use slash_record::{
    GuardianSlashRecord, SlashEvent, SlashReason, SlashRecordStore,
    JAIL_DURATION_SLOTS, MAX_WRONG_FREEZES_30_DAYS, THIRTY_DAYS_SLOTS,
    WRONG_FREEZE_SLASH_BPS,
};
pub use trust_score::{TrustScoreCalculator, TrustScoreComponents};
