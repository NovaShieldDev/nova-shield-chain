//! ZkError — nova-zk hata türleri

#[derive(Debug, Clone, thiserror::Error)]
pub enum ZkError {
    #[error("note bulunamadı: {commitment_hex}")]
    NoteNotFound { commitment_hex: String },

    #[error("nullifier zaten harcanmış")]
    NullifierAlreadySpent,

    #[error("geçersiz ZK kanıtı")]
    InvalidProof,

    #[error("commitment zaten mevcut")]
    CommitmentAlreadyExists,

    /// NOVA-RED-ZK-002: Transfer girdi commitment'ı pool'da kayıtlı değil
    #[error("commitment pool'da bulunamadı — var olmayan note harcanamaz")]
    CommitmentNotFound,

    #[error("yetersiz shielded bakiye: gerekli={required}, mevcut={available}")]
    InsufficientShieldedBalance { required: u128, available: u128 },

    #[error("geçersiz spending key")]
    InvalidSpendingKey,

    #[error("değer taşması")]
    ValueOverflow,

    #[error("kanıt üretimi başarısız: {0}")]
    ProofGenerationFailed(String),

    #[error("havuz dolu: maksimum {max_capacity} note")]
    PoolFull { max_capacity: usize },

    #[error("değer dengesizliği: girdi={input}, çıktı={output}")]
    ValueImbalance { input: u128, output: u128 },

    #[error("geçersiz diversifier")]
    InvalidDiversifier,

    /// Production ortamında mock/geliştirme kanıtı kullanım girişimi
    #[error("production ortamında mock ZK setup kullanılamaz; gerçek MPC ceremony keylerini kullanın")]
    MockSetupInProduction,

    /// Production için gerçek trusted setup gerekli
    #[error("production ZK keyler henüz yüklenmedi; `load_production_keys()` çağrısı gerekli")]
    ProductionSetupRequired,
}

pub type ZkResult<T> = Result<T, ZkError>;
