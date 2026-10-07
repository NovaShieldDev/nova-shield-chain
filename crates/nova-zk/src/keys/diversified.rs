//! DiversifiedAddress — tek bir viewing key'den birden fazla adres türetme
//!
//! Penumbra: Diversifier ∈ [u8; 16], transmission_key = IVK × diversifier_point
//! Nova: BLAKE3 tabanlı basitleştirilmiş implementasyon

use crate::keys::viewing::IncomingViewingKey;
use serde::{Deserialize, Serialize};

// ─── DiversifiedAddress ───────────────────────────────────────────────────────

/// Çeşitlendirilmiş ödeme adresi
/// Aynı cüzdana birden fazla adres bağlanabilir (privacy++)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiversifiedAddress {
    /// Diversifier index (16 byte)
    pub diversifier: [u8; 16],
    /// Alıcının transmission key'i
    pub transmission_key: [u8; 32],
}

impl DiversifiedAddress {
    /// IVK ve index'ten yeni adres türet
    pub fn new(ivk: &IncomingViewingKey, index: u64) -> Self {
        // Diversifier = BLAKE3("nova-diversifier" || ivk || index)[..16]
        let diversifier_bytes = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:diversifier:v1!!!!!!");
            h.update(ivk.as_bytes());
            h.update(&index.to_le_bytes());
            let hash = h.finalize();
            let mut d = [0u8; 16];
            d.copy_from_slice(&hash.as_bytes()[..16]);
            d
        };

        // Transmission key = BLAKE3("nova-tk-div" || ivk || diversifier)
        let transmission_key = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:diversified-tk:v1!!!");
            h.update(ivk.as_bytes());
            h.update(&diversifier_bytes);
            *h.finalize().as_bytes()
        };

        Self { diversifier: diversifier_bytes, transmission_key }
    }

    pub fn transmission_key(&self) -> &[u8; 32] {
        &self.transmission_key
    }

    pub fn diversifier(&self) -> &[u8; 16] {
        &self.diversifier
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ivk(seed: u8) -> IncomingViewingKey {
        IncomingViewingKey([seed; 32])
    }

    #[test]
    fn test_diversified_address_deterministic() {
        let ivk = make_ivk(1);
        let a1 = DiversifiedAddress::new(&ivk, 0);
        let a2 = DiversifiedAddress::new(&ivk, 0);
        assert_eq!(a1, a2);
    }

    #[test]
    fn test_diversified_address_unique_per_index() {
        let ivk = make_ivk(1);
        let a0 = DiversifiedAddress::new(&ivk, 0);
        let a1 = DiversifiedAddress::new(&ivk, 1);
        assert_ne!(a0, a1);
        assert_ne!(a0.transmission_key(), a1.transmission_key());
    }

    #[test]
    fn test_diversified_address_unique_per_ivk() {
        let ivk1 = make_ivk(1);
        let ivk2 = make_ivk(2);
        let a1 = DiversifiedAddress::new(&ivk1, 0);
        let a2 = DiversifiedAddress::new(&ivk2, 0);
        assert_ne!(a1, a2);
    }

    #[test]
    fn test_diversifier_length() {
        let ivk = make_ivk(5);
        let addr = DiversifiedAddress::new(&ivk, 42);
        assert_eq!(addr.diversifier().len(), 16);
        assert_eq!(addr.transmission_key().len(), 32);
    }
}
