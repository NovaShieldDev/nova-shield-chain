//! nova-crypto'ya özel hata tipleri.

use thiserror::Error;

/// nova-crypto Result alias.
pub type CryptoResult<T> = Result<T, CryptoError>;

/// Kriptografik işlem hataları.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// BLAKE3 hashing hatası (pratikte olmamalı).
    #[error("hash hatası: {0}")]
    Hash(String),

    /// ECDSA anahtar oluşturma hatası.
    #[error("ECDSA keygen hatası: {0}")]
    EcdsaKeygen(String),

    /// ECDSA imzalama hatası.
    #[error("ECDSA sign hatası: {0}")]
    EcdsaSign(String),

    /// ECDSA doğrulama başarısız.
    #[error("ECDSA doğrulama başarısız: {0}")]
    EcdsaVerify(String),

    /// Dilithium3 anahtar oluşturma hatası.
    #[error("Dilithium keygen hatası: {0}")]
    DilithiumKeygen(String),

    /// Dilithium3 imzalama hatası.
    #[error("Dilithium sign hatası: {0}")]
    DilithiumSign(String),

    /// Dilithium3 doğrulama başarısız.
    #[error("Dilithium doğrulama başarısız: {0}")]
    DilithiumVerify(String),

    /// BLS anahtar oluşturma hatası.
    #[error("BLS keygen hatası: {0}")]
    BlsKeygen(String),

    /// BLS imzalama hatası.
    #[error("BLS sign hatası: {0}")]
    BlsSign(String),

    /// BLS doğrulama başarısız.
    #[error("BLS doğrulama başarısız: {0}")]
    BlsVerify(String),

    /// BLS aggregate hatası.
    #[error("BLS aggregate hatası: {0}")]
    BlsAggregate(String),

    /// VRF işlem hatası.
    #[error("VRF hatası: {0}")]
    Vrf(String),

    /// Pedersen commitment hatası.
    #[error("commitment hatası: {0}")]
    Commitment(String),

    /// Threshold şifreleme hatası.
    #[error("threshold şifreleme hatası: {0}")]
    ThresholdEncrypt(String),

    /// Threshold çözme hatası.
    #[error("threshold çözme hatası: {0}")]
    ThresholdDecrypt(String),

    /// ZK kanıt üretme hatası.
    #[error("ZK kanıt üretme hatası: {0}")]
    ZkProve(String),

    /// ZK kanıt doğrulama başarısız.
    #[error("ZK kanıt doğrulama başarısız: {0}")]
    ZkVerify(String),

    /// AES-GCM şifreleme hatası.
    #[error("AES-GCM şifreleme hatası: {0}")]
    AesGcmEncrypt(String),

    /// AES-GCM çözme hatası.
    #[error("AES-GCM çözme hatası: {0}")]
    AesGcmDecrypt(String),

    /// Geçersiz anahtar boyutu.
    #[error("geçersiz anahtar boyutu: expected={expected}, got={got}")]
    InvalidKeySize {
        /// Beklenen anahtar boyutu (byte).
        expected: usize,
        /// Alınan anahtar boyutu (byte).
        got: usize,
    },

    /// Geçersiz imza boyutu.
    #[error("geçersiz imza boyutu: expected={expected}, got={got}")]
    InvalidSignatureSize {
        /// Beklenen imza boyutu (byte).
        expected: usize,
        /// Alınan imza boyutu (byte).
        got: usize,
    },

    /// Geçersiz parametre.
    #[error("geçersiz parametre: {0}")]
    InvalidParameter(String),

    /// Merkle kanıtı doğrulama başarısız.
    #[error("Merkle kanıtı geçersiz")]
    InvalidMerkleProof,

    /// HKDF anahtar türetme hatası.
    #[error("HKDF türetme hatası: {0}")]
    Kdf(String),
}
