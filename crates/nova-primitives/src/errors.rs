//! Nova Shield Chain — Hata Tipleri
//!
//! Tüm crate'lerin döndürebildiği temel hata türleri.
//! Her crate kendi özel hatalarını `thiserror` ile genişletir,
//! ancak `NovaError` en üst seviye sarmalayıcıdır.

use thiserror::Error;

/// Nova Shield Chain genel `Result` alias'ı.
pub type Result<T> = std::result::Result<T, NovaError>;

/// En üst seviye Nova Shield Chain hatası.
#[derive(Debug, Error)]
pub enum NovaError {
    // ─── KRİPTOGRAFİ HATALARI ──────────────────────────────────────────────
    /// Geçersiz imza (doğrulama başarısız).
    #[error("geçersiz imza: {context}")]
    InvalidSignature {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// Geçersiz MPC key share.
    #[error("geçersiz MPC key share: {context}")]
    InvalidKeyShare {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// MPC quorum sağlanamadı.
    #[error("MPC quorum eksik: {have}/{need} imza var")]
    QuorumNotMet {
        /// Mevcut imza sayısı.
        have: usize,
        /// Gerekli imza sayısı.
        need: usize,
    },

    /// ZK proof doğrulama başarısız.
    #[error("ZK kanıt doğrulama başarısız: {context}")]
    InvalidProof {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// Threshold şifreleme hatası.
    #[error("threshold şifreleme hatası: {context}")]
    ThresholdEncryption {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    // ─── KONSENSÜS HATALARI ──────────────────────────────────────────────────
    /// Blok doğrulama hatası.
    #[error("blok doğrulama hatası slot={slot}: {reason}")]
    BlockValidation {
        /// Hatalı bloğun slot numarası.
        slot: u64,
        /// Doğrulama hata sebebi.
        reason: String,
    },

    /// Çift imza (double sign) tespit edildi.
    #[error("çift imza tespit edildi: validator={validator_id}")]
    DoubleSign {
        /// Çift imza atan validator ID'si.
        validator_id: u64,
    },

    /// Geçersiz slashing kanıtı.
    #[error("geçersiz slashing kanıtı: {context}")]
    InvalidSlashingEvidence {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// Finalize edilmiş bloğa müdahale girişimi.
    #[error("finalize edilmiş bloğu değiştirme girişimi: block={block_hash}")]
    FinalizedBlockMutation {
        /// Müdahale edilen bloğun hash'i.
        block_hash: String,
    },

    // ─── MEMPOOL HATALARI ────────────────────────────────────────────────────
    /// Şifreli TX commitment doğrulama başarısız.
    #[error("commitment doğrulama başarısız: {context}")]
    InvalidCommitment {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// Reveal edilen TX commitment'la eşleşmiyor.
    #[error("reveal/commitment uyumsuzluğu: slot={slot}")]
    CommitmentMismatch {
        /// Uyumsuzluğun oluştuğu slot numarası.
        slot: u64,
    },

    /// Mempool kapasitesi doldu.
    #[error("mempool kapasitesi doldu: max={max} TX")]
    MempoolFull {
        /// Mempool maksimum TX kapasitesi.
        max: usize,
    },

    /// TX slot süresi doldu (replay saldırısı).
    #[error("TX süresi doldu: slot={slot}")]
    TxExpired {
        /// TX'in geçerliliği sona eren slot numarası.
        slot: u64,
    },

    // ─── AUCTION HATALARI ────────────────────────────────────────────────────
    /// Geçersiz emir (miktar sıfır, vb.).
    #[error("geçersiz emir: {reason}")]
    InvalidOrder {
        /// Geçersizlik sebebi.
        reason: String,
    },

    /// Yanlış clearing price (slashable).
    #[error("yanlış clearing price: expected={expected}, got={got}")]
    WrongClearingPrice {
        /// Beklenen clearing fiyatı.
        expected: u128,
        /// Hesaplanan clearing fiyatı.
        got: u128,
    },

    /// Batch auction zaman aşımı.
    #[error("batch auction zaman aşımı: slot={slot}")]
    AuctionTimeout {
        /// Zaman aşımının oluştuğu slot numarası.
        slot: u64,
    },

    // ─── HESAP / MPC HESAP HATALARI ──────────────────────────────────────────
    /// Hesap bulunamadı.
    #[error("hesap bulunamadı: {address}")]
    AccountNotFound {
        /// Bulunamayan hesap adresi.
        address: String,
    },

    /// Hesap dondurulmuş.
    #[error("hesap dondurulmuş: {address}, sebep={reason}")]
    AccountFrozen {
        /// Dondurulmuş hesap adresi.
        address: String,
        /// Dondurma sebebi.
        reason: String,
    },

    /// Yetersiz bakiye.
    #[error("yetersiz bakiye: have={have}, need={need}")]
    InsufficientBalance {
        /// Mevcut bakiye.
        have: u128,
        /// Gerekli bakiye.
        need: u128,
    },

    /// Guardian değişikliği bekleme süresi dolmadı.
    #[error("guardian değişikliği bekleniyor: {remaining_secs} saniye kaldı")]
    GuardianChangeWindow {
        /// Bekleme süresinde kalan saniye.
        remaining_secs: u64,
    },

    /// Geçersiz threshold parametresi.
    #[error("geçersiz threshold: t={t} n={n}")]
    InvalidThreshold {
        /// Gerekli imza sayısı (t).
        t: u8,
        /// Toplam pay sahibi sayısı (n).
        n: u8,
    },

    // ─── MİRAS HATALARI ──────────────────────────────────────────────────────
    /// Miras planı bulunamadı.
    #[error("miras planı bulunamadı: {account}")]
    InheritancePlanNotFound {
        /// Hesap adresi.
        account: String,
    },

    /// Tetikleyici henüz aktif değil.
    #[error("miras tetikleyici henüz aktif değil: {remaining_secs} saniye kaldı")]
    TriggerNotActive {
        /// Aktivasyona kalan saniye.
        remaining_secs: u64,
    },

    /// Miras araştırma penceresi aktif (itiraz süresi).
    #[error("miras itiraz penceresi açık: {remaining_secs} saniye kaldı")]
    InvestigationWindowActive {
        /// Pencere kapanmasına kalan saniye.
        remaining_secs: u64,
    },

    // ─── FREEZE HATALARI ──────────────────────────────────────────────────────
    /// Freeze araştırma penceresi aktif.
    #[error("freeze itiraz penceresi açık: {remaining_secs} saniye kaldı")]
    FreezeWindowActive {
        /// Pencere kapanmasına kalan saniye.
        remaining_secs: u64,
    },

    /// Guardian freeze quorum eksik.
    #[error("freeze quorum eksik: {have}/{need} guardian onayladı")]
    FreezeQuorumNotMet {
        /// Mevcut onay sayısı.
        have: usize,
        /// Gerekli onay sayısı.
        need: usize,
    },

    // ─── VM HATALARI ─────────────────────────────────────────────────────────
    /// WASM çalıştırma hatası.
    #[error("WASM çalıştırma hatası: {context}")]
    WasmExecution {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// EVM çalıştırma hatası.
    #[error("EVM çalıştırma hatası: {context}")]
    EvmExecution {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// Gas limiti aşıldı.
    #[error("gas limiti aşıldı: limit={limit}, used={used}")]
    OutOfGas {
        /// Gas limiti.
        limit: u64,
        /// Kullanılan gas miktarı.
        used: u64,
    },

    // ─── AĞ HATALARI ─────────────────────────────────────────────────────────
    /// P2P bağlantı hatası.
    #[error("P2P bağlantı hatası: {peer}: {reason}")]
    NetworkConnection {
        /// Bağlantı kurulamayan peer adresi.
        peer: String,
        /// Bağlantı hata sebebi.
        reason: String,
    },

    /// Mesaj seri/deserialization hatası.
    #[error("serialization hatası: {context}")]
    Serialization {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    // ─── DEPOLAMA HATALARI ───────────────────────────────────────────────────
    /// RocksDB hatası.
    #[error("depolama hatası: {context}")]
    Storage {
        /// Hata bağlamı açıklaması.
        context: String,
    },

    /// State kök uyuşmazlığı.
    #[error("state root uyuşmazlığı: expected={expected}, got={got}")]
    StateRootMismatch {
        /// Beklenen state root hash'i.
        expected: String,
        /// Hesaplanan state root hash'i.
        got: String,
    },

    // ─── GENEL ────────────────────────────────────────────────────────────────
    /// Beklenmedik dahili hata (asla olmamalı).
    #[error("dahili hata: {0}")]
    Internal(String),

    /// Henüz implement edilmemiş özellik.
    #[error("henüz implement edilmedi: {0}")]
    NotImplemented(String),
}

impl NovaError {
    /// Hata slashable mı? (Validator cezalandırılabilir mi?)
    pub fn is_slashable(&self) -> bool {
        matches!(
            self,
            NovaError::DoubleSign { .. }
                | NovaError::WrongClearingPrice { .. }
                | NovaError::CommitmentMismatch { .. }
        )
    }

    /// Hata geçici mi (tekrar denenebilir)?
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            NovaError::NetworkConnection { .. }
                | NovaError::MempoolFull { .. }
                | NovaError::AuctionTimeout { .. }
        )
    }
}
