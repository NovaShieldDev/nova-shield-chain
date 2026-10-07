//! nova-mempool hata tipleri.
//!
//! Tüm mempool operasyonları bu hataları döndürür.
//! Her hata türü hangi modülden/protokol adımından geldiğini açıkça belirtir.

use nova_primitives::types::SlotId;
use thiserror::Error;

/// Mempool operasyonu için sonuç tipi.
pub type MempoolResult<T> = Result<T, MempoolError>;

/// Tüm mempool hataları.
#[derive(Debug, Error)]
pub enum MempoolError {
    // ─── Şifreleme Hataları ───────────────────────────────────────────────────

    /// TX şifreleme işlemi başarısız. §5.2.2
    #[error("TX şifreleme başarısız: {0}")]
    EncryptionFailed(String),

    /// TX şifre çözme başarısız. §5.2.2
    #[error("TX şifre çözme başarısız: {0}")]
    DecryptionFailed(String),

    /// AES-GCM şifreleme hatası.
    #[error("AES-GCM şifreleme hatası: {0}")]
    AesEncryptionError(String),

    /// AES-GCM şifre çözme hatası (authentication tag geçersiz).
    #[error("AES-GCM şifre çözme hatası (tag geçersiz): {0}")]
    AesDecryptionError(String),

    /// Eğri noktası decompress edilemedi (geçersiz Ristretto noktası).
    #[error("Ristretto noktası geçersiz: {0}")]
    InvalidCurvePoint(String),

    // ─── Threshold Hataları ───────────────────────────────────────────────────

    /// Threshold şifre çözme için yeterli share yok. §6.1.4
    #[error("Yetersiz decryption share: gerekli={required} mevcut={available}")]
    InsufficientDecryptionShares { required: u8, available: usize },

    /// Share index geçersiz (0 yasak, n'yi aşıyor).
    #[error("Geçersiz share index: {index} (n={n})")]
    InvalidShareIndex { index: u8, n: u8 },

    /// Threshold public key geçersiz.
    #[error("Geçersiz threshold public key: {0}")]
    InvalidThresholdPublicKey(String),

    /// Decryption share doğrulama başarısız.
    #[error("Decryption share geçersiz (share index={index}): {reason}")]
    InvalidDecryptionShare { index: u8, reason: String },

    // ─── Commitment Hataları ──────────────────────────────────────────────────

    /// TX commitment hash doğrulama başarısız. §5.2.2
    #[error("Commitment doğrulama başarısız: beklenen={expected} hesaplanan={computed}")]
    CommitmentMismatch { expected: String, computed: String },

    /// Commitment verisi geçersiz format.
    #[error("Geçersiz commitment formatı: {0}")]
    InvalidCommitmentFormat(String),

    // ─── TX Geçerlilik Hataları ───────────────────────────────────────────────

    /// Aynı TX hash zaten havuzda. §5.2.2
    #[error("Yinelenen TX: {tx_hash}")]
    DuplicateTransaction { tx_hash: String },

    /// TX imzası geçersiz. §5.1.3
    #[error("Geçersiz TX imzası: {0}")]
    InvalidSignature(String),

    /// TX imzası uzunluğu yanlış.
    #[error("Geçersiz imza uzunluğu: beklenen={expected} alınan={got}")]
    InvalidSignatureLength { expected: usize, got: usize },

    /// TX gönderici adresi geçersiz.
    #[error("Geçersiz gönderici adresi")]
    InvalidSenderAddress,

    /// TX nonce sırası bozuk. §5.1.3
    #[error("Nonce uyuşmazlığı: beklenen={expected} gelen={got}")]
    NonceMismatch { expected: u64, got: u64 },

    /// TX bakiye yetersiz.
    #[error("Yetersiz bakiye: gerekli={required} mevcut={available}")]
    InsufficientBalance { required: u128, available: u128 },

    /// Gas limiti aşıldı.
    #[error("Gas limiti geçersiz: verilen={given} maksimum={maximum}")]
    GasLimitExceeded { given: u64, maximum: u64 },

    /// Gas fiyatı minimum altında.
    #[error("Gas fiyatı çok düşük: verilen={given} minimum={minimum}")]
    GasPriceTooLow { given: u128, minimum: u128 },

    // ─── Slot / Zamanlama Hataları ────────────────────────────────────────────

    /// TX hedef slot geçmiş. §5.2.2 (slot_number commit'e dahil)
    #[error("TX slot geçmiş: tx_slot={tx_slot} mevcut={current}")]
    ExpiredSlot { tx_slot: SlotId, current: SlotId },

    /// TX slot çok ileride (senkronizasyon sorunu).
    #[error("TX slot çok ileri: tx_slot={tx_slot} mevcut={current} maksimum_ilerlik={max_future}")]
    SlotTooFarInFuture {
        tx_slot: SlotId,
        current: SlotId,
        max_future: u64,
    },

    /// Slot açılması (reveal) henüz başlamamış.
    #[error("Slot {0} açılma penceresi henüz başlamadı")]
    RevealWindowNotOpen(SlotId),

    /// Slot açılması (reveal) zaten tamamlandı.
    #[error("Slot {0} zaten açıldı")]
    SlotAlreadyRevealed(SlotId),

    // ─── Padding Hataları ─────────────────────────────────────────────────────

    /// TX boyutu maksimum padding boyutunu aştı. §6.4.2
    #[error("TX boyutu çok büyük: {size} byte > maksimum {maximum} byte")]
    TxTooLarge { size: usize, maximum: usize },

    /// Padding kaldırma başarısız (bozuk veri).
    #[error("Padding kaldırma başarısız: {0}")]
    UnpadError(String),

    // ─── Havuz Kapasitesi ─────────────────────────────────────────────────────

    /// Mempool doldu. §5.9.1 (MAX_TX_PER_SLOT)
    #[error("Mempool dolu: {current}/{maximum} TX")]
    MempoolFull { current: usize, maximum: usize },

    /// Slot havuzu doldu.
    #[error("Slot {slot} havuzu dolu: {current}/{maximum} TX")]
    SlotPoolFull {
        slot: SlotId,
        current: usize,
        maximum: usize,
    },

    // ─── Seri/Deserializasyon Hataları ───────────────────────────────────────

    /// TX serializasyon hatası.
    #[error("TX serializasyon hatası: {0}")]
    SerializationError(String),

    /// TX deserializasyon hatası.
    #[error("TX deserializasyon hatası: {0}")]
    DeserializationError(String),

    // ─── Dahili Hatalar ───────────────────────────────────────────────────────

    /// Kriptografi kütüphanesi hatası.
    #[error("Kriptografi hatası: {0}")]
    CryptoError(String),

    /// Genel TX format/içerik hatası (decryption sonrası doğrulama). §5.1.3
    #[error("Geçersiz TX: {0}")]
    InvalidTx(String),

    /// Beklenmeyen dahili hata.
    #[error("Dahili hata: {0}")]
    Internal(String),
}

impl From<nova_crypto::errors::CryptoError> for MempoolError {
    fn from(e: nova_crypto::errors::CryptoError) -> Self {
        MempoolError::CryptoError(e.to_string())
    }
}

impl From<borsh::io::Error> for MempoolError {
    fn from(e: borsh::io::Error) -> Self {
        MempoolError::SerializationError(e.to_string())
    }
}

// Not: borsh::io::Error = std::io::Error olduğundan From<std::io::Error>
// ayrıca implemente edilmez — çakışma yaratır.

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hata_mesajlari_okunakli() {
        let hata = MempoolError::InsufficientDecryptionShares {
            required: 3,
            available: 1,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("3"), "gerekli share sayısı mesajda olmalı");
        assert!(mesaj.contains("1"), "mevcut share sayısı mesajda olmalı");
    }

    #[test]
    fn test_expired_slot_hatasi() {
        let hata = MempoolError::ExpiredSlot {
            tx_slot: SlotId(100),
            current: SlotId(200),
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("100"), "tx_slot mesajda olmalı");
        assert!(mesaj.contains("200"), "current slot mesajda olmalı");
    }

    #[test]
    fn test_commitment_mismatch_hatasi() {
        let hata = MempoolError::CommitmentMismatch {
            expected: "aabbcc".to_string(),
            computed: "ddeeff".to_string(),
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("aabbcc"));
        assert!(mesaj.contains("ddeeff"));
    }

    #[test]
    fn test_mempool_full_hatasi() {
        let hata = MempoolError::MempoolFull {
            current: 10_000,
            maximum: 10_000,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("10000"), "kapasite bilgisi mesajda olmalı");
    }

    #[test]
    fn test_nonce_mismatch_hatasi() {
        let hata = MempoolError::NonceMismatch { expected: 5, got: 3 };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("5"));
        assert!(mesaj.contains("3"));
    }

    #[test]
    fn test_tx_too_large_hatasi() {
        let hata = MempoolError::TxTooLarge {
            size: 5000,
            maximum: 4096,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("5000"));
        assert!(mesaj.contains("4096"));
    }

    #[test]
    fn test_invalid_share_index() {
        let hata = MempoolError::InvalidShareIndex { index: 0, n: 5 };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("0"));
        assert!(mesaj.contains("5"));
    }

    #[test]
    fn test_crypto_error_donusum() {
        let crypto_err = nova_crypto::errors::CryptoError::InvalidParameter("test".into());
        let mempool_err: MempoolError = crypto_err.into();
        assert!(
            matches!(mempool_err, MempoolError::CryptoError(_)),
            "CryptoError → MempoolError::CryptoError dönüşümü çalışmalı"
        );
    }

    #[test]
    fn test_debug_impl() {
        let hata = MempoolError::InvalidSignature("test".into());
        let debug_str = format!("{hata:?}");
        assert!(!debug_str.is_empty(), "Debug impl çalışmalı");
    }
}
