//! Pedersen DKG (Distributed Key Generation).
//!
//! §6.1.4 — Threshold şifreleme için dağıtık anahtar üretimi.
//! Referans: rolling-shutter DKG (03-rolling-shutter.md §Pedersen DKG)
//! 4 faz: Dealing → Accusing → Apologizing → Finalized
//!
//! ## Protokol
//! Round 1 (Dealing): Her katılımcı i:
//!   - f_i(X) = a_{i,0} + a_{i,1}·X + ... + a_{i,t-1}·X^{t-1} (rastgele polinom)
//!   - s_{i,j} = f_i(j) — share for participant j
//!   - A_{i,k} = a_{i,k}·G — commitment to coefficients (Feldman VSS)
//!   - Yayın: {A_{i,k}} + şifreli {s_{i,j} for j≠i}
//!
//! Round 2 (Verification): Her katılımcı j alınan share'i doğrula:
//!   - s_{i,j}·G == Σ_k A_{i,k}·j^k
//!
//! Round 3 (Finalization): Toplama:
//!   - sk_j = Σ_i s_{i,j} — threshold secret key share
//!   - pk = Σ_i A_{i,0} — threshold public key

use curve25519_dalek::{
    ristretto::RistrettoPoint,
    scalar::Scalar,
    traits::MultiscalarMul,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::errors::{CryptoError, CryptoResult};
use super::elgamal::{SecretKeyShare, ThresholdPublicKey};

/// DKG katılımcı parametreleri.
#[derive(Debug, Clone)]
pub struct DkgParams {
    /// Toplam katılımcı sayısı (n).
    pub n: u8,
    /// Threshold (t).
    pub t: u8,
    /// Bu katılımcının index'i (1-indexed).
    pub my_index: u8,
}

impl DkgParams {
    /// Yeni DKG parametresi.
    pub fn new(n: u8, t: u8, my_index: u8) -> CryptoResult<Self> {
        if t == 0 || t > n {
            return Err(CryptoError::InvalidParameter(
                format!("geçersiz threshold: t={t} n={n}"),
            ));
        }
        if my_index == 0 || my_index > n {
            return Err(CryptoError::InvalidParameter(
                format!("geçersiz index: {my_index}, 1..={n} olmalı"),
            ));
        }
        Ok(Self { n, t, my_index })
    }
}

/// Feldman VSS katsayı commitment'ları.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeldmanCommitments {
    /// A_k = a_k · G, k = 0..t-1 (compressed Ristretto, her biri 32 byte).
    pub commitments: Vec<[u8; 32]>,
}

/// DKG Round 1 çıktısı.
pub struct DkgRound1Output {
    /// Bu katılımcının Feldman commitment'ları.
    pub feldman: FeldmanCommitments,
    /// Diğer katılımcılara gönderilecek share'ler.
    /// share[j] = f(j+1) — j. katılımcının share'i (j=0 → index=1)
    pub shares: Vec<Scalar>,
    /// Gizli polinom katsayıları (dahili, asla yayınlanmaz).
    coefficients: Vec<Scalar>,
}

impl Drop for DkgRound1Output {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for c in &mut self.coefficients {
            *c = Scalar::ZERO;
        }
        for s in &mut self.shares {
            *s = Scalar::ZERO;
        }
    }
}

/// DKG Round 1: Polinom oluştur, share dağıt.
pub fn dkg_round1(params: &DkgParams) -> DkgRound1Output {
    let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    // Rastgele polinom katsayıları: a_0, a_1, ..., a_{t-1}
    let coefficients: Vec<Scalar> = (0..params.t).map(|_| Scalar::random(&mut OsRng)).collect();

    // Feldman commitment'lar: A_k = a_k · G
    let commitments: Vec<[u8; 32]> = coefficients
        .iter()
        .map(|a_k| (a_k * g).compress().to_bytes())
        .collect();

    // Share'ler: f(j) = Σ_k a_k · j^k, j = 1..n
    let shares: Vec<Scalar> = (1..=(params.n as u64))
        .map(|j| {
            let x = Scalar::from(j);
            let mut result = Scalar::ZERO;
            let mut x_pow = Scalar::ONE;
            for a_k in &coefficients {
                result += a_k * x_pow;
                x_pow *= x;
            }
            result
        })
        .collect();

    DkgRound1Output {
        feldman: FeldmanCommitments { commitments },
        shares,
        coefficients,
    }
}

/// DKG Round 2: Alınan share'i Feldman commitment'a göre doğrula.
///
/// `share_value`: s_{i,j} — katılımcı i'den alınan share for me (j=my_index)
/// `feldman`: Katılımcı i'nin yayınladığı Feldman commitment'lar
/// `my_index`: Benim index'im (j)
pub fn dkg_round2_verify(
    share_value: &Scalar,
    feldman: &FeldmanCommitments,
    my_index: u8,
) -> CryptoResult<()> {
    use curve25519_dalek::ristretto::CompressedRistretto;

    let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    // LHS: s_{i,j} · G
    let lhs = share_value * g;

    // RHS: Σ_k A_{i,k} · j^k
    let j = Scalar::from(my_index as u64);
    let mut j_pow = Scalar::ONE;
    let mut rhs = RistrettoPoint::default();

    for commitment_bytes in &feldman.commitments {
        let a_k = CompressedRistretto::from_slice(commitment_bytes)
            .map_err(|_| CryptoError::Commitment("commitment decompress hatası".into()))?
            .decompress()
            .ok_or_else(|| CryptoError::Commitment("commitment geçersiz nokta".into()))?;

        rhs += j_pow * a_k;
        j_pow *= j;
    }

    if lhs == rhs {
        Ok(())
    } else {
        Err(CryptoError::Commitment(
            format!("Feldman doğrulama başarısız: katılımcı share geçersiz")
        ))
    }
}

/// DKG Round 3: Share'leri topla → kişisel sk_share ve threshold pk.
///
/// `received_shares`: Tüm katılımcılardan (i=1..n) alınan share'ler
///   received_shares[i] = f_i(my_index)
/// `all_feldman`: Tüm katılımcıların Feldman commitment'ları
///   all_feldman[i].commitments[0] = A_{i,0} = a_{i,0}·G (pk payı)
/// `my_index`: Bu katılımcının 1-indexed index'i (1..=n).
///
/// ## NOVA-RED-CRYPTO-002 Güvenlik Düzeltmesi
///
/// Önceki sürümde `my_index = received_shares.len()` kullanılıyordu.
/// Bu yalnızca n katılımcının tümünden share alındığında tesadüfen doğruydu.
/// Gerçekte `my_index` farklı olabilir (örn. n=5, my_index=3 → yanlış index=5).
/// Lagrange katsayısı yanlış hesaplanır → imza başarısız veya güvenlik zafiyeti.
pub fn dkg_round3_finalize(
    received_shares: &[Scalar],
    all_feldman: &[FeldmanCommitments],
    my_index: u8,
) -> CryptoResult<(SecretKeyShare, ThresholdPublicKey, u8)> {
    use curve25519_dalek::ristretto::CompressedRistretto;

    if received_shares.is_empty() || all_feldman.is_empty() {
        return Err(CryptoError::Commitment("boş share/feldman listesi".into()));
    }

    // sk_j = Σ_i f_i(j)
    let sk_sum = received_shares.iter().fold(Scalar::ZERO, |acc, s| acc + s);

    // pk = Σ_i A_{i,0} = Σ_i a_{i,0}·G
    let mut pk_sum = RistrettoPoint::default();
    for feldman in all_feldman {
        if feldman.commitments.is_empty() {
            return Err(CryptoError::Commitment("boş Feldman commitment".into()));
        }
        let a_i0 = CompressedRistretto::from_slice(&feldman.commitments[0])
            .map_err(|_| CryptoError::Commitment("pk commitment decompress hatası".into()))?
            .decompress()
            .ok_or_else(|| CryptoError::Commitment("pk commitment geçersiz nokta".into()))?;
        pk_sum += a_i0;
    }

    // NOVA-RED-CRYPTO-002: my_index parametreden alınır (received_shares.len() değil).
    if my_index == 0 {
        return Err(CryptoError::Commitment("geçersiz my_index: 0 olamaz".into()));
    }
    // NOVA-RED-CRYPTO-003: pub_key_share hesapla — DLEQ proof için gerekli.
    let sk_share = SecretKeyShare::new(my_index, sk_sum);
    let threshold_pk = ThresholdPublicKey::from_bytes(pk_sum.compress().to_bytes());

    Ok((sk_share, threshold_pk, my_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_feldman(n: u8, t: u8, index: u8) -> (FeldmanCommitments, Vec<Scalar>) {
        let params = DkgParams::new(n, t, index).unwrap();
        let round1 = dkg_round1(&params);
        (round1.feldman.clone(), round1.shares.clone())
    }

    /// NOVA-RED-CRYPTO-002: Doğru my_index ile sk_share index'i eşleşmeli.
    #[test]
    fn test_dkg_round3_correct_index() {
        let n = 3u8;
        let t = 2u8;
        let my_index = 2u8; // katılımcı 2

        // 3 katılımcı, her biri share paylaşıyor
        let mut received_shares: Vec<Scalar> = Vec::new();
        let mut all_feldman: Vec<FeldmanCommitments> = Vec::new();

        for i in 1u8..=n {
            let params = DkgParams::new(n, t, i).unwrap();
            let round1 = dkg_round1(&params);
            // share for katılımcı my_index=2 → shares[my_index-1] = shares[1]
            received_shares.push(round1.shares[(my_index - 1) as usize]);
            all_feldman.push(round1.feldman.clone());
        }

        let (sk_share, _pk, idx) = dkg_round3_finalize(
            &received_shares,
            &all_feldman,
            my_index,
        ).unwrap();

        // Index doğru mu?
        assert_eq!(idx, my_index, "my_index doğru olmalı");
        assert_eq!(sk_share.index, my_index, "sk_share.index doğru olmalı");
    }

    /// NOVA-RED-CRYPTO-002: Eski kod received_shares.len() kullanıyordu → yanlış index.
    /// n=5, my_index=3 durumunda len()=5, my_index=3 — fark edilir mi?
    #[test]
    fn test_dkg_round3_index_not_equal_to_len() {
        let n = 5u8;
        let t = 3u8;
        let my_index = 3u8; // katılımcı 3, ama received_shares.len()=5 olacak

        let mut received_shares: Vec<Scalar> = Vec::new();
        let mut all_feldman: Vec<FeldmanCommitments> = Vec::new();

        for i in 1u8..=n {
            let params = DkgParams::new(n, t, i).unwrap();
            let round1 = dkg_round1(&params);
            received_shares.push(round1.shares[(my_index - 1) as usize]);
            all_feldman.push(round1.feldman.clone());
        }

        assert_eq!(received_shares.len(), 5); // len = n = 5
        assert_ne!(received_shares.len() as u8, my_index); // 5 != 3

        let (sk_share, _pk, idx) = dkg_round3_finalize(
            &received_shares,
            &all_feldman,
            my_index,
        ).unwrap();

        // Düzeltilmiş kod my_index=3 döndürmeli (eski kod 5 döndürürdü)
        assert_eq!(idx, 3u8, "eski kod yanlışlıkla 5 döndürürdü");
        assert_eq!(sk_share.index, 3u8);
    }

    /// my_index=0 geçersiz.
    #[test]
    fn test_dkg_round3_zero_index_rejected() {
        let shares = vec![Scalar::ONE];
        let feldman = FeldmanCommitments {
            commitments: vec![[1u8; 32]],
        };
        let result = dkg_round3_finalize(&shares, &[feldman], 0);
        assert!(result.is_err());
    }
}
