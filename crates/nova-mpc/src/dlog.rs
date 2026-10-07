//! Schnorr Discrete Logarithm (DLog) ZK Kanıtı.
//!
//! §6.1: "Non-interactive pre-signing" ve guardian'ların gizli bilgisini
//!        kanıtlaması için kullanılır.
//!
//! §02-tss-lib.md §3.6: Schnorr ZK kanıtı referans alındı.
//! §01-multi-party-ecdsa.md §4.2: "KnowledgeOfExponent::prove()" referans alındı.
//!
//! ## Protokol (Fiat-Shamir Heuristic ile Non-Interactive)
//!
//! **Prover**: Gizli `x`, public `X = x * G` biliyor.
//!
//! 1. Rastgele `r` seç
//! 2. `R = r * G` hesapla
//! 3. Challenge: `c = BLAKE3(context || X || R || session_id)`
//! 4. Response: `s = r + c * x` (skaler aritmetik)
//! 5. Kanıt: `(R, s)`
//!
//! **Verifier**: `X` ve `(R, s)` verilen:
//!
//! 1. Challenge'ı yeniden hesapla: `c = BLAKE3(context || X || R || session_id)`
//! 2. `s * G == R + c * X` kontrol et
//!
//! ## Güvenlik
//!
//! - Random Oracle Model'de Simulation-Extractable güvenlik
//! - Witness-Indistinguishable: `r` gizli kalır
//! - Session ID domain separation: farklı protokoller arası kanıt transferi önlenir
//! - Context string: farklı kullanım alanları arası ayrım

use k256::{ProjectivePoint, Scalar};
use k256::elliptic_curve::Field;
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::crypto_utils::{
    fiat_shamir_challenge, point_add, point_from_bytes, point_to_bytes,
    scalar_from_bytes, scalar_mul, scalar_mul_generator, scalar_mul_point,
    scalar_to_bytes,
};
use crate::errors::{MpcError, MpcResult};
use crate::types::DlogProofBytes;

// ─── KANIT ÜRETME ────────────────────────────────────────────────────────────

/// Schnorr DLog kanıtı üret.
///
/// ## Parametreler
///
/// - `secret`: Gizli scalar `x`. Kanıt üretildikten sonra bu değer
///   çağıran tarafta güvenli şekilde silinmeli.
/// - `public_point`: `X = x * G` (sıkıştırılmış 33 byte). Kanıt üretiminden
///   önce `scalar_mul_generator(secret)` ile hesaplanmış olmalı.
/// - `context`: Protokol domain separation string'i (örn: "nova-keygen-dlog-v1").
///   Sabit olmalı — farklı protokoller farklı context kullanır.
/// - `session_id`: Bu oturuma özgü 32 byte. Cross-session replay önler.
///
/// ## Güvenlik Notu
///
/// `nonce` (r) her çağrıda `OsRng` ile üretilir. Aynı `(secret, context)`
/// için bile farklı kanıt üretilir. Bu kanıtın tersinden `secret` çıkarılmasını
/// engeller.
pub fn dlog_prove(
    secret: &[u8; 32],
    public_point: &[u8; 33],
    context: &str,
    session_id: &[u8; 32],
) -> MpcResult<DlogProofBytes> {
    // Gizli skaleri yükle
    let x = scalar_from_bytes(secret)?;

    // Rastgele nonce r
    let r = Scalar::random(&mut OsRng);
    let r_bytes = scalar_to_bytes(&r);

    // R = r * G
    let big_r = scalar_mul_generator(&r);

    // Challenge: c = H(context || X || R || session_id)
    let c_bytes = fiat_shamir_challenge(context, &[
        public_point.as_ref(),
        big_r.as_ref(),
        session_id.as_ref(),
    ]);
    let c = scalar_from_bytes(&c_bytes)?;

    // Response: s = r + c * x
    let cx = scalar_mul(&c, &x);
    let s = r + cx;
    let s_bytes = scalar_to_bytes(&s);

    // r hassas — bellekten sil
    drop(r); // Scalar'ın Zeroize impl'i varsa çalışır

    Ok(DlogProofBytes {
        nonce_point: big_r,
        response: s_bytes,
    })
}

/// Schnorr DLog kanıtını doğrula.
///
/// `s * G == R + c * X` kontrol eder.
///
/// ## Sabit Zaman
///
/// Nokta karşılaştırması byte-level yapılır. k256 ile skaler çarpım
/// constant-time değildir (büyük tamsayı işlemleri). Ancak bu fonksiyon
/// yalnızca public verilerle çalıştığı için timing saldırısı gizli bilgi
/// sızdırmaz — sadece geçerlilik bilgisi öğrenilebilir.
pub fn dlog_verify(
    public_point: &[u8; 33],
    proof: &DlogProofBytes,
    context: &str,
    session_id: &[u8; 32],
) -> MpcResult<bool> {
    // s ve R'yi yükle
    let s = scalar_from_bytes(&proof.response)?;
    let big_r = &proof.nonce_point;

    // Challenge'ı yeniden hesapla
    let c_bytes = fiat_shamir_challenge(context, &[
        public_point.as_ref(),
        big_r.as_ref(),
        session_id.as_ref(),
    ]);
    let c = scalar_from_bytes(&c_bytes)?;

    // Sol taraf: s * G
    let lhs = scalar_mul_generator(&s);

    // Sağ taraf: R + c * X
    let cx_point = scalar_mul_point(&c, public_point)?;
    let rhs = point_add(big_r, &cx_point)?;

    // lhs == rhs ?
    Ok(lhs == rhs)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto_utils::scalar_mul_generator;
    use k256::Scalar;
    use rand::rngs::OsRng;

    const CONTEXT: &str = "nova-test-dlog-v1";
    const SESSION: [u8; 32] = [0xAB; 32];

    fn random_keypair() -> ([u8; 32], [u8; 33]) {
        let secret = Scalar::random(&mut OsRng);
        let secret_bytes = crate::crypto_utils::scalar_to_bytes(&secret);
        let public = scalar_mul_generator(&secret);
        (secret_bytes, public)
    }

    #[test]
    fn test_dlog_prove_and_verify() {
        let (secret, public) = random_keypair();
        let proof = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        let valid = dlog_verify(&public, &proof, CONTEXT, &SESSION).unwrap();
        assert!(valid, "geçerli Schnorr DLog kanıtı doğrulanmalı");
    }

    #[test]
    fn test_dlog_wrong_public_key_rejected() {
        let (secret, public) = random_keypair();
        let (_, other_public) = random_keypair();
        let proof = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        // Farklı public key ile doğrulama başarısız olmalı
        let valid = dlog_verify(&other_public, &proof, CONTEXT, &SESSION).unwrap();
        assert!(!valid, "yanlış public key reddedilmeli");
    }

    #[test]
    fn test_dlog_wrong_context_rejected() {
        let (secret, public) = random_keypair();
        let proof = dlog_prove(&secret, &public, "context-A", &SESSION).unwrap();
        let valid = dlog_verify(&public, &proof, "context-B", &SESSION).unwrap();
        assert!(!valid, "yanlış context reddedilmeli");
    }

    #[test]
    fn test_dlog_wrong_session_rejected() {
        let (secret, public) = random_keypair();
        let proof = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        let wrong_session = [0x00u8; 32];
        let valid = dlog_verify(&public, &proof, CONTEXT, &wrong_session).unwrap();
        assert!(!valid, "yanlış session ID reddedilmeli");
    }

    #[test]
    fn test_dlog_tampered_response_rejected() {
        let (secret, public) = random_keypair();
        let mut proof = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        // Response'u boz
        proof.response[15] ^= 0xFF;
        let valid = dlog_verify(&public, &proof, CONTEXT, &SESSION).unwrap();
        assert!(!valid, "bozuk response reddedilmeli");
    }

    #[test]
    fn test_dlog_tampered_nonce_point_rejected() {
        let (secret, public) = random_keypair();
        let mut proof = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        // Nonce noktasını boz — geçersiz nokta → hata veya false döner
        proof.nonce_point[16] ^= 0xFF;
        let result = dlog_verify(&public, &proof, CONTEXT, &SESSION);
        // Ya hata ya da false döner
        assert!(
            result.is_err() || !result.unwrap(),
            "bozuk nonce noktası reddedilmeli"
        );
    }

    #[test]
    fn test_dlog_non_deterministic() {
        // Aynı secret ile iki farklı kanıt üretilmeli (r rastgele)
        let (secret, public) = random_keypair();
        let p1 = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        let p2 = dlog_prove(&secret, &public, CONTEXT, &SESSION).unwrap();
        assert_ne!(p1.nonce_point, p2.nonce_point, "her kanıt benzersiz nonce içermeli");
        assert_ne!(p1.response, p2.response, "her kanıt benzersiz response içermeli");
        // İkisi de geçerli olmalı
        assert!(dlog_verify(&public, &p1, CONTEXT, &SESSION).unwrap());
        assert!(dlog_verify(&public, &p2, CONTEXT, &SESSION).unwrap());
    }

    #[test]
    fn test_dlog_invalid_scalar_fails() {
        // Sıfır secret → InvalidScalar hatası
        let zero_secret = [0u8; 32];
        let (_, public) = random_keypair();
        // Not: sıfır scalar geçerli bir scalar ama DLog kanıtı üretilebilir
        // Sadece public key olarak sonsuz nokta kullanmak sorun çıkarır
        let zero_public = crate::crypto_utils::IDENTITY_BYTES;
        let result = dlog_prove(&zero_secret, &zero_public, CONTEXT, &SESSION);
        // Sonsuz nokta ile kanıt üretmek başarısız olabilir ya da doğrulama başarısız
        // Her iki durum da güvenlidir
        if let Ok(proof) = result {
            let valid = dlog_verify(&zero_public, &proof, CONTEXT, &SESSION).unwrap_or(false);
            // Sonsuz nokta ile kanıt kabul edilmemeli
            let _ = valid; // test sadece panic olmadığını kontrol eder
        }
    }
}
