//! RuntimeError — nova-runtime hata tipleri

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    // ─── Transaction Hataları ─────────────────────────────────────────────
    #[error("Geçersiz imza: {0}")]
    InvalidSignature(String),

    #[error("Hatalı nonce: beklenen {expected}, gelen {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Yetersiz bakiye: gereken {required}, mevcut {available}")]
    InsufficientBalance { required: u128, available: u128 },

    #[error("Gas limiti aşıldı: kullanılan {used}, limit {limit}")]
    GasLimitExceeded { used: u64, limit: u64 },

    #[error("Base fee yetersiz: minimum {min_fee}, sunulan {provided}")]
    BaseFeeTooLow { min_fee: u128, provided: u128 },

    #[error("İşlem boyutu çok büyük: {size} bayt (maksimum {max})")]
    TransactionTooLarge { size: usize, max: usize },

    #[error("İşlem süresi doldu: slot {tx_slot}, geçerli slot {current_slot}, maksimum yaş {max_age}")]
    TransactionExpired { tx_slot: u64, current_slot: u64, max_age: u64 },

    #[error("İşlem çok ileri tarihli: slot {tx_slot}, geçerli slot {current_slot}, maksimum ileri {max_future}")]
    TransactionTooFuture { tx_slot: u64, current_slot: u64, max_future: u64 },

    #[error("Compute budget aşıldı: kullanılan {used}, limit {limit}")]
    ComputeBudgetExceeded { used: u64, limit: u64 },

    #[error("Hesap yazma kilidi alınamadı: hesap meşgul")]
    AccountWriteLockContention,

    #[error("Upgrade hash uyuşmuyor: beklenen {expected}, gelen {got}")]
    UpgradeHashMismatch { expected: String, got: String },

    #[error("Upgrade henüz yetkilendirilmedi")]
    UpgradeNotAuthorized,

    // ─── Staking Hataları ─────────────────────────────────────────────────
    #[error("Validator zaten kayıtlı")]
    ValidatorAlreadyExists,

    #[error("Validator bulunamadı")]
    ValidatorNotFound,

    #[error("Yetersiz stake: minimum {min}, sunulan {provided}")]
    InsufficientStake { min: u128, provided: u128 },

    #[error("Jailed validator: unbond isteği {until_slot} slot'una kadar engellenmiştir")]
    ValidatorJailed { until_slot: u64 },

    #[error("Unbonding zaten aktif")]
    UnbondingAlreadyActive,

    #[error("Henüz unbonding tamamlanmadı: {remaining_slots} slot kaldı")]
    UnbondingNotComplete { remaining_slots: u64 },

    #[error("Delegatör bulunamadı")]
    DelegatorNotFound,

    #[error("Delegasyon bulunamadı")]
    DelegationNotFound,

    #[error("Ödül zaten talep edildi: era {era}")]
    RewardAlreadyClaimed { era: u64 },

    // ─── Governance Hataları ──────────────────────────────────────────────
    #[error("Öneri bulunamadı: {id}")]
    ProposalNotFound { id: u64 },

    #[error("Oylama süresi doldu")]
    VotingPeriodExpired,

    /// NOVA-RED-RUNTIME-002: Talep edilen oy gücü stake'i aşıyor
    #[error("Aşırı oy gücü: talep={claimed} maksimum={max_allowed}")]
    ExcessiveVotingPower { claimed: u128, max_allowed: u128 },

    #[error("Oylama henüz başlamadı")]
    VotingNotStarted,

    #[error("Bu adres zaten oy kullandı")]
    AlreadyVoted,

    #[error("Quorum sağlanamadı")]
    QuorumNotMet,

    #[error("Yetersiz depozit: minimum {min}, sunulan {provided}")]
    InsufficientDeposit { min: u128, provided: u128 },

    #[error("Öneri yürütme hatası: {reason}")]
    ProposalExecutionFailed { reason: String },

    // ─── Token Hataları ───────────────────────────────────────────────────
    #[error("Toplam arz limitine ulaşıldı")]
    MaxSupplyReached,

    #[error("Yakma için yetersiz bakiye")]
    InsufficientBurnBalance,

    #[error("Kilitli bakiye: {locked} token vesting kilidinde, kullanılabilir {available}, istenen {requested}")]
    VestingLockedTransfer { locked: u128, available: u128, requested: u128 },

    // ─── State Hataları ───────────────────────────────────────────────────
    #[error("Hesap bulunamadı")]
    AccountNotFound,

    #[error("State yazma hatası: {reason}")]
    StateWriteError { reason: String },

    // ─── Blok Hataları ────────────────────────────────────────────────────
    #[error("Blok zaten mühürlü")]
    BlockAlreadySealed,

    #[error("Blok henüz açık değil")]
    BlockNotOpen,

    #[error("Hatalı blok yüksekliği: beklenen {expected}, gelen {got}")]
    InvalidBlockHeight { expected: u64, got: u64 },

    // ─── Upgrade Hataları ─────────────────────────────────────────────────
    #[error("Upgrade yetki hatası")]
    UpgradeUnauthorized,

    #[error("Geçersiz runtime versiyonu")]
    InvalidRuntimeVersion,

    #[error("Upgrade henüz uygulanamaz: {remaining_slots} slot kaldı")]
    UpgradeNotReady { remaining_slots: u64 },
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;
