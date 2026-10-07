//! SpendingKey — ZK-Shield spending key hiyerarşisi (Penumbra pattern'inden ilham)
//!
//! Hiyerarşi:
//!   SpendingKey (32 byte random)
//!     └── FullViewingKey  (deterministic → sadece görüntüleme)
//!             ├── IncomingViewingKey (gelen note'ları tespit et)
//!             ├── OutgoingViewingKey (giden note'ları görüntüle)
//!             └── NullifierKey      (nullifier türet)

use crate::utxo::nullifier::NullifierKey;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

// ─── SpendingKey ──────────────────────────────────────────────────────────────

/// Spending key — hesap sahipliğinin kanıtı; asla paylaşılmaz
#[derive(Clone, Debug, Zeroize, Serialize, Deserialize)]
pub struct SpendingKey {
    key_bytes: [u8; 32],
}

impl SpendingKey {
    /// Rastgele spending key üret
    pub fn generate<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let mut key_bytes = [0u8; 32];
        rng.fill_bytes(&mut key_bytes);
        Self { key_bytes }
    }

    /// Ham byte'lardan oluştur
    pub fn from_bytes(key_bytes: [u8; 32]) -> Self {
        Self { key_bytes }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.key_bytes
    }

    /// Nullifier key türet: BLAKE3("nova-nk-derive" || spending_key)
    pub fn nullifier_key(&self) -> NullifierKey {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-shield:nullifier-key:v1!!!!");
        hasher.update(&self.key_bytes);
        NullifierKey(*hasher.finalize().as_bytes())
    }

    /// Transmission key türet (alıcının public key'i gibi davranır)
    /// BLAKE3("nova-tk-derive" || spending_key)
    pub fn transmission_key(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-shield:transmission-key:v1!");
        hasher.update(&self.key_bytes);
        *hasher.finalize().as_bytes()
    }

    /// Full viewing key türet (deterministic)
    pub fn full_viewing_key(&self) -> super::viewing::FullViewingKey {
        super::viewing::FullViewingKey::from_spending_key(self)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn deterministic_sk(seed: u64) -> SpendingKey {
        let mut rng = StdRng::seed_from_u64(seed);
        SpendingKey::generate(&mut rng)
    }

    #[test]
    fn test_spending_key_generate_random() {
        let mut rng = rand::thread_rng();
        let sk1 = SpendingKey::generate(&mut rng);
        let sk2 = SpendingKey::generate(&mut rng);
        assert_ne!(sk1.to_bytes(), sk2.to_bytes());
    }

    #[test]
    fn test_spending_key_from_bytes_roundtrip() {
        let bytes = [42u8; 32];
        let sk = SpendingKey::from_bytes(bytes);
        assert_eq!(sk.to_bytes(), bytes);
    }

    #[test]
    fn test_nullifier_key_deterministic() {
        let sk = deterministic_sk(1);
        let nk1 = sk.nullifier_key();
        let nk2 = sk.nullifier_key();
        assert_eq!(nk1, nk2);
    }

    #[test]
    fn test_nullifier_key_unique_per_spending_key() {
        let sk1 = deterministic_sk(1);
        let sk2 = deterministic_sk(2);
        assert_ne!(sk1.nullifier_key(), sk2.nullifier_key());
    }

    #[test]
    fn test_transmission_key_deterministic() {
        let sk = deterministic_sk(5);
        let tk1 = sk.transmission_key();
        let tk2 = sk.transmission_key();
        assert_eq!(tk1, tk2);
    }

    #[test]
    fn test_transmission_key_unique() {
        let sk1 = deterministic_sk(1);
        let sk2 = deterministic_sk(2);
        assert_ne!(sk1.transmission_key(), sk2.transmission_key());
    }
}
