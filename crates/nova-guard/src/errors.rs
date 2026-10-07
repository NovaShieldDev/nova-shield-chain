//! nova-guard hata türleri

use thiserror::Error;

pub type GuardResult<T> = Result<T, GuardError>;

#[derive(Debug, Clone, Error, PartialEq)]
pub enum GuardError {
    #[error("desteklenmayan zincir: {chain}")]
    UnsupportedChain { chain: String },

    #[error("vault bulunamadı: {vault_id}")]
    VaultNotFound { vault_id: u64 },

    #[error("MPC imza başarısız: {reason}")]
    MpcSignatureFailed { reason: String },

    #[error("yetersiz guardian: {got}/{needed}")]
    InsufficientGuardians { got: usize, needed: usize },

    #[error("geçersiz adres: {addr}")]
    InvalidAddress { addr: String },

    #[error("geçersiz işlem: {reason}")]
    InvalidTransaction { reason: String },

    #[error("IBC kanalı bulunamadı: {channel_id}")]
    IbcChannelNotFound { channel_id: String },

    #[error("BEEFY taahhüdü geçersiz: {reason}")]
    BeefyCommitmentInvalid { reason: String },

    #[error("miras planı bulunamadı")]
    InheritancePlanNotFound,

    #[error("fee hesaplama hatası: {reason}")]
    FeeCalculationError { reason: String },

    #[error("serileştirme hatası: {reason}")]
    SerializationError { reason: String },

    #[error("broadcast başarısız: {reason}")]
    BroadcastFailed { reason: String },

    #[error("bakiye yetersiz: mevcut {available}, gerekli {required}")]
    InsufficientBalance { available: u64, required: u64 },
}
