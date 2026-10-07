//! Guardian kayıt modülü: kategoriler, kayıt, doğrulama

pub mod categories;
pub mod registration;
pub mod verification;

pub use categories::{
    GuardianCategory, KycStatus, MIN_STAKE_CERTIFIED_INDIVIDUAL, MIN_STAKE_DAO,
    MIN_STAKE_INSTITUTIONAL,
};
pub use registration::{Guardian, GuardianRegistry, GuardianStatus};
pub use verification::{KycVerifier, SlaTerms};
