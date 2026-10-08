//! GuardianError — nova-guardian hata türleri

#[derive(Debug, Clone, thiserror::Error)]
pub enum GuardianError {
    #[error("guardian zaten kayıtlı: {address_hex}")]
    AlreadyRegistered { address_hex: String },

    #[error("guardian bulunamadı")]
    NotFound,

    #[error("yetersiz stake: gerekli={required}, mevcut={provided}")]
    InsufficientStake { required: u128, provided: u128 },

    #[error("guardian jailed (askıya alınmış)")]
    GuardianJailed,

    #[error("teklif bulunamadı")]
    OfferNotFound,

    #[error("sözleşme bulunamadı")]
    ContractNotFound,

    #[error("sigorta talebi reddedildi: {reason}")]
    ClaimRejected { reason: String },

    #[error("sigorta fonu yetersiz: gerekli={required}, mevcut={available}")]
    InsufficientInsuranceFund { required: u128, available: u128 },

    #[error("geçersiz kategori: kurumsal guardian 10_000 NSC stake gerektirir")]
    InvalidCategory,

    #[error("SLA ihlali: {detail}")]
    SlaViolation { detail: String },

    #[error("maksimum guardian kapasitesi aşıldı")]
    MaxCapacityExceeded,

    #[error("ücret modeli geçersiz")]
    InvalidFeeModel,

    #[error("sigorta talebi bulunamadı")]
    ClaimNotFound,

    #[error("geçersiz talep durumu")]
    InvalidClaimStatus,

    #[error("geçersiz parametre: {0}")]
    InvalidParameter(String),

    #[error("yetkisiz erişim: bu işlem için yetki yok")]
    Unauthorized,
}

pub type GuardianResult<T> = Result<T, GuardianError>;
