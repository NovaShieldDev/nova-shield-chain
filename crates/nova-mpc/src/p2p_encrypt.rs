//! MPC protokolünde parti-to-parti (P2P) mesaj şifrelemesi.
//!
//! §01-multi-party-ecdsa.md §11.1: "P2P Şifreleme: ECDH → AES-256-GCM"
//! §02-tss-lib.md §10.3: "Transport güvenliği: E2E şifreleme"
//! §6.2: "Validator key share güvenliği — HSM dışına çıkmaz"
//!
//! ## Nasıl Çalışır?
//!
//! 1. Her parti, keygen/reshare oturumu başında ephemeral ECDH keypair üretir.
//! 2. Public key'i Round 1'de yayımlar.
//! 3. Party A, Party B'ye VSS payı göndermek için:
//!    - `shared_secret = A_ecdh_sk * B_ecdh_pk` (ECDH)
//!    - `key = HKDF(shared_secret, "nova-mpc-p2p-v1" || session_id)`
//!    - `ciphertext = AES-256-GCM(key, share_bytes, aad=session||from||to)`
//!
//! ## Güvenlik Özellikleri
//!
//! - Forward secrecy: Ephemeral ECDH key her oturum için yeniden üretilir.
//! - Authenticated encryption: GCM tag ile mesaj bütünlüğü.
//! - AAD (from||to||session): Mesajı yanlış alıcıya iletmek imzalı mesajla
//!   tespit edilir (kriptografik bağlama).
//! - Zeroize: `EcdhSecret` drop edildiğinde gizli key silinir.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce as AesNonce,
};
use k256::{ProjectivePoint, Scalar};
use k256::elliptic_curve::Field;
use rand::rngs::OsRng;
use rand_core::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto_utils::{
    point_from_bytes, point_to_bytes, scalar_mul, scalar_mul_generator, scalar_to_bytes,
    scalar_from_bytes,
};
use crate::errors::{MpcError, MpcResult};
use crate::types::{EcdhPublicKey, PartyIndex, SessionId};

// ─── ECDH KEYPAIR ─────────────────────────────────────────────────────────────

/// Oturuma özgü ephemeral ECDH keypair.
///
/// `ZeroizeOnDrop` — drop edildiğinde gizli key sıfırlanır.
#[derive(Debug, ZeroizeOnDrop)]
pub struct EcdhSecret {
    /// Gizli ECDH skaleri. ZEROIZE ON DROP.
    secret: [u8; 32],
    /// Public ECDH noktası (sıkıştırılmış 33 byte).
    #[zeroize(skip)]
    public: [u8; 33],
}

impl EcdhSecret {
    /// Yeni rastgele ephemeral ECDH keypair üret.
    pub fn generate() -> Self {
        let secret = Scalar::random(&mut OsRng);
        let secret_bytes = scalar_to_bytes(&secret);
        let public = scalar_mul_generator(&secret);
        Self {
            secret: secret_bytes,
            public,
        }
    }

    /// Public key'i (broadcast için).
    pub fn public_key(&self) -> EcdhPublicKey {
        EcdhPublicKey { compressed: self.public }
    }

    /// ECDH shared secret hesapla.
    ///
    /// `our_secret * their_public = their_secret * our_public = shared_point`
    ///
    /// Shared secret olarak noktanın x-koordinatı kullanılır (standart ECDH).
    fn shared_secret(&self, their_public: &EcdhPublicKey) -> MpcResult<[u8; 32]> {
        let their_point = point_from_bytes(&their_public.compressed)?;
        let our_sk = scalar_from_bytes(&self.secret)?;
        let shared_point = their_point * &our_sk;

        // x-koordinatını al (standart ECDH)
        crate::crypto_utils::point_x_coord(&point_to_bytes(&shared_point))
    }

    /// P2P şifreleme anahtarı türet (HKDF-SHA256).
    ///
    /// `key = HKDF(ecdh_shared, salt=session_id, info="nova-mpc-p2p-v1||from||to")`
    pub fn derive_p2p_key(
        &self,
        their_public: &EcdhPublicKey,
        session_id: &SessionId,
        from: PartyIndex,
        to: PartyIndex,
    ) -> MpcResult<[u8; 32]> {
        let shared = self.shared_secret(their_public)?;

        // info = "nova-mpc-p2p-v1" || from_le || to_le
        let mut info = Vec::with_capacity(16 + 4 + 4);
        info.extend_from_slice(b"nova-mpc-p2p-v1:");
        info.extend_from_slice(&from.0.to_le_bytes());
        info.extend_from_slice(&to.0.to_le_bytes());

        // HKDF-SHA256(ikm=shared, salt=session_id, info=info) → 32 byte
        use hkdf::Hkdf;
        use sha2::Sha256;
        let hk = Hkdf::<Sha256>::new(Some(session_id.as_bytes()), &shared);
        let mut key = [0u8; 32];
        hk.expand(&info, &mut key)
            .map_err(|e| MpcError::EncryptionFailed(e.to_string()))?;
        Ok(key)
    }
}

// ─── ŞİFRELEME/ÇÖZMe ─────────────────────────────────────────────────────────

/// VSS payını AES-256-GCM ile şifrele.
///
/// AAD (Additional Authenticated Data): `session_id || from.get() || to.get()`
/// → Mesajın başka bir alıcıya yönlendirilmesi GCM tag doğrulamasını bozar.
pub fn encrypt_share(
    our_secret: &EcdhSecret,
    their_public: &EcdhPublicKey,
    session_id: &SessionId,
    from: PartyIndex,
    to: PartyIndex,
    share_bytes: &[u8; 32],
) -> MpcResult<([u8; 12], Vec<u8>)> {
    let key = our_secret.derive_p2p_key(their_public, session_id, from, to)?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| MpcError::EncryptionFailed(e.to_string()))?;

    // Rastgele 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = AesNonce::from_slice(&nonce_bytes);

    // AAD = session_id || from_le || to_le
    let mut aad = Vec::with_capacity(32 + 4 + 4);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(&from.0.to_le_bytes());
    aad.extend_from_slice(&to.0.to_le_bytes());

    let ciphertext = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload { msg: share_bytes.as_ref(), aad: &aad },
        )
        .map_err(|e| MpcError::EncryptionFailed(e.to_string()))?;

    Ok((nonce_bytes, ciphertext))
}

/// AES-256-GCM ile şifrelenmiş payı çöz.
pub fn decrypt_share(
    our_secret: &EcdhSecret,
    sender_public: &EcdhPublicKey,
    session_id: &SessionId,
    from: PartyIndex,
    to: PartyIndex,
    nonce_bytes: &[u8; 12],
    ciphertext: &[u8],
) -> MpcResult<[u8; 32]> {
    // P2P key: to tarafı olarak çalışıyoruz, from'ın public key'ini kullanıyoruz
    let key = our_secret.derive_p2p_key(sender_public, session_id, from, to)?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| MpcError::DecryptionFailed(e.to_string()))?;

    let nonce = AesNonce::from_slice(nonce_bytes);

    let mut aad = Vec::with_capacity(32 + 4 + 4);
    aad.extend_from_slice(session_id.as_bytes());
    aad.extend_from_slice(&from.0.to_le_bytes());
    aad.extend_from_slice(&to.0.to_le_bytes());

    let plaintext = cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload { msg: ciphertext, aad: &aad },
        )
        .map_err(|e| MpcError::DecryptionFailed(e.to_string()))?;

    if plaintext.len() != 32 {
        return Err(MpcError::DecryptionFailed(
            format!("beklenen 32 byte, gelen {}", plaintext.len())
        ));
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&plaintext);
    Ok(out)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    fn test_session() -> SessionId {
        SessionId([0x42u8; 32])
    }

    #[test]
    fn test_ecdh_encrypt_decrypt_roundtrip() {
        let alice_secret = EcdhSecret::generate();
        let bob_secret = EcdhSecret::generate();

        let alice_pk = alice_secret.public_key();
        let bob_pk = bob_secret.public_key();

        let from = PartyIndex(1);
        let to = PartyIndex(2);
        let session = test_session();

        let share = [0xBB; 32];
        let (nonce, ciphertext) = encrypt_share(
            &alice_secret, &bob_pk, &session, from, to, &share
        ).unwrap();

        let decrypted = decrypt_share(
            &bob_secret, &alice_pk, &session, from, to, &nonce, &ciphertext
        ).unwrap();

        assert_eq!(decrypted, share, "şifre çözme orijinal payı vermeli");
    }

    #[test]
    fn test_wrong_recipient_fails() {
        let alice_secret = EcdhSecret::generate();
        let bob_secret = EcdhSecret::generate();
        let eve_secret = EcdhSecret::generate();

        let bob_pk = bob_secret.public_key();
        let eve_pk = eve_secret.public_key();

        let from = PartyIndex(1);
        let to = PartyIndex(2);
        let wrong_to = PartyIndex(3);
        let session = test_session();

        let share = [0xCC; 32];
        let (nonce, ciphertext) = encrypt_share(
            &alice_secret, &bob_pk, &session, from, to, &share
        ).unwrap();

        // Eve, Alice'in Bob'a gönderdiği mesajı okumaya çalışıyor
        let result = decrypt_share(
            &eve_secret, &alice_secret.public_key(), &session, from, wrong_to, &nonce, &ciphertext
        );
        assert!(result.is_err(), "yanlış alıcı şifre çözememeli");
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let alice_secret = EcdhSecret::generate();
        let bob_secret = EcdhSecret::generate();

        let bob_pk = bob_secret.public_key();
        let alice_pk = alice_secret.public_key();

        let from = PartyIndex(1);
        let to = PartyIndex(2);
        let session = test_session();

        let share = [0xDD; 32];
        let (nonce, mut ciphertext) = encrypt_share(
            &alice_secret, &bob_pk, &session, from, to, &share
        ).unwrap();

        // Şifreli metni boz
        ciphertext[5] ^= 0xFF;

        let result = decrypt_share(
            &bob_secret, &alice_pk, &session, from, to, &nonce, &ciphertext
        );
        assert!(result.is_err(), "bozuk şifreli metin reddedilmeli");
    }

    #[test]
    fn test_wrong_session_fails() {
        let alice_secret = EcdhSecret::generate();
        let bob_secret = EcdhSecret::generate();

        let bob_pk = bob_secret.public_key();
        let alice_pk = alice_secret.public_key();

        let from = PartyIndex(1);
        let to = PartyIndex(2);
        let session1 = SessionId([0x01u8; 32]);
        let session2 = SessionId([0x02u8; 32]);

        let share = [0xEE; 32];
        let (nonce, ciphertext) = encrypt_share(
            &alice_secret, &bob_pk, &session1, from, to, &share
        ).unwrap();

        let result = decrypt_share(
            &bob_secret, &alice_pk, &session2, from, to, &nonce, &ciphertext
        );
        assert!(result.is_err(), "yanlış session ID reddedilmeli");
    }

    #[test]
    fn test_ecdh_symmetric() {
        // Alice ve Bob aynı shared secret'e ulaşmalı
        let alice = EcdhSecret::generate();
        let bob = EcdhSecret::generate();

        let alice_to_bob = alice.shared_secret(&bob.public_key()).unwrap();
        let bob_to_alice = bob.shared_secret(&alice.public_key()).unwrap();

        assert_eq!(alice_to_bob, bob_to_alice, "ECDH shared secret simetrik olmalı");
    }
}
