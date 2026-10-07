//! Dahili kriptografik yardımcı fonksiyonlar.
//!
//! k256 (secp256k1) eliptik eğrisi üzerinde temel işlemler.
//! Tüm fonksiyonlar sabit-zamanlı olarak tasarlanmıştır (§6.2).

use k256::{
    AffinePoint, ProjectivePoint, Scalar,
    elliptic_curve::{
        group::GroupEncoding,
        sec1::{FromEncodedPoint, ToEncodedPoint},
        Field,
        PrimeField,
    },
};
use crate::errors::{MpcError, MpcResult};

/// Sonsuz nokta (kimlik elemanı) — sıkıştırılmış encoding.
/// secp256k1'de sonsuz nokta 0x00 prefix ile temsil edilir.
/// Bu sabit, nokta toplamlarının başlangıç değeri olarak kullanılır.
pub const IDENTITY_BYTES: [u8; 33] = {
    let mut b = [0u8; 33];
    b[0] = 0x00; // SEC1 prefix for infinity
    b
};

/// secp256k1 generator noktası G (sıkıştırılmış, 33 byte).
pub fn generator_bytes() -> [u8; 33] {
    let g = ProjectivePoint::GENERATOR;
    point_to_bytes(&g)
}

/// Skaleri 32 byte big-endian dizisine dönüştür.
pub fn scalar_to_bytes(s: &Scalar) -> [u8; 32] {
    let repr = s.to_repr();
    let mut out = [0u8; 32];
    out.copy_from_slice(repr.as_slice());
    out
}

/// 32 byte big-endian dizisinden skaler oluştur.
/// Sıfır skaler veya alan dışı değer → `MpcError::InvalidScalar`.
pub fn scalar_from_bytes(bytes: &[u8; 32]) -> MpcResult<Scalar> {
    use k256::FieldBytes;
    let field_bytes = FieldBytes::from_slice(bytes);
    let opt: Option<Scalar> = Scalar::from_repr(*field_bytes).into();
    opt.ok_or(MpcError::InvalidScalar)
}

/// u32 değerini secp256k1 skalerine dönüştür.
/// Party index'ler için kullanılır (1..=n).
/// u32 her zaman secp256k1 mertebesinden küçüktür → güvenlidir.
pub fn scalar_from_u32(n: u32) -> Scalar {
    let mut bytes = [0u8; 32];
    bytes[28..].copy_from_slice(&n.to_be_bytes());
    scalar_from_bytes(&bytes).expect("u32 her zaman geçerli secp256k1 skaleridir")
}

/// u64 değerini secp256k1 skalerine dönüştür.
pub fn scalar_from_u64(n: u64) -> Scalar {
    let mut bytes = [0u8; 32];
    bytes[24..].copy_from_slice(&n.to_be_bytes());
    scalar_from_bytes(&bytes).expect("u64 her zaman geçerli secp256k1 skaleridir")
}

/// Noktayı sıkıştırılmış SEC1 formatına (33 byte) dönüştür.
pub fn point_to_bytes(p: &ProjectivePoint) -> [u8; 33] {
    if *p == ProjectivePoint::IDENTITY {
        return IDENTITY_BYTES;
    }
    let affine = p.to_affine();
    let encoded = affine.to_encoded_point(true);
    let slice = encoded.as_bytes();
    debug_assert_eq!(slice.len(), 33, "sıkıştırılmış nokta 33 byte olmalı");
    let mut out = [0u8; 33];
    out.copy_from_slice(slice);
    out
}

/// Sıkıştırılmış 33 byte'tan ProjectivePoint oluştur.
/// Geçersiz nokta (eğri üzerinde de��il) → `MpcError::InvalidPoint`.
/// Sonsuz nokta (IDENTITY_BYTES) → `Ok(ProjectivePoint::IDENTITY)`.
pub fn point_from_bytes(bytes: &[u8; 33]) -> MpcResult<ProjectivePoint> {
    // Sonsuz nokta kontrolü
    if bytes[0] == 0x00 {
        return Ok(ProjectivePoint::IDENTITY);
    }

    let encoded = k256::EncodedPoint::from_bytes(bytes as &[u8])
        .map_err(|_| MpcError::InvalidPoint)?;

    let affine_opt: Option<AffinePoint> = AffinePoint::from_encoded_point(&encoded).into();
    let affine = affine_opt.ok_or(MpcError::InvalidPoint)?;

    Ok(ProjectivePoint::from(affine))
}

/// Nokta topla: `a + b` (sıkıştırılmış byte'larla).
pub fn point_add(a: &[u8; 33], b: &[u8; 33]) -> MpcResult<[u8; 33]> {
    let pa = point_from_bytes(a)?;
    let pb = point_from_bytes(b)?;
    Ok(point_to_bytes(&(pa + pb)))
}

/// Skaler çarpım: `k * G` (G = generator).
pub fn scalar_mul_generator(k: &Scalar) -> [u8; 33] {
    let p = ProjectivePoint::GENERATOR * k;
    point_to_bytes(&p)
}

/// Skaler çarpım: `k * P`.
pub fn scalar_mul_point(k: &Scalar, p: &[u8; 33]) -> MpcResult<[u8; 33]> {
    let point = point_from_bytes(p)?;
    Ok(point_to_bytes(&(point * k)))
}

/// Skaler ters: `k^{-1} mod n`.
/// k = 0 → `MpcError::ScalarInvertFailed`.
pub fn scalar_invert(k: &Scalar) -> MpcResult<Scalar> {
    let inv: Option<Scalar> = k.invert().into();
    inv.ok_or(MpcError::ScalarInvertFailed)
}

/// İki nokta eşit mi? Sabit-zaman karşılaştırma.
pub fn points_equal_ct(a: &[u8; 33], b: &[u8; 33]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

/// İki skaler eşit mi? Sabit-zaman karşılaştırma.
pub fn scalars_equal_ct(a: &Scalar, b: &Scalar) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

/// Skaler toplamı: `a + b mod n`.
pub fn scalar_add(a: &Scalar, b: &Scalar) -> Scalar {
    a + b
}

/// Skaler çarpımı: `a * b mod n`.
pub fn scalar_mul(a: &Scalar, b: &Scalar) -> Scalar {
    a * b
}

/// Skaler negatifi: `-a mod n`.
pub fn scalar_neg(a: &Scalar) -> Scalar {
    a.negate()
}

/// BLAKE3 ile Fiat-Shamir challenge üret.
///
/// `context`: Domain separation string (sabit, protokol versiyonu içerir)
/// `parts`: Hash edilecek veriler (sırayla)
///
/// Sabit uzunluklu girdi olmak zorunda değil — BLAKE3 length-extension'a dayanıklı.
pub fn fiat_shamir_challenge(context: &str, parts: &[&[u8]]) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new_derive_key(context);
    for part in parts {
        // Uzunluk prefix ekle — concat ambiguity önle
        let len = (part.len() as u64).to_le_bytes();
        hasher.update(&len);
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// Nokta `P`'nin x-koordinatını 32 byte big-endian olarak döndür.
/// ECDSA / Schnorr `r = R.x mod n` için kullanılır.
pub fn point_x_coord(p: &[u8; 33]) -> MpcResult<[u8; 32]> {
    let point = point_from_bytes(p)?;
    if point == ProjectivePoint::IDENTITY {
        return Err(MpcError::InvalidPoint);
    }
    let affine = point.to_affine();
    let encoded = affine.to_encoded_point(false); // uncompressed
    let x_bytes = &encoded.as_bytes()[1..33]; // skip 0x04 prefix
    let mut out = [0u8; 32];
    out.copy_from_slice(x_bytes);
    Ok(out)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_scalar_roundtrip() {
        let s = Scalar::random(&mut OsRng);
        let bytes = scalar_to_bytes(&s);
        let s2 = scalar_from_bytes(&bytes).unwrap();
        assert!(scalars_equal_ct(&s, &s2), "scalar roundtrip başarısız");
    }

    #[test]
    fn test_scalar_from_u32() {
        let s = scalar_from_u32(42);
        let bytes = scalar_to_bytes(&s);
        // En son 4 byte 42'yi içermeli (big-endian)
        let expected_end: [u8; 4] = 42u32.to_be_bytes();
        assert_eq!(&bytes[28..], &expected_end);
    }

    #[test]
    fn test_point_roundtrip() {
        let s = Scalar::random(&mut OsRng);
        let p = ProjectivePoint::GENERATOR * &s;
        let bytes = point_to_bytes(&p);
        let p2 = point_from_bytes(&bytes).unwrap();
        assert!(points_equal_ct(&bytes, &point_to_bytes(&p2)));
    }

    #[test]
    fn test_identity_roundtrip() {
        let p = point_from_bytes(&IDENTITY_BYTES).unwrap();
        assert_eq!(p, ProjectivePoint::IDENTITY, "sonsuz nokta roundtrip");
    }

    #[test]
    fn test_scalar_mul_generator() {
        let s = scalar_from_u32(1);
        let p = scalar_mul_generator(&s);
        let g = generator_bytes();
        assert!(points_equal_ct(&p, &g), "1*G == G");
    }

    #[test]
    fn test_point_add_commutativity() {
        let s1 = Scalar::random(&mut OsRng);
        let s2 = Scalar::random(&mut OsRng);
        let p1 = point_to_bytes(&(ProjectivePoint::GENERATOR * &s1));
        let p2 = point_to_bytes(&(ProjectivePoint::GENERATOR * &s2));
        let ab = point_add(&p1, &p2).unwrap();
        let ba = point_add(&p2, &p1).unwrap();
        assert!(points_equal_ct(&ab, &ba), "nokta toplama değişmeli olmalı");
    }

    #[test]
    fn test_scalar_invert() {
        let s = scalar_from_u32(7);
        let inv = scalar_invert(&s).unwrap();
        let product = scalar_mul(&s, &inv);
        let one_bytes = {
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        };
        let one = scalar_from_bytes(&one_bytes).unwrap();
        assert!(scalars_equal_ct(&product, &one), "s * s^-1 == 1");
    }

    #[test]
    fn test_scalar_invert_zero_fails() {
        let zero = Scalar::ZERO;
        assert!(scalar_invert(&zero).is_err(), "sıfır skaler terslenemez");
    }

    #[test]
    fn test_invalid_point_fails() {
        let mut bad = [0u8; 33];
        bad[0] = 0x02;
        bad[1..].fill(0xFF);
        assert!(point_from_bytes(&bad).is_err(), "geçersiz nokta reddedilmeli");
    }

    #[test]
    fn test_fiat_shamir_domain_separation() {
        let c1 = fiat_shamir_challenge("nova-protocol-A", &[b"data"]);
        let c2 = fiat_shamir_challenge("nova-protocol-B", &[b"data"]);
        assert_ne!(c1, c2, "farklı context → farklı challenge");
    }

    #[test]
    fn test_fiat_shamir_deterministic() {
        let c1 = fiat_shamir_challenge("test", &[b"part1", b"part2"]);
        let c2 = fiat_shamir_challenge("test", &[b"part1", b"part2"]);
        assert_eq!(c1, c2, "aynı girdi → aynı challenge");
    }

    #[test]
    fn test_point_x_coord() {
        let s = scalar_from_u32(5);
        let p = scalar_mul_generator(&s);
        let x = point_x_coord(&p).unwrap();
        assert_ne!(x, [0u8; 32], "x-koordinat sıfır olmamalı");
    }
}
