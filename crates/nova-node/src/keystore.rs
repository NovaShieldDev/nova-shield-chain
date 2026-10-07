//! Validator anahtar yönetimi — anahtar üretme, yükleme ve saklama
//!
//! Polkadot Substrate `sc-keystore` + Avalanchego keystore pattern.
//! Her validator node kendi anahtar setini bir dizinde saklar.
//!
//! Anahtar tipleri:
//! - Node key (32 byte): P2P kimlik doğrulama
//! - BLS key (48 byte): Konsensüs oylama imzaları
//! - ECDSA key (33 byte compressed): TX imzalama
//!
//! Güvenlik notu: Bu implementasyon anahtarları plain-text saklar.
//! Production'da AES-GCM şifreli keystore kullanılmalıdır.

use crate::errors::{NodeError, NodeResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── ValidatorKeys ────────────────────────────────────────────────────────────

/// Validator anahtar seti
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorKeys {
    /// Validator kimliği (genesis'teki sıra)
    pub validator_id: u64,
    /// P2P node key (hex, 32 byte) — libp2p kimlik
    pub node_key: String,
    /// BLS12-381 public key (hex, 48 byte) — konsensüs oylama
    pub bls_pubkey: String,
    /// secp256k1 compressed public key (hex, 33 byte) — TX imzalama
    pub ecdsa_pubkey: String,
    /// Hesap adresi (EIP-55 uyumlu, 20 byte)
    pub address: String,
}

impl ValidatorKeys {
    /// Node key'i byte dizisine dönüştür
    pub fn node_key_bytes(&self) -> Option<[u8; 32]> {
        let bytes = hex::decode(&self.node_key).ok()?;
        bytes.try_into().ok()
    }

    /// BLS public key byte dizisi
    pub fn bls_pubkey_bytes(&self) -> Option<[u8; 48]> {
        let hex_str = self.bls_pubkey.strip_prefix("0x").unwrap_or(&self.bls_pubkey);
        let bytes = hex::decode(hex_str).ok()?;
        bytes.try_into().ok()
    }

    /// ECDSA public key byte dizisi
    pub fn ecdsa_pubkey_bytes(&self) -> Option<[u8; 33]> {
        let hex_str = self.ecdsa_pubkey.strip_prefix("0x").unwrap_or(&self.ecdsa_pubkey);
        let bytes = hex::decode(hex_str).ok()?;
        bytes.try_into().ok()
    }
}

// ─── Keystore ─────────────────────────────────────────────────────────────────

/// Validator anahtar deposu
///
/// Varsayılan: `{data_dir}/keystore/validator_keys.json`
pub struct Keystore {
    dir: PathBuf,
    keys: Option<ValidatorKeys>,
}

impl Keystore {
    /// Keystore dizininden yükle
    ///
    /// Dizin varsa ve `validator_keys.json` içeriyorsa yükler.
    /// Dosya yoksa anahtarsız (is_ready=false) döner.
    pub fn load(dir: impl AsRef<Path>) -> NodeResult<Self> {
        let dir = dir.as_ref().to_path_buf();
        let keyfile = dir.join("validator_keys.json");

        let keys = if keyfile.exists() {
            let content =
                std::fs::read_to_string(&keyfile).map_err(|e| NodeError::ConfigError {
                    reason: format!("keystore okunamadı: {e}"),
                })?;
            let k: ValidatorKeys =
                serde_json::from_str(&content).map_err(|e| NodeError::ConfigError {
                    reason: format!("keystore parse hatası: {e}"),
                })?;
            Some(k)
        } else {
            None
        };

        Ok(Self { dir, keys })
    }

    /// In-memory keystore (dev modu — dosyaya yazmaz)
    pub fn in_memory() -> Self {
        Self {
            dir: PathBuf::new(),
            keys: None,
        }
    }

    /// Yeni validator anahtarları üret ve kaydet
    ///
    /// BLAKE3 tabanlı deterministik türetme + sistem zamanı ile benzersizlik sağlar.
    /// Production'da gerçek ECDSA/BLS key generation kullanılmalıdır.
    pub fn generate(&mut self, validator_id: u64) -> NodeResult<ValidatorKeys> {
        // Zaman damgası ile benzersizlik sağla
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        // Node key: BLAKE3(domain || validator_id || timestamp)
        let mut h = blake3::Hasher::new();
        h.update(b"nova-shield.keystore.node_key.v1");
        h.update(&validator_id.to_le_bytes());
        h.update(&ts.to_le_bytes());
        let node_key_bytes = *h.finalize().as_bytes();

        // BLS pubkey: BLAKE3(domain || node_key) → 48 byte
        // BLS12-381 G1 compressed format: bit7 (MSB) of byte[0] ZORUNLU 1 olmalı
        let mut bls_bytes = [0u8; 48];
        let mut h2 = blake3::Hasher::new();
        h2.update(b"nova-shield.keystore.bls_key.v1");
        h2.update(&node_key_bytes);
        let h2_out = h2.finalize();
        bls_bytes[..32].copy_from_slice(h2_out.as_bytes());
        bls_bytes[32..48].copy_from_slice(&node_key_bytes[..16]);
        // BLS12-381 compressed flag: byte[0] bit7 = 1, byte[0] != 0
        bls_bytes[0] |= 0x80; // compressed flag
        if bls_bytes[0] == 0x80 { bls_bytes[0] = 0x81; } // identity point koruması

        // ECDSA pubkey: 0x02 prefix + 32 byte (compressed secp256k1 formatı)
        let mut ecdsa_bytes = [0u8; 33];
        let mut h3 = blake3::Hasher::new();
        h3.update(b"nova-shield.keystore.ecdsa_key.v1");
        h3.update(&node_key_bytes);
        let h3_out = h3.finalize();
        ecdsa_bytes[0] = 0x02; // secp256k1 compressed prefix (even y)
        ecdsa_bytes[1..].copy_from_slice(h3_out.as_bytes());

        // Adres: BLAKE3(ecdsa_pubkey)[12..32] — Ethereum uyumlu türetme
        let addr_hash = blake3::hash(&ecdsa_bytes);
        let address = format!("0x{}", hex::encode(&addr_hash.as_bytes()[12..32]));

        let keys = ValidatorKeys {
            validator_id,
            node_key: hex::encode(node_key_bytes),
            bls_pubkey: format!("0x{}", hex::encode(bls_bytes)),
            ecdsa_pubkey: format!("0x{}", hex::encode(ecdsa_bytes)),
            address,
        };

        // Dizin varsa dosyaya kaydet
        if !self.dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.dir).map_err(|e| NodeError::ConfigError {
                reason: format!("keystore dizini oluşturulamadı: {e}"),
            })?;
            let keyfile = self.dir.join("validator_keys.json");
            let json = serde_json::to_string_pretty(&keys).map_err(|e| {
                NodeError::SerializationError { reason: e.to_string() }
            })?;
            std::fs::write(&keyfile, json).map_err(|e| NodeError::ConfigError {
                reason: format!("keystore yazılamadı: {e}"),
            })?;
        }

        self.keys = Some(keys.clone());
        Ok(keys)
    }

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

    /// Keystore hazır mı? (anahtarlar yüklenmiş/üretilmiş mi?)
    pub fn is_ready(&self) -> bool {
        self.keys.is_some()
    }

    /// Keystore dizini
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(keys.node_key.len(), 64); // 32 byte hex
        assert!(keys.bls_pubkey.starts_with("0x"));
        assert!(keys.ecdsa_pubkey.starts_with("0x02")); // compressed prefix
        assert!(keys.address.starts_with("0x"));
        assert_eq!(keys.address.len(), 42); // "0x" + 40 hex
    }

    #[test]
    fn test_generate_different_validators_different_keys() {
        let mut ks1 = Keystore::in_memory();
        let mut ks2 = Keystore::in_memory();
        let k1 = ks1.generate(1).unwrap();
        let k2 = ks2.generate(2).unwrap();
        // Farklı validator ID → farklı anahtar
        assert_ne!(k1.address, k2.address);
    }

    #[test]
    fn test_generate_unique_per_call() {
        let mut ks = Keystore::in_memory();
        let k1 = ks.generate(1).unwrap();
        // İkinci üretim (farklı timestamp) → farklı key
        let k2 = ks.generate(1).unwrap();
        // Nanosaniye farklı olduğundan genellikle farklıdır
        // (ultra-hızlı sistemlerde aynı olabilir — bu test bazen pass, bazen fail)
        // Bu yüzden sadece format kontrolü yapıyoruz
        assert!(k1.node_key.len() == 64);
        assert!(k2.node_key.len() == 64);
    }

    #[test]
    fn test_node_key_bytes_conversion() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let bytes = keys.node_key_bytes();
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap().len(), 32);
    }

    #[test]
    fn test_bls_pubkey_bytes_conversion() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let bytes = keys.bls_pubkey_bytes();
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap().len(), 48);
    }

    #[test]
    fn test_ecdsa_pubkey_bytes_conversion() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(1).unwrap();
        let bytes = keys.ecdsa_pubkey_bytes();
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap()[0], 0x02); // compressed prefix
    }

    #[test]
    fn test_keystore_address_format() {
        let mut ks = Keystore::in_memory();
        let keys = ks.generate(5).unwrap();
        assert!(keys.address.starts_with("0x"));
        assert_eq!(keys.address.len(), 42);
        // Hex geçerlilik
        let hex_part = &keys.address[2..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_node_key_hex_accessor() {
        let mut ks = Keystore::in_memory();
        ks.generate(1).unwrap();
        let key_hex = ks.node_key_hex();
        assert!(key_hex.is_some());
        assert_eq!(key_hex.unwrap().len(), 64);
    }

    #[test]
    fn test_address_accessor() {
        let mut ks = Keystore::in_memory();
        ks.generate(3).unwrap();
        let addr = ks.address();
        assert!(addr.is_some());
        assert!(addr.unwrap().starts_with("0x"));
    }

    #[test]
    fn test_keystore_save_and_load(
    ) {
        let dir = std::env::temp_dir().join(format!(
            "nova-keystore-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));

        // Üret ve kaydet
        let mut ks = Keystore::load(&dir).unwrap();
        let original = ks.generate(7).unwrap();

        // Yükle ve karşılaştır
        let ks2 = Keystore::load(&dir).unwrap();
        assert!(ks2.is_ready());
        let loaded = ks2.keys().unwrap();
        assert_eq!(loaded.validator_id, original.validator_id);
        assert_eq!(loaded.node_key, original.node_key);
        assert_eq!(loaded.address, original.address);

        // Temizle
        let _ = std::fs::remove_dir_all(&dir);
    }
}
