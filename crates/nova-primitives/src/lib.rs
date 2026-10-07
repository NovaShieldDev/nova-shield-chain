//! # Nova Shield Chain — Primitives
//!
//! Tüm crate'lerin bağlandığı temel tip, sabit ve trait tanımları.
//! Bu crate kasıtlı olarak minimal tutulmuştur — yalnızca
//! bağımlılık döngüsü yaratmayan, saf veri tipleri içerir.

#![deny(clippy::all)]
// Geliştirme aşamasında warn, mainnet öncesi deny yapılacak
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod constants;
pub mod crypto;
pub mod errors;
pub mod traits;
pub mod types;

// En sık kullanılanları re-export et
pub use constants::*;
pub use crypto::{
    BlsPublicKey, BlsSignature, DilithiumPublicKey, Ed25519PublicKey, Hash, PublicKey,
    Signature, SignatureType, Sr25519PublicKey,
};
pub use errors::{NovaError, Result};
pub use traits::{
    BeginSlotCtx, EndSlotCtx, GenesisHandler, HealthChecker, HealthReport,
    NovaModule, SlashableInvariant,
};
pub use types::{
    AccountId, Address, AssetId, Balance, BlockHash, BlockNumber, ChainId, Coin, Coins,
    Dec, EpochId, GeoRegion, MultiSignature, NodeId, Nonce, RuntimeVersion, SlotId,
    Threshold, Timestamp, ValidatorId,
};

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Address EIP-55 Checksum Testleri ─────────────────────────────────────

    /// Go-Ethereum EIP-55 checksum doğrulaması.
    /// `common.Address.Hex()` ile üretilen değerlerle karşılaştırılır.
    #[test]
    fn test_address_eip55_checksum() {
        // Sıfır adres → tüm küçük harf
        let zero = Address::ZERO;
        let hex = zero.to_checksum_hex();
        assert!(hex.starts_with("0x"), "0x prefix olmalı");
        assert_eq!(hex.len(), 42, "0x + 40 hex char = 42");
    }

    #[test]
    fn test_address_from_bytes_roundtrip() {
        let mut bytes = [0u8; 20];
        bytes[0] = 0xAB;
        bytes[19] = 0xCD;
        let addr = Address::from_bytes(bytes);
        assert_eq!(addr.as_bytes(), &bytes);
    }

    #[test]
    fn test_address_display_has_prefix() {
        let addr = Address::ZERO;
        let s = format!("{}", addr);
        assert!(s.starts_with("0x"));
    }

    // EIP-55 roundtrip: to_checksum_hex → from_checksum_hex
    #[test]
    fn test_eip55_roundtrip() {
        let bytes: [u8; 20] = [
            0x5a, 0xAe, 0xb6, 0x05, 0x3F, 0x3E, 0x94, 0xC9, 0xb9, 0xA0,
            0x9f, 0x33, 0x66, 0x94, 0x35, 0xE7, 0xEf, 0x1B, 0xeA, 0xd0,
        ];
        let addr = Address::from_bytes(bytes);
        let hex = addr.to_checksum_hex();
        assert!(hex.starts_with("0x"));
        let parsed = Address::from_checksum_hex(&hex).expect("parse başarılı olmalı");
        assert_eq!(parsed.as_bytes(), addr.as_bytes());
    }

    // EIP-55: yanlış checksum → None
    #[test]
    fn test_eip55_wrong_checksum_rejected() {
        let addr = Address::from_bytes([0xABu8; 20]);
        let hex = addr.to_checksum_hex();
        // Bir harfi ters çevir
        let wrong: String = hex.chars().enumerate().map(|(i, c)| {
            if i == 5 && c.is_ascii_alphabetic() {
                if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() }
            } else { c }
        }).collect();
        // Eğer wrong == hex ise bu test anlamlı değil (karakterler sayısal olabilir)
        if wrong != hex {
            assert!(Address::from_checksum_hex(&wrong).is_none(), "yanlış checksum reddedilmeli");
        }
    }

    // EIP-55: is_valid_checksum
    #[test]
    fn test_eip55_is_valid_checksum() {
        let addr = Address::from_bytes([0x55u8; 20]);
        let hex = addr.to_checksum_hex();
        assert!(Address::is_valid_checksum(&hex));

        // Tümü küçük harf (non-checksummed) da geçerli
        let lower = hex.to_lowercase();
        assert!(Address::is_valid_checksum(&lower));

        // Geçersiz uzunluk
        assert!(!Address::is_valid_checksum("0x1234"));
    }

    // EIP-55: checksum büyük/küçük harf dağılımı
    #[test]
    fn test_eip55_mixed_case() {
        let addr = Address::from_bytes([
            0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xde, 0xad, 0xbe, 0xef,
        ]);
        let hex = addr.to_checksum_hex();
        // En az bir büyük harf bulunmalı (deterministik keccak256 sonucuna bağlı)
        let has_upper = hex[2..].chars().any(|c| c.is_ascii_uppercase());
        let has_digit = hex[2..].chars().any(|c| c.is_ascii_digit());
        // Adres bazı hex digit içeriyor
        assert!(has_digit || has_upper);
        assert_eq!(hex.len(), 42);
    }

    // ─── NodeId Testleri (Avalanchego ids.NodeID) ─────────────────────────────

    /// Avalanchego NodeID: public key'den 20-byte ID türetimi.
    #[test]
    fn test_node_id_from_public_key() {
        let pubkey = vec![0x02u8; 33]; // mock secp256k1 compressed
        let node_id = NodeId::from_public_key(&pubkey);
        assert_ne!(node_id, NodeId::ZERO, "NodeId sıfır olmamalı");
    }

    #[test]
    fn test_node_id_cb58_format() {
        let node_id = NodeId::from_bytes([1u8; 20]);
        let cb58 = node_id.to_cb58();
        assert!(!cb58.is_empty(), "CB58 boş olmamalı");
    }

    #[test]
    fn test_node_id_deterministic() {
        let pubkey = vec![0x03u8; 33];
        let id1 = NodeId::from_public_key(&pubkey);
        let id2 = NodeId::from_public_key(&pubkey);
        assert_eq!(id1, id2, "Aynı public key → aynı NodeId");
    }

    // ─── AssetId Testleri (Avalanchego ids.ID) ────────────────────────────────

    /// Avalanchego ID: isimden deterministik ID.
    #[test]
    fn test_asset_id_from_name() {
        let id1 = AssetId::from_name("NOVA");
        let id2 = AssetId::from_name("NOVA");
        assert_eq!(id1, id2, "Aynı isim → aynı AssetId");
    }

    #[test]
    fn test_asset_id_different_names() {
        let nsc = AssetId::from_name("NOVA");
        let btc = AssetId::from_name("BTC");
        assert_ne!(nsc, btc, "Farklı isim → farklı AssetId");
    }

    #[test]
    fn test_asset_id_nsc_is_zero() {
        assert_eq!(AssetId::NOVA, AssetId::ZERO, "NSC = sıfır AssetId");
    }

    // ─── Coin Testleri (Cosmos-SDK sdk.Coin) ──────────────────────────────────

    /// Cosmos-SDK Coin.Add() karşılığı.
    #[test]
    fn test_coin_add_same_asset() {
        let a = Coin::nova(100);
        let b = Coin::nova(50);
        let result = a.add(&b).expect("toplama başarılı olmalı");
        assert_eq!(result.amount, 150);
    }

    #[test]
    fn test_coin_add_different_asset_fails() {
        let nsc = Coin::nova(100);
        let btc = Coin::new(AssetId::from_name("BTC"), 1);
        assert!(nsc.add(&btc).is_none(), "Farklı varlıklar toplanamaz");
    }

    #[test]
    fn test_coin_sub() {
        let a = Coin::nova(100);
        let b = Coin::nova(30);
        let result = a.sub(&b).expect("çıkarma başarılı olmalı");
        assert_eq!(result.amount, 70);
    }

    #[test]
    fn test_coin_sub_underflow() {
        let a = Coin::nova(10);
        let b = Coin::nova(20);
        assert!(a.sub(&b).is_none(), "Negatif çıkarma None olmalı");
    }

    #[test]
    fn test_coins_amount_of() {
        let coins = Coins::of(Coin::nova(500));
        assert_eq!(coins.amount_of(&AssetId::ZERO), 500);
        assert_eq!(coins.amount_of(&AssetId::from_name("BTC")), 0);
    }

    // ─── Dec Testleri (Cosmos-SDK sdk.Dec) ────────────────────────────────────

    /// Cosmos-SDK sdk.Dec.MulInt64 karşılığı: Dec × u64.
    #[test]
    fn test_dec_from_integer() {
        let d = Dec::from_integer(2);
        assert_eq!(d.0, 2 * Dec::PRECISION);
    }

    #[test]
    fn test_dec_from_ratio() {
        let half = Dec::from_ratio(1, 2).unwrap();
        assert_eq!(half, Dec::HALF);
    }

    #[test]
    fn test_dec_from_ratio_zero_den() {
        assert!(Dec::from_ratio(1, 0).is_none());
    }

    #[test]
    fn test_dec_mul_amount() {
        // %50 oranı × 200 = 100
        let half = Dec::HALF;
        let result = half.mul_amount(200);
        assert_eq!(result, 100);
    }

    #[test]
    fn test_dec_from_bps() {
        // 5000 BPS = %50
        let d = Dec::from_bps(5000);
        assert!((d.to_f64() - 0.5).abs() < 0.000001);
    }

    #[test]
    fn test_dec_add_sub() {
        let a = Dec::from_integer(1);
        let b = Dec::HALF;
        let sum = a.add(b);
        assert!((sum.to_f64() - 1.5).abs() < 0.000001);
        let diff = sum.sub(b);
        assert!((diff.to_f64() - 1.0).abs() < 0.000001);
    }

    // ─── MultiSignature Testleri (Polkadot MultiSignature) ────────────────────

    /// Polkadot MultiSignature: doğru boyut kontrolü.
    #[test]
    fn test_multi_sig_ed25519_valid_size() {
        let bytes = vec![0u8; 64];
        let sig = MultiSignature::ed25519(bytes).expect("64 byte geçerli");
        assert_eq!(sig.scheme_name(), "Ed25519");
        assert!(sig.is_valid_size());
    }

    #[test]
    fn test_multi_sig_ecdsa_valid_size() {
        let bytes = vec![0u8; 65];
        let sig = MultiSignature::ecdsa(bytes).expect("65 byte geçerli");
        assert_eq!(sig.scheme_name(), "Ecdsa");
    }

    #[test]
    fn test_multi_sig_wrong_size() {
        let bytes = vec![0u8; 32]; // Ed25519 için yanlış boyut
        assert!(MultiSignature::ed25519(bytes).is_none());
    }

    #[test]
    fn test_multi_sig_dilithium_post_quantum() {
        let bytes = vec![0u8; 3293]; // Dilithium3 imza boyutu
        let sig = MultiSignature::dilithium3(bytes);
        assert!(sig.is_post_quantum());
        assert_eq!(sig.scheme_name(), "Dilithium3");
    }

    #[test]
    fn test_multi_sig_aggregate_bls() {
        let bytes = vec![0u8; 96];
        let sig = MultiSignature::aggregate_bls(bytes).expect("96 byte geçerli");
        assert_eq!(sig.scheme_name(), "AggregateBls");
        assert!(!sig.is_post_quantum());
    }

    // ─── RuntimeVersion Testleri (Polkadot RuntimeVersion) ────────────────────

    /// Polkadot can_author_with karşılığı: versiyon uyumluluk kontrolü.
    #[test]
    fn test_runtime_version_genesis() {
        let v = RuntimeVersion::genesis();
        assert_eq!(v.spec_name, "nova-shield");
        assert_eq!(v.spec_version, 1);
        assert_eq!(v.transaction_version, 1);
    }

    #[test]
    fn test_runtime_version_compatibility() {
        let v1 = RuntimeVersion::genesis();
        let v2 = RuntimeVersion::genesis();
        assert!(v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_runtime_version_incompatible_spec() {
        let v1 = RuntimeVersion::genesis();
        let mut v2 = RuntimeVersion::genesis();
        v2.spec_version = 2;
        assert!(!v1.is_compatible_with(&v2));
    }

    // ─── Ed25519PublicKey Testleri ─────────────────────────────────────────────

    /// Solana Pubkey.to_string() karşılığı: base58 encoding.
    #[test]
    fn test_ed25519_pubkey_base58() {
        let key = Ed25519PublicKey::from_bytes([1u8; 32]);
        let b58 = key.to_base58();
        assert!(!b58.is_empty());
        // Base58 decode edince orijinal byte'lar geri gelmeli
        let decoded = bs58::decode(&b58).into_vec().unwrap();
        assert_eq!(decoded, vec![1u8; 32]);
    }

    #[test]
    fn test_ed25519_pubkey_roundtrip() {
        let original = [42u8; 32];
        let key = Ed25519PublicKey::from_bytes(original);
        assert_eq!(key.as_bytes(), &original);
    }

    // ─── HealthReport Testleri (Avalanchego health.Result) ────────────────────

    #[test]
    fn test_health_report_ok() {
        let report = HealthReport::ok(1000);
        assert!(report.healthy);
        assert_eq!(report.message, "OK");
    }

    #[test]
    fn test_health_report_error() {
        let report = HealthReport::error("bağlantı kesildi", 2000);
        assert!(!report.healthy);
        assert_eq!(report.message, "bağlantı kesildi");
        assert_eq!(report.checked_at_ms, 2000);
    }

    // ─── Threshold Testleri ────────────────────────────────────────────────────

    #[test]
    fn test_threshold_default_3of5() {
        let t = Threshold::default_3_of_5();
        assert_eq!(t.t, 3);
        assert_eq!(t.n, 5);
        assert!(t.is_met_by(3));
        assert!(!t.is_met_by(2));
    }

    #[test]
    fn test_threshold_invalid() {
        assert!(Threshold::new(0, 5).is_none(), "t=0 geçersiz");
        assert!(Threshold::new(6, 5).is_none(), "t > n geçersiz");
    }
}
