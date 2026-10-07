//! Nova VM hata türleri

#[derive(Debug, Clone, thiserror::Error)]
pub enum VmError {
    #[error("geçersiz WASM bytecode: {reason}")]
    InvalidBytecode { reason: String },

    #[error("kontrat bulunamadı: {address_hex}")]
    ContractNotFound { address_hex: String },

    #[error("kontrat zaten mevcut: {address_hex}")]
    ContractAlreadyExists { address_hex: String },

    #[error("yürütme başarısız: {reason}")]
    ExecutionFailed { reason: String },

    #[error("gas tükendi: limit={limit}, kullanılan={used}")]
    OutOfGas { limit: u64, used: u64 },

    #[error("bellek sınırı aşıldı: max={max_bytes}")]
    MemoryLimitExceeded { max_bytes: usize },

    #[error("geçersiz giriş verisi")]
    InvalidInput,

    #[error("izin reddedildi: {reason}")]
    PermissionDenied { reason: String },

    #[error("yığın taşması: derinlik={depth}")]
    StackOverflow { depth: u32 },

    #[error("host fonksiyonu hatası: {name}: {reason}")]
    HostFunctionError { name: String, reason: String },

    #[error("ABI decode hatası: {reason}")]
    AbiDecodeError { reason: String },

    #[error("EVM hata: {reason}")]
    EvmError { reason: String },

    #[error("kontrat panik yaptı: {message}")]
    ContractPanic { message: String },
}

pub type VmResult<T> = Result<T, VmError>;
