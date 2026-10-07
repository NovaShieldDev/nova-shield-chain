//! BLS12-381 imzalama — konsensüs oylama ve aggregate imzalar.
//!
//! §5.10 — Validator oylama imzaları BLS kullanır.
//! BLS aggregate: n validator imzası → 1 imza (alan: O(1) yerine O(n)).
//!
//! ## Özellikler
//! - G1: Public key (48 byte compressed)
//! - G2: İmza (96 byte compressed)
//! - Aggregate: n imzayı 1'e indir → blok boyutunu küçült
//! - `blst` kütüphanesi: en hızlı BLS12-381 implementasyonu
//!
//! ## Referans
//! - Polkadot BEEFY (10-polkadot-sdk.md): BLS12-381 Ethereum bridge
//! - Ethereum beacon chain (Altair) BLS aggregate signatures
//! - `blst` crate (Supranational — Ethereum Foundation destekli)

use blst::{
    min_pk::{AggregateSignature, PublicKey as BlstPk, SecretKey as BlstSk, Signature as BlstSig},
    BLST_ERROR,
};
use nova_primitives::crypto::{BlsPublicKey, BlsSignature, Signature};
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{CryptoError, CryptoResult};

// BLS domain separation tag — beacon chain uyumlu
const DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

// ─── KEY PAIR ────────────────────────────────────────────────────────────────

/// BLS12-381 key pair.
/// min_pk varyantı: G1'de pubkey, G2'de imza.
pub struct BlsKeypair {
    secret: BlstSk,
    public: BlstPk,
}

impl Zeroize for BlsKeypair {
    fn zeroize(&mut self) {
        let mut raw = self.secret.to_bytes();
        raw.zeroize();
    }
}

impl Drop for BlsKeypair {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl BlsKeypair {
    /// Rastgele yeni BLS key pair üret.
    /// IKM (Input Key Material): 32 byte rastgele
    pub fn generate() -> CryptoResult<Self> {
        let mut ikm = [0u8; 32];
        OsRng.fill_bytes(&mut ikm);
        Self::from_ikm(&ikm)
    }

    /// IKM'den deterministik key pair türet.
    /// BLS key gen spec: IKM → SK → PK.
    pub fn from_ikm(ikm: &[u8; 32]) -> CryptoResult<Self> {
        let secret = BlstSk::key_gen(ikm, &[])
            .map_err(|e| CryptoError::BlsKeygen(format!("{e:?}")))?;
        let public = secret.sk_to_pk();
        Ok(Self { secret, public })
    }

    /// Compressed G1 public key (48 byte).
    pub fn bls_public_key(&self) -> BlsPublicKey {
        BlsPublicKey::from_bytes(self.public.to_bytes())
    }

    /// Nova Signature wrapper olarak BLS imzası.
    pub fn sign(&self, message: &[u8]) -> CryptoResult<Signature> {
        let sig = self.secret.sign(message, DST, &[]);
        let sig_bytes = sig.to_bytes();
        Ok(Signature::new_bls(sig_bytes.to_vec()))
    }

    /// Çiğ blst imzası.
    pub fn sign_raw(&self, message: &[u8]) -> BlsSignature {
        let sig = self.secret.sign(message, DST, &[]);
        BlsSignature::from_bytes(sig.to_bytes())
    }

    /// Secret key byte'larını döndür (güvenli saklama için).
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }
}

// ─── DOĞRULAYICI ─────────────────────────────────────────────────────────────

/// BLS imza doğrulayıcı.
pub struct BlsVerifier;

impl BlsVerifier {
    /// Tekil BLS imzasını doğrula.
    pub fn verify(
        message: &[u8],
        signature: &BlsSignature,
        public_key: &BlsPublicKey,
    ) -> CryptoResult<()> {
        let pk = BlstPk::from_bytes(public_key.as_bytes())
            .map_err(|e| CryptoError::BlsVerify(format!("{e:?}")))?;

        let sig = BlstSig::from_bytes(signature.as_bytes())
            .map_err(|e| CryptoError::BlsVerify(format!("{e:?}")))?;

        let result = sig.verify(true, message, DST, &[], &pk, true);
        match result {
            BLST_ERROR::BLST_SUCCESS => Ok(()),
            e => Err(CryptoError::BlsVerify(format!("{e:?}"))),
        }
    }

    /// Aggregate BLS imzasını doğrula.
    ///
    /// Tüm message'lar farklı olmalı (rogue key saldırısı önlemi).
    /// Referans: Ethereum beacon chain batch verification.
    pub fn verify_aggregate(
        messages: &[&[u8]],
        signatures: &[&BlsSignature],
        public_keys: &[&BlsPublicKey],
    ) -> CryptoResult<()> {
        if messages.len() != signatures.len() || messages.len() != public_keys.len() {
            return Err(CryptoError::InvalidParameter(
                "mesaj, imza ve public key sayıları eşit olmalı".into(),
            ));
        }
        if messages.is_empty() {
            return Err(CryptoError::InvalidParameter("en az 1 eleman gerekli".into()));
        }

        // Tüm imzaları aggregate et
        let blst_sigs: Vec<BlstSig> = signatures
            .iter()
            .map(|s| {
                BlstSig::from_bytes(s.as_bytes())
                    .map_err(|e| CryptoError::BlsAggregate(format!("{e:?}")))
            })
            .collect::<CryptoResult<_>>()?;

        let sig_refs: Vec<&BlstSig> = blst_sigs.iter().collect();
        let mut agg_sig = AggregateSignature::from_signature(sig_refs[0]);
        for s in &sig_refs[1..] {
            agg_sig.add_signature(s, true)
                .map_err(|e| CryptoError::BlsAggregate(format!("{e:?}")))?;
        }

        let final_sig = agg_sig.to_signature();

        // Public key'leri dönüştür
        let blst_pks: Vec<BlstPk> = public_keys
            .iter()
            .map(|pk| {
                BlstPk::from_bytes(pk.as_bytes())
                    .map_err(|e| CryptoError::BlsVerify(format!("{e:?}")))
            })
            .collect::<CryptoResult<_>>()?;

        let pk_refs: Vec<&BlstPk> = blst_pks.iter().collect();

        let result = final_sig.aggregate_verify(
            true,
            messages,
            DST,
            &pk_refs,
            true,
        );

        match result {
            BLST_ERROR::BLST_SUCCESS => Ok(()),
            e => Err(CryptoError::BlsVerify(format!("aggregate doğrulama başarısız: {e:?}"))),
        }
    }

    /// Tüm validatörlerin aynı mesajı imzaladığı durum (consensus vote).
    /// Daha verimli: tek aggregate pk + tek imza doğrulama.
    pub fn verify_aggregate_same_msg(
        message: &[u8],
        signatures: &[&BlsSignature],
        public_keys: &[&BlsPublicKey],
    ) -> CryptoResult<()> {
        if signatures.is_empty() || public_keys.is_empty() {
            return Err(CryptoError::InvalidParameter("boş liste".into()));
        }

        // İmzaları aggregate et
        let blst_sigs: Vec<BlstSig> = signatures
            .iter()
            .map(|s| {
                BlstSig::from_bytes(s.as_bytes())
                    .map_err(|e| CryptoError::BlsAggregate(format!("{e:?}")))
            })
            .collect::<CryptoResult<_>>()?;

        let sig_refs: Vec<&BlstSig> = blst_sigs.iter().collect();
        let mut agg_sig = AggregateSignature::from_signature(sig_refs[0]);
        for s in &sig_refs[1..] {
            agg_sig.add_signature(s, true)
                .map_err(|e| CryptoError::BlsAggregate(format!("{e:?}")))?;
        }
        let final_sig = agg_sig.to_signature();

        // Public key'leri aggregate et
        let blst_pks: Vec<BlstPk> = public_keys
            .iter()
            .map(|pk| {
                BlstPk::from_bytes(pk.as_bytes())
                    .map_err(|e| CryptoError::BlsVerify(format!("{e:?}")))
            })
            .collect::<CryptoResult<_>>()?;

        use blst::min_pk::AggregatePublicKey;
        let pk_refs: Vec<&BlstPk> = blst_pks.iter().collect();
        let mut agg_pk = AggregatePublicKey::from_public_key(pk_refs[0]);
        for pk in &pk_refs[1..] {
            agg_pk.add_public_key(pk, true)
                .map_err(|e| CryptoError::BlsAggregate(format!("{e:?}")))?;
        }
        let final_pk = agg_pk.to_public_key();

        let result = final_sig.fast_aggregate_verify(true, message, DST, &pk_refs);
        match result {
            BLST_ERROR::BLST_SUCCESS => Ok(()),
            e => Err(CryptoError::BlsVerify(format!("fast aggregate başarısız: {e:?}"))),
        }
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bls_keygen() {
        let kp = BlsKeypair::generate().unwrap();
        let pk = kp.bls_public_key();
        assert_eq!(pk.as_bytes().len(), 48, "BLS public key 48 byte olmalı");
    }

    #[test]
    fn test_bls_sign_verify() {
        let kp = BlsKeypair::generate().unwrap();
        let message = b"Nova Shield consensus vote slot=100";
        let sig = kp.sign_raw(message);
        let pk = kp.bls_public_key();

        BlsVerifier::verify(message, &sig, &pk).expect("BLS doğrulama başarılı olmalı");
    }

    #[test]
    fn test_bls_wrong_message() {
        let kp = BlsKeypair::generate().unwrap();
        let sig = kp.sign_raw(b"original");
        let pk = kp.bls_public_key();

        let result = BlsVerifier::verify(b"tampered", &sig, &pk);
        assert!(result.is_err(), "yanlış mesaj reddedilmeli");
    }

    #[test]
    fn test_bls_aggregate_same_msg() {
        let kp1 = BlsKeypair::generate().unwrap();
        let kp2 = BlsKeypair::generate().unwrap();
        let kp3 = BlsKeypair::generate().unwrap();

        let message = b"block-hash-for-vote";
        let s1 = kp1.sign_raw(message);
        let s2 = kp2.sign_raw(message);
        let s3 = kp3.sign_raw(message);

        let pk1 = kp1.bls_public_key();
        let pk2 = kp2.bls_public_key();
        let pk3 = kp3.bls_public_key();

        BlsVerifier::verify_aggregate_same_msg(
            message,
            &[&s1, &s2, &s3],
            &[&pk1, &pk2, &pk3],
        )
        .expect("aggregate doğrulama başarılı olmalı");
    }

    #[test]
    fn test_bls_from_ikm() {
        let ikm = [7u8; 32];
        let kp1 = BlsKeypair::from_ikm(&ikm).unwrap();
        let kp2 = BlsKeypair::from_ikm(&ikm).unwrap();

        assert_eq!(
            kp1.bls_public_key().as_bytes(),
            kp2.bls_public_key().as_bytes(),
            "aynı IKM → aynı public key"
        );
    }
}
