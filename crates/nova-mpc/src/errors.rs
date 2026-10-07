//! nova-mpc hata tipleri.
//!
//! Tüm MPC protokol hataları bu modülde tanımlanır.
//! §6.1 — CGGMP21 Identifiable Abort: kötü niyetli taraf hem tespit
//!         edilir hem cezalandırılır. `IdentifiableAbort` varyantı bunu taşır.

use thiserror::Error;

/// MPC protokol hataları.
#[derive(Debug, Error)]
pub enum MpcError {
    // ─── Parametre hataları ────────────────────────────────────────────────────

    /// Geçersiz taraf indeksi (1..=n arası olmalı).
    #[error("geçersiz taraf indeksi: {index} (maksimum {max})")]
    InvalidPartyIndex { index: u32, max: u32 },

    /// Geçersiz threshold parametresi (0 < t ≤ n).
    #[error("geçersiz threshold: t={t} n={n} (0 < t ≤ n zorunlu)")]
    InvalidThreshold { t: u8, n: u8 },

    /// Beklenen ile farklı sayıda pay.
    #[error("yanlış pay sayısı: beklenen={expected} gelen={got}")]
    InvalidShareCount { expected: usize, got: usize },

    /// Beklenen ile farklı sayıda imzacı.
    #[error("yanlış imzacı sayısı: beklenen={expected} gelen={got}")]
    InvalidSignerCount { expected: usize, got: usize },

    // ─── VSS hataları ─────────────────────────────────────────────────────────

    /// Feldman VSS doğrulaması başarısız — pay taahhütle uyuşmuyor.
    #[error("VSS doğrulaması başarısız: taraf {party} için pay geçersiz")]
    VssVerificationFailed { party: u32 },

    /// Taahhüt açılımı uyuşmuyor (Round 1 commit vs Round 2 decommit).
    #[error("taahhüt uyuşmazlığı: taraf {party} round1 ve round2 uyuşmuyor")]
    CommitmentMismatch { party: u32 },

    /// VSS sabit terimi beklenen public key ile uyuşmuyor (reshare doğrulama).
    #[error("reshare doğrulaması başarısız: public key değişmiş")]
    ResharePublicKeyMismatch,

    // ─── ZK kanıt hataları ────────────────────────────────────────────────────

    /// Schnorr DLog kanıtı geçersiz.
    #[error("DLog kanıtı geçersiz: taraf {party}")]
    DlogProofInvalid { party: u32 },

    /// İmza doğrulaması başarısız.
    #[error("imza doğrulaması başarısız")]
    SignatureInvalid,

    // ─── Kriptografik hatalar ─────────────────────────────────────────────────

    /// Geçersiz scalar (sıfır veya alan dışı).
    #[error("geçersiz scalar değeri (sıfır veya alan dışı)")]
    InvalidScalar,

    /// Geçersiz eliptik eğri noktası.
    #[error("geçersiz eliptik eğri noktası (sonsuz nokta veya bozuk encoding)")]
    InvalidPoint,

    /// Scalar ters elemanı hesaplanamadı (sıfır scalar).
    #[error("scalar ters hesaplanamadı: sıfır scalar")]
    ScalarInvertFailed,

    // ─── Protokol akış hataları ───────────────────────────────────────────────

    /// Yetersiz pay sayısı (threshold karşılanamıyor).
    #[error("yetersiz pay: gerekli={needed} mevcut={got}")]
    InsufficientShares { needed: usize, got: usize },

    /// Session ID uyuşmazlığı (cross-session replay koruması).
    #[error("session ID uyuşmazlığı: beklenen={} gelen={}", hex::encode(expected), hex::encode(got))]
    SessionMismatch { expected: Vec<u8>, got: Vec<u8> },

    /// Beklenmedik round numaralı mesaj.
    #[error("beklenmedik mesaj: round={round} gönderen={from}")]
    UnexpectedMessage { round: u32, from: u32 },

    /// Aynı taraftan yinelenen mesaj.
    #[error("yinelenen mesaj: round={round} gönderen={from}")]
    DuplicateMessage { round: u32, from: u32 },

    /// Round için mesaj eksik.
    #[error("mesaj eksik: round={round} taraf={from}")]
    MissingMessage { round: u32, from: u32 },

    /// Round tamamlanmadı — bazı taraflar cevap bekliyor.
    #[error("round tamamlanmadı: round={round} bekleniyor={waiting_for:?}")]
    RoundNotComplete { round: u32, waiting_for: Vec<u32> },

    // ─── Identifiable Abort (§6.1 CGGMP21) ───────────────────────────────────

    /// Protokol iptal edildi — kötü niyetli taraflar tespit edildi.
    /// Birden fazla taraf aynı anda kötü davranıyorsa hepsi listelenir.
    #[error("protokol iptal: suçlular={culprits:?} sebep={reason}")]
    ProtocolAbort { culprits: Vec<u32>, reason: String },

    /// Tek bir taraf tespit edilerek iptal — matematiksel kanıt içerir.
    /// §6.1: "kötü niyetli taraf hem tespit edilir hem cezalandırılır"
    #[error("identifiable abort: suçlu={culprit} kanıt={}", hex::encode(evidence))]
    IdentifiableAbort { culprit: u32, evidence: Vec<u8> },

    // ─── Pre-signing hataları ─────────────────────────────────────────────────

    /// Pre-signature süresi dolmuş (slot bazlı TTL).
    #[error("pre-signature süresi dolmuş: slot {slot}")]
    PreSignatureExpired { slot: u64 },

    /// Pre-signature zaten kullanılmış (double-use koruması).
    #[error("pre-signature zaten kullanılmış")]
    PreSignatureAlreadyUsed,

    /// Pre-signature listesi boş �� önceden hazırlanmamış.
    #[error("kullanılabilir pre-signature yok, önce presign aşaması çalıştırılmalı")]
    NoPreSignatureAvailable,

    // ─��─ Guardian hataları ────────────────────────────────────────────────────

    /// Guardian bulunamadı.
    #[error("guardian bulunamadı: id={id}")]
    GuardianNotFound { id: u32 },

    /// Guardian iptal edilmiş durumda.
    #[error("guardian iptal edilmiş: id={id}")]
    GuardianRevoked { id: u32 },

    /// Guardian threshold karşılanamıyor.
    #[error("guardian threshold karşılanamıyor: gerekli={required} mevcut={available}")]
    GuardianThresholdNotMet { required: u8, available: u8 },

    // ─── Simetrik kriptografi hataları ───────────────────────────────────────

    /// P2P mesaj şifreleme başarısız.
    #[error("şifreleme başarısız: {0}")]
    EncryptionFailed(String),

    /// P2P mesaj şifre çözme başarısız (MAC doğrulama hatası dahil).
    #[error("şifre çözme başarısız: {0}")]
    DecryptionFailed(String),

    // ─── Genel hatalar ────────────────────────────────────────────────────────

    /// Geçersiz giriş parametresi.
    #[error("geçersiz giriş: {0}")]
    InvalidInput(String),

    /// Serializasyon/deserializasyon hatası.
    #[error("serializasyon hatası: {0}")]
    SerializationFailed(String),

    /// Beklenmedik iç hata (bug göstergesi).
    #[error("iç hata: {0}")]
    InternalError(String),
}

/// MPC sonuç tipi.
pub type MpcResult<T> = Result<T, MpcError>;
