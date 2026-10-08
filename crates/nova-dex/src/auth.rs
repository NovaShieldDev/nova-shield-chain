//! Nova DEX — Handler kimlik doğrulama (NOVA-RED-DEX-AUTH-001 düzeltmesi)
//!
//! ## Sorun
//!
//! Önceki implementasyonda tüm handler'lar adresi JSON body'den düz metin olarak alıyordu.
//! Herhangi bir saldırgan başka bir kullanıcının adresini body'e yazarak o kullanıcı adına
//! işlem yapabiliyordu.
//!
//! ## Çözüm
//!
//! Her isteğe `signature` ve `public_key` alanı eklendi.
//! Sunucu tarafında:
//! 1. `request_hash = BLAKE3("nova-dex-auth-v1" || method || body_json)`
//! 2. `ed25519::verify(request_hash, signature, public_key)`
//! 3. `address_from_pubkey(public_key)` ile türetilen adres body'deki adresle eşleşmeli

use crate::errors::{DexError, DexResult};
use blake3;
use ed25519_dalek::{Signature, VerifyingKey, Verifier};

/// İmzalı DEX isteği — tüm mutating handler'lar bu yapıyı kullanır.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedRequest<T> {
    /// İstek gövdesi (asıl veri)
    pub body: T,
    /// ed25519 public key (32 byte, hex)
    pub public_key: String,
    /// ed25519 imzası (64 byte, hex) — `sign(request_hash(body))`
    pub signature: String,
}

/// İstek hash'i hesapla.
///
/// `BLAKE3("nova-dex-auth-v1" || method || body_json_bytes)`
pub fn compute_request_hash(method: &str, body_json: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"nova-dex-auth-v1");
    hasher.update(method.as_bytes());
    hasher.update(body_json);
    *hasher.finalize().as_bytes()
}

/// Adres türet: public key'den BLAKE3 ile 20-byte adres.
///
/// `address = BLAKE3(pubkey)[0..20]`
pub fn address_from_pubkey(pubkey: &[u8; 32]) -> [u8; 20] {
    let hash = blake3::hash(pubkey);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash.as_bytes()[..20]);
    addr
}

/// Adresi hex string'e çevir.
pub fn address_to_hex(addr: &[u8; 20]) -> String {
    format!("0x{}", hex::encode(addr))
}

/// Hex string'den adres parse et.
pub fn parse_hex_address(s: &str) -> Option<[u8; 20]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).ok()?;
    if bytes.len() != 20 { return None; }
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Some(addr)
}

/// İmzalı isteği doğrula.
///
/// 1. `public_key` hex → `[u8; 32]` parse
/// 2. `signature` hex → `[u8; 64]` parse
/// 3. `ed25519::verify(request_hash, sig, pk)` kontrol
/// 4. `address_from_pubkey(pk)` ile türetilen adres `expected_address` ile eşleşmeli
pub fn verify_signed_request(
    method: &str,
    body_json: &[u8],
    public_key_hex: &str,
    signature_hex: &str,
    expected_address: &[u8; 20],
) -> DexResult<()> {
    // public_key parse
    let pk_bytes = hex::decode(public_key_hex.strip_prefix("0x").unwrap_or(public_key_hex))
        .map_err(|_| DexError::InvalidSignature)?;
    if pk_bytes.len() != 32 {
        return Err(DexError::InvalidSignature);
    }
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pk_bytes);

    // signature parse
    let sig_bytes = hex::decode(signature_hex.strip_prefix("0x").unwrap_or(signature_hex))
        .map_err(|_| DexError::InvalidSignature)?;
    if sig_bytes.len() != 64 {
        return Err(DexError::InvalidSignature);
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);

    // ed25519 doğrula
    let vk = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|_| DexError::InvalidSignature)?;
    let sig = Signature::from_bytes(&sig_arr);
    let hash = compute_request_hash(method, body_json);
    vk.verify(&hash, &sig)
        .map_err(|_| DexError::InvalidSignature)?;

    // Adres eşleşmeli
    let derived_addr = address_from_pubkey(&pk_arr);
    if &derived_addr != expected_address {
        return Err(DexError::Unauthorized {
            reason: "imzalayan adres beklenen adresle eşleşmiyor".into(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{SigningKey, Signer};

    fn make_keypair(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn test_verify_valid_signature() {
        let sk = make_keypair(0x42);
        let vk = sk.verifying_key();
        let pk_bytes = vk.to_bytes();
        let expected_addr = address_from_pubkey(&pk_bytes);

        let body = br#"{"amount":"1000"}"#;
        let hash = compute_request_hash("swap", body);
        let sig = sk.sign(&hash);

        let result = verify_signed_request(
            "swap",
            body,
            &hex::encode(pk_bytes),
            &hex::encode(sig.to_bytes()),
            &expected_addr,
        );
        assert!(result.is_ok(), "geçerli imza kabul edilmeli");
    }

    #[test]
    fn test_verify_wrong_signature_rejected() {
        let sk = make_keypair(0x42);
        let vk = sk.verifying_key();
        let pk_bytes = vk.to_bytes();
        let expected_addr = address_from_pubkey(&pk_bytes);

        let body = br#"{"amount":"1000"}"#;
        let fake_sig = [0xABu8; 64];

        let result = verify_signed_request(
            "swap",
            body,
            &hex::encode(pk_bytes),
            &hex::encode(fake_sig),
            &expected_addr,
        );
        assert!(matches!(result, Err(DexError::InvalidSignature)));
    }

    #[test]
    fn test_verify_wrong_address_rejected() {
        let sk = make_keypair(0x42);
        let vk = sk.verifying_key();
        let pk_bytes = vk.to_bytes();

        let body = br#"{"amount":"1000"}"#;
        let hash = compute_request_hash("swap", body);
        let sig = sk.sign(&hash);

        // Farklı bir adres bekleyelim
        let wrong_addr = [0xFFu8; 20];

        let result = verify_signed_request(
            "swap",
            body,
            &hex::encode(pk_bytes),
            &hex::encode(sig.to_bytes()),
            &wrong_addr,
        );
        assert!(matches!(result, Err(DexError::Unauthorized { .. })));
    }
}
