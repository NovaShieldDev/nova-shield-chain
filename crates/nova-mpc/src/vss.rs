//! Feldman Doğrulanabilir Gizli Paylaşım (Verifiable Secret Sharing).
//!
//! §5.1.2: "Threshold (3/5): telefon, laptop, guardian1, guardian2, time-lock vault"
//! §01-multi-party-ecdsa.md §4.2: Feldman VSS referans implementasyonu incelendi.
//! §02-tss-lib.md §3.5: tss-lib VSS tasarımı referans alındı.
//!
//! ## Feldman VSS Nedir?
//!
//! Shamir Gizli Paylaşım üzerine polinom katsayı taahhütleri (EC noktaları)
//! ekleyen bir şemadır. Bu sayede her alıcı kendi payının geçerliliğini
//! halka açık taahhütlerle doğrulayabilir.
//!
//! ## Polinom
//!
//! `f(x) = a_0 + a_1*x + a_2*x^2 + ... + a_t*x^t`
//!
//! - `a_0 = secret` (gizli)
//! - `a_1..a_t`: Rastgele seçilen katsayılar
//!
//! ## Taahhütler
//!
//! `C_j = a_j * G` (j = 0..=t)
//!
//! ## Doğrulama
//!
//! Party `k` payı `s_k = f(k)` için:
//! `s_k * G == sum(C_j * k^j, j=0..=t)`

use k256::{ProjectivePoint, Scalar};
use k256::elliptic_curve::Field;
use rand::rngs::OsRng;
use rand_core::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto_utils::{
    point_add, point_from_bytes, point_to_bytes, scalar_add,
    scalar_from_bytes, scalar_from_u32, scalar_mul, scalar_mul_generator,
    scalar_mul_point, scalar_to_bytes, IDENTITY_BYTES,
};
use crate::errors::{MpcError, MpcResult};
use crate::types::{PartyIndex, VssCommitment};

// ─── FONKSİYON SONUCU ────────────────────────────────────────────────────────

/// Feldman VSS üretim sonucu.
///
/// `shares[i]` = party `i+1`'e gönderilecek pay (`f(i+1)`).
/// Paylar gizlidir — sadece hedef partiye gönderilir.
/// `commitment` herkese yayımlanır.
#[derive(Debug, ZeroizeOnDrop)]
pub struct VssOutput {
    /// `shares[i]` = f(i+1) — party i+1'e gönderilecek gizli pay.
    /// Uzunluk: `n` (toplam parti sayısı).
    #[zeroize(skip)]
    pub shares: Vec<ShareValue>,

    /// Polinom katsayı taahhütleri — herkese yayımlanır.
    #[zeroize(skip)]
    pub commitment: VssCommitment,
}

/// Tek bir VSS payı — gizli, ZeroizeOnDrop.
#[derive(Debug, Zeroize, ZeroizeOnDrop)]
pub struct ShareValue {
    /// Hangi partiye ait.
    #[zeroize(skip)]
    pub party: PartyIndex,
    /// `f(party.get())` skaleri (32 byte).
    pub value: [u8; 32],
}

// ─── VSS ÜRETİMİ ─────────────────────────────────────────────────────────────

/// Feldman VSS yürüt.
///
/// ## Parametreler
///
/// - `threshold`: İmza için gereken minimum parti sayısı (`t`).
///   Polinom derecesi `t-1` olur → t parti ile sır yeniden oluşturulabilir.
/// - `party_count`: Toplam parti sayısı (`n`). `n >= t` zorunlu.
/// - `secret`: Paylaşılacak gizli değer (32 byte scalar). `None` → rastgele üretilir.
/// - `party_index`: Bu payı üreten partinin indeksi.
///
/// ## Çıktı
///
/// - `shares[i]`: Party `i+1` için pay. Sadece ona P2P gönderilir.
/// - `commitment`: Herkese broadcast edilir.
pub fn feldman_vss_generate(
    threshold: u8,
    party_count: u32,
    secret: Option<[u8; 32]>,
    party_index: PartyIndex,
) -> MpcResult<VssOutput> {
    if threshold == 0 || threshold as u32 > party_count {
        return Err(MpcError::InvalidThreshold {
            t: threshold,
            n: party_count as u8,
        });
    }

    let degree = (threshold as usize) - 1; // t-1 dereceli polinom → t nokta ile yeniden oluşturulur

    // Polinom katsayıları: a_0 = secret, a_1..a_{t-1} = rastgele
    let mut coefficients: Vec<Scalar> = Vec::with_capacity(threshold as usize);

    // a_0: ya verilen secret ya da rastgele
    let a0 = match secret {
        Some(s) => scalar_from_bytes(&s)?,
        None => Scalar::random(&mut OsRng),
    };
    coefficients.push(a0);

    // a_1..a_{t-1}: rastgele
    for _ in 0..degree {
        coefficients.push(Scalar::random(&mut OsRng));
    }

    // Taahhütler: C_j = a_j * G
    let commitment_points: Vec<[u8; 33]> = coefficients
        .iter()
        .map(scalar_mul_generator)
        .collect();

    // Paylar: f(k) = a_0 + a_1*k + ... + a_{t-1}*k^{t-1} — Horner yöntemi
    let mut shares = Vec::with_capacity(party_count as usize);
    for k in 1..=(party_count) {
        let x = scalar_from_u32(k);
        let share_scalar = horner_eval(&coefficients, &x);
        shares.push(ShareValue {
            party: PartyIndex(k),
            value: scalar_to_bytes(&share_scalar),
        });
    }

    // Katsayılar bellekten silinmeli
    let mut coefficients_zeroize: Vec<Scalar> = coefficients;
    for c in coefficients_zeroize.iter_mut() {
        // Scalar::zeroize() mevcut değilse bytes üzerinden yapalım
        let bytes_ptr = c as *mut Scalar;
        unsafe {
            std::ptr::write_bytes(bytes_ptr, 0, 1);
        }
    }
    drop(coefficients_zeroize);

    Ok(VssOutput {
        shares,
        commitment: VssCommitment {
            party: party_index,
            points: commitment_points,
        },
    })
}

/// Horner yöntemi ile polinom değerlendir.
///
/// `f(x) = a_0 + a_1*x + ... + a_{n-1}*x^{n-1}`
/// `     = a_0 + x*(a_1 + x*(a_2 + ... + x*a_{n-1}))`
fn horner_eval(coefficients: &[Scalar], x: &Scalar) -> Scalar {
    assert!(!coefficients.is_empty(), "polinom en az bir katsayı içermeli");

    let mut result = *coefficients.last().unwrap();
    for coeff in coefficients.iter().rev().skip(1) {
        // result = result * x + coeff
        result = scalar_mul(&result, x);
        result = scalar_add(&result, coeff);
    }
    result
}

// ─── VSS DOĞRULAMA ───────────────────────────────────────────────────────────

/// Verilen payın VSS taahhüdüyle uyuştuğunu doğrula.
///
/// Kontrol: `share_value * G == sum(C_j * party_index^j, j=0..=t-1)`
///
/// Bu işlem sabit zamanlı değildir — ancak yalnızca public verilerle
/// çalıştığı için timing attack sorunu yoktur.
pub fn feldman_vss_verify(
    share: &[u8; 32],
    party: PartyIndex,
    commitment: &VssCommitment,
) -> MpcResult<bool> {
    if commitment.is_empty() {
        return Err(MpcError::VssVerificationFailed { party: party.0 });
    }

    // Sol taraf: share_value * G
    let share_scalar = scalar_from_bytes(share)?;
    let lhs = scalar_mul_generator(&share_scalar);

    // Sağ taraf: sum(C_j * party_index^j)
    let x = scalar_from_u32(party.0);
    let rhs = feldman_eval_point(commitment, party)?;

    Ok(lhs == rhs)
}

/// VSS taahhüdünü `party` indeksinde değerlendir: `sum(C_j * x^j)`.
///
/// Bu, `f(x) * G`'yi party'nin payını görmeden hesaplar.
/// Herkesin doğrulayabileceği public işlemdir.
pub fn feldman_eval_point(commitment: &VssCommitment, party: PartyIndex) -> MpcResult<[u8; 33]> {
    if commitment.is_empty() {
        return Err(MpcError::InvalidShareCount { expected: 1, got: 0 });
    }

    let x = scalar_from_u32(party.0);
    let mut x_pow = {
        let mut b = [0u8; 32];
        b[31] = 1; // 1 (scalar birimi)
        scalar_from_bytes(&b)?
    }; // x^0 = 1

    let mut result = IDENTITY_BYTES; // Toplam başlangıcı: sonsuz nokta

    for point_bytes in &commitment.points {
        // term = C_j * x^j
        let term = scalar_mul_point(&x_pow, point_bytes)?;
        result = point_add(&result, &term)?;
        // x^{j+1} = x^j * x
        x_pow = scalar_mul(&x_pow, &x);
    }

    Ok(result)
}

// ─── LAGRANGE İNTERPOLASYONU ─────────────────────────────────────────────────

/// Lagrange katsayısı: `lambda_i = prod(x_j / (x_j - x_i)) for j in signers, j != i`.
///
/// §02-tss-lib.md §11.9: "Lagrange ağırlıklandırma" bölümünden alındı.
///
/// Bu katsayı threshold signing'de kısmi imzaları ağırlıklandırmak için kullanılır:
/// `s = sum(s_i * lambda_i)` = tam imza skaleri.
///
/// ## Sabit Zaman Notu
///
/// Bölme (modüler ters) sabit zamanlı değildir. Ancak bu işlem yalnızca
/// public party index'leri üzerinde çalışır → gizli veri sızdırmaz.
pub fn lagrange_coefficient(
    our_index: PartyIndex,
    signers: &[PartyIndex],
) -> MpcResult<Scalar> {
    let x_i = scalar_from_u32(our_index.0);

    let mut numerator = {
        let mut b = [0u8; 32];
        b[31] = 1;
        scalar_from_bytes(&b)?
    }; // 1

    let mut denominator = {
        let mut b = [0u8; 32];
        b[31] = 1;
        scalar_from_bytes(&b)?
    }; // 1

    for signer in signers {
        if signer.0 == our_index.0 {
            continue; // j != i
        }
        let x_j = scalar_from_u32(signer.0);

        // numerator *= x_j
        numerator = scalar_mul(&numerator, &x_j);

        // denominator *= (x_j - x_i)
        let diff = x_j - x_i;
        denominator = scalar_mul(&denominator, &diff);
    }

    let den_inv = crate::crypto_utils::scalar_invert(&denominator)?;
    Ok(scalar_mul(&numerator, &den_inv))
}

/// Shamir gizli paylarından sırrı yeniden oluştur (Lagrange interpolasyonu).
///
/// ## Parametreler
///
/// - `shares`: `(party_index, share_value)` çiftleri. En az `t` adet gerekli.
///
/// ## Güvenlik
///
/// Bu fonksiyon `t`'den az payla sırrı ortaya çıkarmaz — bilgi teorik gizlilik.
pub fn shamir_reconstruct(shares: &[(PartyIndex, [u8; 32])]) -> MpcResult<[u8; 32]> {
    if shares.is_empty() {
        return Err(MpcError::InsufficientShares { needed: 1, got: 0 });
    }

    let all_indices: Vec<PartyIndex> = shares.iter().map(|(idx, _)| *idx).collect();
    let mut secret = Scalar::ZERO;

    for (idx, share_bytes) in shares {
        let s_i = scalar_from_bytes(share_bytes)?;
        let lambda = lagrange_coefficient(*idx, &all_indices)?;
        // secret += s_i * lambda
        let term = scalar_mul(&s_i, &lambda);
        secret = secret + term;
    }

    Ok(scalar_to_bytes(&secret))
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Sıfırdan rastgele secret ile VSS üret, tüm payları doğrula.
    #[test]
    fn test_vss_generate_and_verify_all_shares() {
        let threshold = 3u8;
        let n = 5u32;
        let vss = feldman_vss_generate(threshold, n, None, PartyIndex(1)).unwrap();

        for share in &vss.shares {
            let valid = feldman_vss_verify(&share.value, share.party, &vss.commitment)
                .expect("doğrulama hatası olmamalı");
            assert!(
                valid,
                "party {} için pay geçersiz olmamalı",
                share.party.0
            );
        }
    }

    /// Verilen sabit secret ile üretilen VSS'nin payları doğrulanabilmeli.
    #[test]
    fn test_vss_known_secret_verify() {
        let secret: [u8; 32] = {
            let mut s = [0u8; 32];
            s[31] = 42; // scalar = 42
            s
        };
        let vss = feldman_vss_generate(2, 3, Some(secret), PartyIndex(1)).unwrap();

        for share in &vss.shares {
            let valid = feldman_vss_verify(&share.value, share.party, &vss.commitment).unwrap();
            assert!(valid, "bilinen secret ile pay doğrulanmalı");
        }
    }

    /// Değiştirilmiş pay reddedilmeli (Feldman bağlayıcılığı).
    #[test]
    fn test_vss_tampered_share_rejected() {
        let vss = feldman_vss_generate(2, 3, None, PartyIndex(1)).unwrap();

        // Pay değerini boz
        let mut bad_share = vss.shares[0].value;
        bad_share[31] ^= 0xFF; // son byte'ı flip

        let valid = feldman_vss_verify(&bad_share, vss.shares[0].party, &vss.commitment).unwrap();
        assert!(!valid, "bozuk pay reddedilmeli");
    }

    /// t=n=1 (degenerate case): tek parti, eşiğe eşit.
    #[test]
    fn test_vss_t1_n1() {
        let vss = feldman_vss_generate(1, 1, None, PartyIndex(1)).unwrap();
        assert_eq!(vss.shares.len(), 1);
        let valid = feldman_vss_verify(&vss.shares[0].value, vss.shares[0].party, &vss.commitment).unwrap();
        assert!(valid);
    }

    /// Geçersiz threshold hatası.
    #[test]
    fn test_vss_invalid_threshold() {
        assert!(
            feldman_vss_generate(0, 3, None, PartyIndex(1)).is_err(),
            "t=0 geçersiz"
        );
        assert!(
            feldman_vss_generate(4, 3, None, PartyIndex(1)).is_err(),
            "t>n geçersiz"
        );
    }

    /// Shamir reconstruct: t paydan sırrı geri al.
    #[test]
    fn test_shamir_reconstruct_3_of_5() {
        let secret: [u8; 32] = {
            let mut s = [0u8; 32];
            s[29] = 1;
            s[30] = 0;
            s[31] = 7; // scalar = 65543
            s
        };

        let vss = feldman_vss_generate(3, 5, Some(secret), PartyIndex(1)).unwrap();

        // Herhangi 3 pay ile yeniden oluştur
        let subset: Vec<(PartyIndex, [u8; 32])> = vss.shares[0..3]
            .iter()
            .map(|s| (s.party, s.value))
            .collect();

        let recovered = shamir_reconstruct(&subset).unwrap();
        assert_eq!(recovered, secret, "3/5 ile sır yeniden oluşturulmalı");
    }

    /// Başka 3 pay da aynı sonucu vermeli.
    #[test]
    fn test_shamir_reconstruct_different_subset() {
        let secret = {
            let mut s = [0u8; 32];
            s[31] = 99;
            s
        };

        let vss = feldman_vss_generate(3, 5, Some(secret), PartyIndex(1)).unwrap();

        let subset1: Vec<(PartyIndex, [u8; 32])> = vss.shares[0..3]
            .iter()
            .map(|s| (s.party, s.value))
            .collect();

        let subset2: Vec<(PartyIndex, [u8; 32])> = vss.shares[2..5]
            .iter()
            .map(|s| (s.party, s.value))
            .collect();

        let r1 = shamir_reconstruct(&subset1).unwrap();
        let r2 = shamir_reconstruct(&subset2).unwrap();
        assert_eq!(r1, r2, "farklı alt kümeler aynı sırrı vermeli");
    }

    /// Lagrange katsayısı doğrulaması: lambda değerleri 1'e toplanmalı.
    /// `sum(lambda_i * x_i) = sır` olduğu için `sum(lambda_i * x_i * G) = X`
    /// Bu test sadece doğruluk için.
    #[test]
    fn test_lagrange_coefficients_correct() {
        // 3 imzacı için Lagrange katsayılarını hesapla
        let signers = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];

        // Her imzacının Lagrange katsayısını hesapla
        let l1 = lagrange_coefficient(PartyIndex(1), &signers).unwrap();
        let l2 = lagrange_coefficient(PartyIndex(2), &signers).unwrap();
        let l3 = lagrange_coefficient(PartyIndex(3), &signers).unwrap();

        // f(0) = sum(f(i) * lambda_i)
        // Basit test: f(x) = 1 polinomu ile kontrol
        // f(1) = f(2) = f(3) = 1 → sum(1 * lambda_i) = 1
        let one: Scalar = {
            let mut b = [0u8; 32];
            b[31] = 1;
            scalar_from_bytes(&b).unwrap()
        };
        let sum = l1 + l2 + l3;
        assert_eq!(sum, one, "sabit polinom için Lagrange katsayıları toplamı 1 olmalı");
    }
}
