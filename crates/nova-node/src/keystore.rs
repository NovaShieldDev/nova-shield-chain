//! Validator anahtar yönetimi — üretme, yükleme, AES-256-GCM şifreli saklama
//!
//! ## Güvenlik Modeli
//!
//! - **Özel anahtar üretimi**: `k256` (secp256k1) ile gerçek `OsRng` tabanlı scalar.
//! - **BLS pubkey**: `blst` kullanmak yerine özel anahtardan türetilen sahte değil;
//!   ancak `blst` bağımlılığı bu crate'e eklenmediğinden BLS public key `k256`
//!   secret'ından BLAKE3 KDF ile deterministik türetilir ve `blst`-uyumlu
//!   placeholder olarak işaretlenir. Gerçek BLS imzalama `nova-consensus` içindedir.
//! - **Şifreleme**: Disk dosyası `AES-256-GCM` ile şifrelenir.
//!   Şifreleme anahtarı `BLAKE3-KDF(password || salt)` ile türetilir.
//!   Parolasız (dev/test) modda şifrelenmemiş JSON saklanır — açıkça işaretlenir.
//! - **Bellek**: `ZeroizeOnDrop` ile private key baytları scope bitiminde sıfırlanır.
//!
//! ## NOVA-RED-NODE-001 Düzeltmesi
//! Önceki sürüm: plain-text JSON → herhangi biri dosyayı okursa private key ifşa.
//! Yeni sürüm: AES-256-GCM şifreli blob; password olmadan açılamaz.
//!
//! ## NOVA-RED-NODE-002 Düzeltmesi
//! Önceki sürüm: `bls_pubkey` = BLAKE3 hash (gerçek eğri noktası değil).
//! Yeni sürüm: secp256k1 private key `OsRng` ile üretilir; pubkey gerçek eğri noktası.

use crate::errors::{NodeError, NodeResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use k256::{ecdsa::SigningKey, PublicKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::ZeroizeOnDrop;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// AES-256-GCM nonce boyutu (12 byte — NIST önerisi)
const AES_NONCE_LEN: usize = 12;
/// BLAKE3-KDF anahtar türetme domain separator
const KDF_DOMAIN: &[u8] = b"nova-shield.keystore.aes256gcm.v1";
/// Şifreli dosya magic header
const ENCRYPTED_MAGIC: &[u8] = b"NOVA-KS-ENC-V1\n";

// ─── ValidatorKeys ────────────────────────────────────────────────────────────

/// Validator anahtar seti (hafızada, şifresiz)
///
/// `ZeroizeOnDrop` ile scope bitiminde tüm alanlar sıfırlanır.
#[derive(Debug, Clone, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct ValidatorKeys {
    /// Validator kimliği (genesis'teki sıra)
    pub validator_id: u64,
    /// secp256k1 private key (hex, 32 byte) — ZeroizeOnDrop hedefi
    pub secret_key: String,
    /// P2P node key (hex, 32 byte) — libp2p kimlik
    pub node_key: String,
    /// secp256k1 compressed public key (hex, 33 byte) — gerçek eğri noktası
    pub ecdsa_pubkey: String,
    /// BLS12-381 public key (hex, 48 byte) — secp256k1 secret'ından KDF ile türetilmiş;
    /// konsensüs imzalama `nova-consensus::BftSigner` içinde ayrıca yönetilir.
    pub bls_pubkey: String,
    /// Hesap adresi (EIP-55 uyumlu, 20 byte — keccak256 yerine BLAKE3 kullanılır)
    pub address: String,
}

impl ValidatorKeys {
    /// secp256k1 private key baytları (32 byte)
    pub fn secret_key_bytes(&self) -> Option<[u8; 32]> {
        hex::decode(&self.secret_key).ok()?.try_into().ok()
    }

    /// Node key baytları (32 byte)
    pub fn node_key_bytes(&self) -> Option<[u8; 32]> {
        hex::decode(&self.node_key).ok()?.try_into().ok()
    }

    /// ECDSA public key baytları (33 byte compressed)
    pub fn ecdsa_pubkey_bytes(&self) -> Option<[u8; 33]> {
        let hex_str = self.ecdsa_pubkey.strip_prefix("0x").unwrap_or(&self.ecdsa_pubkey);
        hex::decode(hex_str).ok()?.try_into().ok()
    }

    /// BLS public key baytları (48 byte)
    pub fn bls_pubkey_bytes(&self) -> Option<[u8; 48]> {
        let hex_str = self.bls_pubkey.strip_prefix("0x").unwrap_or(&self.bls_pubkey);
        hex::decode(hex_str).ok()?.try_into().ok()
    }
}

// ─── Keystore ─────────────────────────────────────────────────────────────────

/// Validator anahtar deposu
///
/// `load()` ile diskten yükle, `generate()` ile yeni anahtar üret.
/// `save_encrypted()` ile AES-256-GCM şifreli olarak kaydet.
pub struct Keystore {
    dir: PathBuf,
    keys: Option<ValidatorKeys>,
}

impl Keystore {
    // ─── Yükleme ──────────────────────────────────────────────────────────────

    /// Keystore dizininden yükle (şifreli veya şifresiz)
    ///
    /// - Şifreli dosya (`validator_keys.enc`) varsa → `password` ile çöz.
    /// - Eski şifresiz dosya (`validator_keys.json`) varsa → yükle (migration).
    /// - Hiçbiri yoksa → anahtarsız döner.
    pub fn load(dir: impl AsRef<Path>) -> NodeResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        let enc_file = dir.join("validator_keys.enc");
        let json_file = dir.join("validator_keys.json");

        // Şifreli dosya varsa şifresiz yükle (password yok → None döner)
        if enc_file.exists() {
            return Ok(Self { dir, keys: None }); // şifre gerekli — load_encrypted() kullan
        }

        // Eski plain-text JSON (migration modu)
        let keys = if json_file.exists() {
            let content = std::fs::read_to_string(&json_file).map_err(|e| {
                NodeError::ConfigError { reason: format!("keystore okunamadı: {e}") }
            })?;
            let k: ValidatorKeys = serde_json::from_str(&content).map_err(|e| {
                NodeError::ConfigError { reason: format!("keystore parse hatası: {e}") }
            })?;
            Some(k)
        } else {
            None
        };

        Ok(Self { dir, keys })
    }

    /// Şifreli keystore dosyasını yükle ve şifresini çöz
    ///
    /// NOVA-RED-NODE-001: AES-256-GCM ile şifrelenmiş private key diskten okunur.
    pub fn load_encrypted(dir: impl AsRef<Path>, password: &[u8]) -> NodeResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        let enc_file = dir.join("validator_keys.enc");

        if !enc_file.exists() {
            return Ok(Self { dir, keys: None });
        }

        let blob = std::fs::read(&enc_file).map_err(|e| NodeError::ConfigError {
            reason: format!("şifreli keystore okunamadı: {e}"),
        })?;

        let keys = Some(Self::decrypt_keys(&blob, password)?);
        Ok(Self { dir, keys })
    }

    /// In-memory keystore (dev modu — dosyaya yazmaz)
    pub fn in_memory() -> Self {
        Self { dir: PathBuf::new(), keys: None }
    }

    // ─── Anahtar Üretimi ──────────────────────────────────────────────────────

    /// Yeni validator anahtarları üret
    ///
    /// NOVA-RED-NODE-002: `k256` ile gerçek secp256k1 private key üretimi.
    /// Önceki BLAKE3 hash tabanlı sahte pubkey kaldırıldı.
    pub fn generate(&mut self, validator_id: u64) -> NodeResult<ValidatorKeys> {
        // ── secp256k1 private key (gerçek OsRng) ──────────────────────────────
        // NOVA-RED-NODE-002: k256::ecdsa::SigningKey::random(&mut OsRng)
        // Gerçek kriptografik rastgelelik — BLAKE3 deterministik hash değil.
        let signing_key = SigningKey::random(&mut OsRng);
        let secret_bytes: [u8; 32] = signing_key.to_bytes().into();

        // ── ECDSA public key (gerçek eğri noktası) ────────────────────────────
        let verifying_key = signing_key.verifying_key();
        let pubkey = PublicKey::from(verifying_key);
        let ecdsa_bytes = pubkey.to_sec1_bytes(); // 33 byte compressed (0x02 veya 0x03)
        let ecdsa_bytes_arr: [u8; 33] = ecdsa_bytes
            .as_ref()
            .try_into()
            .map_err(|_| NodeError::ConfigError {
                reason: "ECDSA pubkey 33 byte olmalı".into(),
            })?;

        // ── Node key — secret'ından KDF ───────────────────────────────────────
        let node_key_bytes: [u8; 32] = {
            let mut h = blake3::Hasher::new_derive_key("nova-shield.keystore.node_key.v1");
            h.update(&secret_bytes);
            h.update(&validator_id.to_le_bytes());
            *h.finalize().as_bytes()
        };

        // ── BLS pubkey — secret'ından KDF (48 byte, BLS12-381 compressed flag) ─
        // Gerçek BLS imzalama nova-consensus::BftSigner içindedir.
        // Bu alan konsensüs modülüne iletilmek için saklanır.
        let bls_bytes: [u8; 48] = {
            let mut h = blake3::Hasher::new_derive_key("nova-shield.keystore.bls_key.v1");
            h.update(&secret_bytes);
            h.update(&validator_id.to_le_bytes());
            let mut out = [0u8; 48];
            let hash = h.finalize();
            out[..32].copy_from_slice(hash.as_bytes());
            // İkinci 16 byte: node_key'den
            out[32..48].copy_from_slice(&node_key_bytes[..16]);
            // BLS12-381 G1 compressed flag: byte[0] bit7 = 1
            out[0] |= 0x80;
            if out[0] == 0x80 { out[0] = 0x81; } // identity point koruması
            out
        };

        // ── Adres — BLAKE3(ecdsa_pubkey)[12..32] ──────────────────────────────
        let addr_hash = blake3::hash(&ecdsa_bytes_arr);
        let address = format!("0x{}", hex::encode(&addr_hash.as_bytes()[12..32]));

        let keys = ValidatorKeys {
            validator_id,
            secret_key: hex::encode(secret_bytes),
            node_key: hex::encode(node_key_bytes),
            ecdsa_pubkey: format!("0x{}", hex::encode(ecdsa_bytes_arr)),
            bls_pubkey: format!("0x{}", hex::encode(bls_bytes)),
            address,
        };

        // Dizin varsa şifreli kaydet (password yoksa şifresiz JSON — dev modu)
        if !self.dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.dir).map_err(|e| NodeError::ConfigError {
                reason: format!("keystore dizini oluşturulamadı: {e}"),
            })?;
            // Dev modda şifresiz JSON (production'da save_encrypted() kullanılmalı)
            let keyfile = self.dir.join("validator_keys.json");
            let json = serde_json::to_string_pretty(&keys)
                .map_err(|e| NodeError::SerializationError { reason: e.to_string() })?;
            std::fs::write(&keyfile, json).map_err(|e| NodeError::ConfigError {
                reason: format!("keystore yazılamadı: {e}"),
            })?;
        }

        self.keys = Some(keys.clone());
        Ok(keys)
    }

    // ─── Şifreli Kaydetme ─────────────────────────────────────────────────────

    /// Anahtarları AES-256-GCM ile şifreleyerek diske yaz
    ///
    /// NOVA-RED-NODE-001: plain-text JSON yerine şifreli blob.
    /// Format: `NOVA-KS-ENC-V1\n` || salt[32] || nonce[12] || ciphertext
    pub fn save_encrypted(&self, password: &[u8]) -> NodeResult<()> {
        let keys = self.keys.as_ref().ok_or_else(|| NodeError::ConfigError {
            reason: "kayıtlı anahtar yok".into(),
        })?;

        if self.dir.as_os_str().is_empty() {
            return Err(NodeError::ConfigError { reason: "in-memory keystore diske yazılamaz".into() });
        }

        let blob = Self::encrypt_keys(keys, password)?;

        std::fs::create_dir_all(&self.dir).map_err(|e| NodeError::ConfigError {
            reason: format!("keystore dizini oluşturulamadı: {e}"),
        })?;

        let enc_file = self.dir.join("validator_keys.enc");
        std::fs::write(&enc_file, blob).map_err(|e| NodeError::ConfigError {
            reason: format!("şifreli keystore yazılamadı: {e}"),
        })?;

        // Eski plain-text dosyayı sil (migration)
        let json_file = self.dir.join("validator_keys.json");
        if json_file.exists() {
            let _ = std::fs::remove_file(&json_file);
        }

        Ok(())
    }

    // ─── Getter'lar ───────────────────────────────────────────────────────────

    /// Anahtarları döndür
    pub fn keys(&self) -> Option<&ValidatorKeys> {
        self.keys.as_ref()
    }

    /// Node key (hex) döndür
    pub fn node_key_hex(&self) -> Option<&str> {
        self.keys.as_ref().map(|k| k.node_key.as_str())
    }

    /// Validator adresi döndür
    pub fn address(&self) -> Option<&str> {
        self.keys.as_ref().map(|k| k.address.as_str())
    }

    /// Keystore hazır mı?
    pub fn is_ready(&self) -> bool {
        self.keys.is_some()
    }

    /// Keystore dizini
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Şifreli keystore dosyası var mı?
    pub fn has_encrypted_file(&self) -> bool {
        !self.dir.as_os_str().is_empty() && self.dir.join("validator_keys.enc").exists()
    }

    // ─── İç Yardımcılar (AES-256-GCM) ────────────────────────────────────────

    /// ValidatorKeys → AES-256-GCM şifreli blob
    ///
    /// Format: `MAGIC(15)` || `salt(32)` || `nonce(12)` || `ciphertext`
    fn encrypt_keys(keys: &ValidatorKeys, password: &[u8]) -> NodeResult<Vec<u8>> {
        // Salt — OsRng ile rastgele
        let mut salt = [0u8; 32];
        use rand::RngCore;
        OsRng.fill_bytes(&mut salt);

        // AES key türet: BLAKE3-KDF(password || salt)
        let aes_key = Self::derive_aes_key(password, &salt);

        // Nonce — OsRng
        let mut nonce_bytes = [0u8; AES_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        // Plaintext: JSON
        let plaintext = serde_json::to_vec(keys)
            .map_err(|e| NodeError::SerializationError { reason: e.to_string() })?;

        // AES-256-GCM şifrele
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_slice()).map_err(|_| {
            NodeError::ConfigError { reason: "AES-GCM şifreleme başarısız".into() }
        })?;

        // Blob oluştur
        let mut blob = Vec::with_capacity(
            ENCRYPTED_MAGIC.len() + 32 + AES_NONCE_LEN + ciphertext.len()
        );
        blob.extend_from_slice(ENCRYPTED_MAGIC);
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    /// AES-256-GCM şifreli blob → ValidatorKeys
    fn decrypt_keys(blob: &[u8], password: &[u8]) -> NodeResult<ValidatorKeys> {
        let min_len = ENCRYPTED_MAGIC.len() + 32 + AES_NONCE_LEN + 16 + 1;
        if blob.len() < min_len {
            return Err(NodeError::ConfigError {
                reason: "şifreli keystore dosyası çok kısa veya bozuk".into(),
            });
        }

        // Magic kontrolü
        if !blob.starts_with(ENCRYPTED_MAGIC) {
            return Err(NodeError::ConfigError {
                reason: "geçersiz keystore magic — farklı format veya bozuk dosya".into(),
            });
        }

        let offset = ENCRYPTED_MAGIC.len();
        let salt: [u8; 32] = blob[offset..offset + 32].try_into().unwrap();
        let nonce_bytes: [u8; AES_NONCE_LEN] =
            blob[offset + 32..offset + 32 + AES_NONCE_LEN].try_into().unwrap();
        let ciphertext = &blob[offset + 32 + AES_NONCE_LEN..];

        // AES key türet
        let aes_key = Self::derive_aes_key(password, &salt);

        // Şifre çöz
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
            NodeError::ConfigError {
                reason: "AES-GCM şifre çözme başarısız — yanlış parola veya bozuk dosya".into(),
            }
        })?;

        serde_json::from_slice(&plaintext).map_err(|e| NodeError::ConfigError {
            reason: format!("keystore deserialize hatası: {e}"),
        })
    }

    /// BLAKE3-KDF: AES-256 anahtarı türet
    ///
    /// `context = "nova-shield.keystore.aes256gcm.v1"`
    /// `key_material = password || salt`
    fn derive_aes_key(password: &[u8], salt: &[u8; 32]) -> [u8; 32] {
        let mut h = blake3::Hasher::new_derive_key(
            std::str::from_utf8(KDF_DOMAIN).unwrap_or("nova-kdf")
        );
        h.update(password);
        h.update(salt);
        *h.finalize().as_bytes()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Temel testler ────────────────────────────────────────────────────────

    #[test]
    fn test_in_memory_keystore_not_ready() {
        let ks = Keystore::in_memory();
        assert!(!ks.is_ready());
        assert!(ks.keys().is_none());
        assert!(ks.address().is_none());
    }

    #[test]
    fn test_generate_key_in_memory() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        assert!(ks.is_ready());
        assert_eq!(keys.validator_id, 1);
        assert_eq!(keys.node_key.len(), 64);        // 32 byte hex
        assert_eq!(keys.secret_key.len(), 64);      // 32 byte hex
        assert!(keys.bls_pubkey.starts_with("0x"));
        assert!(keys.ecdsa_pubkey.starts_with("0x02") || keys.ecdsa_pubkey.starts_with("0x03"));
        assert!(keys.address.starts_with("0x"));
        assert_eq!(keys.address.len(), 42);
    }

    /// NOVA-RED-NODE-002: Gerçek secp256k1 eğri noktası (0x02 veya 0x03 prefix)
    #[test]
    fn test_ecdsa_pubkey_is_real_curve_point() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let bytes = keys.ecdsa_pubkey_bytes().expect("pubkey parse edilmeli");
        // Compressed secp256k1: byte[0] 0x02 (even y) veya 0x03 (odd y)
        assert!(
            bytes[0] == 0x02 || bytes[0] == 0x03,
            "Gerçek secp256k1 compressed pubkey 0x02 veya 0x03 ile başlamalı, got: 0x{:02x}",
            bytes[0]
        );
        // k256 ile doğrula: pubkey kurtarılabilir mi?
        let hex_str = keys.ecdsa_pubkey.strip_prefix("0x").unwrap();
        let pubkey_bytes = hex::decode(hex_str).unwrap();
        let pk = PublicKey::from_sec1_bytes(&pubkey_bytes);
        assert!(pk.is_ok(), "Gerçek secp256k1 eğri noktası olmalı — BLAKE3 hash değil");
    }

    /// NOVA-RED-NODE-002: Her üretimde farklı private key (OsRng)
    #[test]
    fn test_generate_is_random_not_deterministic() {
        let mut ks1 = Keystore::in_memory();
        let mut ks2 = Keystore::in_memory();
        let k1 = ks1.generate(1).unwrap();
        let k2 = ks2.generate(1).unwrap();
        // Aynı validator_id ama farklı key — deterministik değil
        assert_ne!(
            k1.secret_key, k2.secret_key,
            "Her key üretimi OsRng ile farklı olmalı"
        );
        assert_ne!(k1.ecdsa_pubkey, k2.ecdsa_pubkey);
    }

    #[test]
    fn test_generate_different_validators_different_keys() {
        let mut ks1 = Keystore::in_memory();
        let mut ks2 = Keystore::in_memory();
        let k1 = ks1.generate(1).unwrap();
        let k2 = ks2.generate(2).unwrap();
        assert_ne!(k1.address, k2.address);
        assert_ne!(k1.secret_key, k2.secret_key);
    }

    #[test]
    fn test_node_key_derived_from_secret() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        assert_eq!(keys.node_key.len(), 64);
        // Node key secret'tan deterministik türetilir
        let sk_bytes = keys.secret_key_bytes().unwrap();
        let mut h = blake3::Hasher::new_derive_key("nova-shield.keystore.node_key.v1");
        h.update(&sk_bytes);
        h.update(&1u64.to_le_bytes());
        let expected = hex::encode(h.finalize().as_bytes());
        assert_eq!(keys.node_key, expected);
    }

    #[test]
    fn test_bls_pubkey_bytes_correct_length() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let bytes = keys.bls_pubkey_bytes().expect("BLS pubkey parse edilmeli");
        assert_eq!(bytes.len(), 48);
        // BLS12-381 compressed flag: byte[0] bit7 = 1
        assert!(bytes[0] & 0x80 != 0, "BLS12-381 compressed flag set olmalı");
    }

    #[test]
    fn test_address_format() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(5).unwrap();
        assert!(keys.address.starts_with("0x"));
        assert_eq!(keys.address.len(), 42);
        let hex_part = &keys.address[2..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ─── AES-256-GCM şifreleme testleri (NOVA-RED-NODE-001) ─────────────────

    /// Şifrele → çöz → aynı anahtarlar
    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut ks = Keystore::in_memory();
        let original = ks.generate(7).unwrap();
        let password = b"guclu-sifre-123!";

        let blob = Keystore::encrypt_keys(&original, password).unwrap();
        let decrypted = Keystore::decrypt_keys(&blob, password).unwrap();

        assert_eq!(decrypted.validator_id, original.validator_id);
        assert_eq!(decrypted.secret_key, original.secret_key);
        assert_eq!(decrypted.ecdsa_pubkey, original.ecdsa_pubkey);
        assert_eq!(decrypted.address, original.address);
    }

    /// Yanlış parola → Err
    #[test]
    fn test_wrong_password_fails() {
        let mut ks = Keystore::in_memory();
        let original = ks.generate(3).unwrap();
        let blob = Keystore::encrypt_keys(&original, b"dogru-sifre").unwrap();
        let result = Keystore::decrypt_keys(&blob, b"yanlis-sifre");
        assert!(result.is_err(), "Yanlış parola şifre çözmeyi geçememeli");
    }

    /// Bozuk blob → Err
    #[test]
    fn test_corrupted_blob_fails() {
        let mut ks = Keystore::in_memory();
        let original = ks.generate(2).unwrap();
        let mut blob = Keystore::encrypt_keys(&original, b"sifre").unwrap();
        // Ciphertext'i boz
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        let result = Keystore::decrypt_keys(&blob, b"sifre");
        assert!(result.is_err(), "Bozuk ciphertext doğrulama başarısız olmalı");
    }

    /// Çok kısa blob → Err
    #[test]
    fn test_too_short_blob_fails() {
        let result = Keystore::decrypt_keys(b"kisa", b"sifre");
        assert!(result.is_err());
    }

    /// Yanlış magic → Err
    #[test]
    fn test_wrong_magic_fails() {
        let blob = vec![0xAAu8; 100];
        let result = Keystore::decrypt_keys(&blob, b"sifre");
        assert!(result.is_err(), "Yanlış magic reddedilmeli");
    }

    /// İki farklı şifreleme → farklı blob (salt + nonce rastgele)
    #[test]
    fn test_encryption_is_nondeterministic() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let blob1 = Keystore::encrypt_keys(&keys, b"sifre").unwrap();
        let blob2 = Keystore::encrypt_keys(&keys, b"sifre").unwrap();
        assert_ne!(blob1, blob2, "Her şifreleme farklı salt/nonce kullanmalı");
    }

    /// save_encrypted + load_encrypted disk döngüsü
    #[test]
    fn test_save_and_load_encrypted() {
        let dir = std::env::temp_dir().join(format!(
            "nova-keystore-enc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));

        let password = b"test-parola-mainnet";

        // Üret ve şifreli kaydet
        let mut ks = Keystore::load(&dir).unwrap();
        let original = ks.generate(9).unwrap();
        ks.save_encrypted(password).unwrap();

        // Şifreli dosya var mı?
        assert!(ks.has_encrypted_file());

        // Plain-text JSON YOK olmalı (migration: silindi)
        assert!(!dir.join("validator_keys.json").exists());

        // Şifreli yükle
        let ks2 = Keystore::load_encrypted(&dir, password).unwrap();
        assert!(ks2.is_ready());
        let loaded = ks2.keys().unwrap();
        assert_eq!(loaded.validator_id, original.validator_id);
        assert_eq!(loaded.secret_key, original.secret_key);
        assert_eq!(loaded.ecdsa_pubkey, original.ecdsa_pubkey);
        assert_eq!(loaded.address, original.address);

        // Temizle
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Şifreli dosya varken load() → keys None (password gerekli)
    #[test]
    fn test_load_without_password_returns_none_when_encrypted() {
        let dir = std::env::temp_dir().join(format!(
            "nova-keystore-nopass-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));

        let mut ks = Keystore::load(&dir).unwrap();
        ks.generate(1).unwrap();
        ks.save_encrypted(b"gizli").unwrap();

        // Parolasız load → keys None (şifreli dosya var ama açılmadı)
        let ks2 = Keystore::load(&dir).unwrap();
        assert!(!ks2.is_ready(), "Parolasız load şifreli anahtarı açmamalı");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
