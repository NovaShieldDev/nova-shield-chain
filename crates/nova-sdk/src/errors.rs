//! SDK hata türleri

use thiserror::Error;

pub type SdkResult<T> = Result<T, SdkError>;

#[derive(Debug, Clone, Error, PartialEq)]
pub enum SdkError {
    #[error("RPC isteği başarısız: {reason}")]
    RpcError { reason: String },

    #[error("hesap bulunamadı: {address}")]
    AccountNotFound { address: String },

    #[error("imzalama başarısız: {reason}")]
    SigningError { reason: String },

    #[error("geçersiz parametre: {param} — {reason}")]
    InvalidParameter { param: String, reason: String },

    #[error("bakiye yetersiz: {available}, gerekli: {required}")]
    InsufficientBalance { available: u64, required: u64 },

    #[error("guardian bulunamadı")]
    GuardianNotFound,

    #[error("miras planı geçersiz: {reason}")]
    InvalidPlan { reason: String },

    #[error("batch işlemi başarısız: {reason}")]
    BatchError { reason: String },

    #[error("bağlantı hatası: {endpoint}")]
    ConnectionError { endpoint: String },

    #[error("zaman aşımı")]
    Timeout,

    #[error("serileştirme hatası: {reason}")]
    SerializationError { reason: String },
}
