//! Nova DEX hata türleri

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DexError {
    #[error("pool bulunamadı: {pool_id}")]
    PoolNotFound { pool_id: String },

    #[error("yetersiz likidite")]
    InsufficientLiquidity,

    #[error("yetersiz bakiye")]
    InsufficientBalance,

    #[error("slippage toleransı aşıldı: çıktı {got} < minimum {min}")]
    SlippageExceeded { got: u128, min: u128 },

    #[error("kilitleme süresi dolmadı: {remaining_ms} ms kaldı")]
    LockNotExpired { remaining_ms: u64 },

    #[error("kimlik doğrulama başarısız: {reason}")]
    Unauthorized { reason: String },

    #[error("geçersiz imza")]
    InvalidSignature,

    #[error("commit bulunamadı: {commit_hash}")]
    CommitNotFound { commit_hash: String },

    #[error("commit zaten var: {commit_hash}")]
    CommitAlreadyExists { commit_hash: String },

    #[error("reveal uyuşmuyor: commit={commit_hash}")]
    RevealMismatch { commit_hash: String },

    #[error("slot dolmadı: commit slot {commit_slot} >= current {current_slot}")]
    SlotNotExpired { commit_slot: u64, current_slot: u64 },

    #[error("sıfır miktar geçersiz")]
    ZeroAmount,

    #[error("oracle fiyatı mevcut değil")]
    OracleUnavailable,

    #[error("pool oluşturma ücreti yetersiz: gerekli {required} nNOVA, ödenen {paid}")]
    InsufficientCreationFee { required: u128, paid: u128 },

    #[error("lp token yetersiz")]
    InsufficientLpTokens,

    #[error("payout yetkilendirmesi başarısız")]
    PayoutUnauthorized,

    #[error("dahili hata: {reason}")]
    Internal { reason: String },
}

pub type DexResult<T> = Result<T, DexError>;
