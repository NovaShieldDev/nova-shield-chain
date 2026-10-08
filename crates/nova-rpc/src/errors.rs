//! RPC hata türleri

use thiserror::Error;

/// JSON-RPC standart hata kodları
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// Nova Shield özel hata kodları
pub const ACCOUNT_NOT_FOUND: i64 = -32001;
pub const BLOCK_NOT_FOUND: i64 = -32002;
pub const TX_NOT_FOUND: i64 = -32003;
pub const GUARDIAN_NOT_FOUND: i64 = -32004;
pub const INSUFFICIENT_BALANCE: i64 = -32005;
/// NOVA-RED-RPC-002/003/004: İmza yokluğu veya format hatası
pub const INVALID_SIGNATURE: i64 = -32006;

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("ayrıştırma hatası")]
    ParseError,

    #[error("geçersiz istek")]
    InvalidRequest,

    #[error("metod bulunamadı: {method}")]
    MethodNotFound { method: String },

    #[error("geçersiz parametreler: {reason}")]
    InvalidParams { reason: String },

    #[error("dahili hata: {reason}")]
    InternalError { reason: String },

    #[error("hesap bulunamadı: {address}")]
    AccountNotFound { address: String },

    #[error("blok bulunamadı: {id}")]
    BlockNotFound { id: String },

    #[error("işlem bulunamadı: {hash}")]
    TxNotFound { hash: String },

    #[error("guardian bulunamadı")]
    GuardianNotFound,

    #[error("yetersiz bakiye")]
    InsufficientBalance,

    /// NOVA-RED-RPC-002/003/004: İmza eksik veya geçersiz format
    #[error("geçersiz imza: {reason}")]
    InvalidSignature { reason: String },
}

impl RpcError {
    pub fn code(&self) -> i64 {
        match self {
            RpcError::ParseError => PARSE_ERROR,
            RpcError::InvalidRequest => INVALID_REQUEST,
            RpcError::MethodNotFound { .. } => METHOD_NOT_FOUND,
            RpcError::InvalidParams { .. } => INVALID_PARAMS,
            RpcError::InternalError { .. } => INTERNAL_ERROR,
            RpcError::AccountNotFound { .. } => ACCOUNT_NOT_FOUND,
            RpcError::BlockNotFound { .. } => BLOCK_NOT_FOUND,
            RpcError::TxNotFound { .. } => TX_NOT_FOUND,
            RpcError::GuardianNotFound => GUARDIAN_NOT_FOUND,
            RpcError::InsufficientBalance => INSUFFICIENT_BALANCE,
            RpcError::InvalidSignature { .. } => INVALID_SIGNATURE,
        }
    }
}

pub type RpcResult<T> = Result<T, RpcError>;
