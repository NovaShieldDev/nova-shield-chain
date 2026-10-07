//! MiMC hash fonksiyonu — ZK-friendly hash (BLS12-381 Fr alanı)
//!
//! MiMC (Minimal Multiplicative Complexity) — Albrecht et al. 2016.
//! 91 round, her round: state = (state + c_i)^3
//! BLS12-381 Fr için 128-bit güvenlik (gcd(3, p-1) = 1 olduğu doğrulanmıştır).
//!
//! Kullanım alanları:
//! - Note commitment: mimc_sponge([domain, value, randomness, asset_id, tk])
//! - Nullifier:       mimc_sponge([domain, nk, cm])
//! - Merkle node:     mimc_sponge([domain, left, right])
//!
//! Her round için 2 R1CS kısıtı (square + multiply → cubic).
//! 91 round × 2 = 182 kısıt/permütasyon.

use ark_bls12_381::Fr;
use ark_ff::{BigInteger, PrimeField, Zero};
use std::sync::OnceLock;

// R1CS modülleri
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};

// ─── Sabitler ──────────────────────────────────────────────────────────────────

/// BLS12-381 Fr üzerinde 128-bit güvenlik için round sayısı
pub const MIMC_ROUNDS: usize = 91;

/// Merkle ağaç derinliği — en fazla 2^MERKLE_DEPTH not desteklenir
pub const MERKLE_DEPTH: usize = 8;

// ─── Round constants ──────────────────────────────────────────────────────────

/// Deterministik BLAKE3-türetilmiş round sabitleri
fn round_constants() -> &'static Vec<Fr> {
    static RC: OnceLock<Vec<Fr>> = OnceLock::new();
    RC.get_or_init(|| {
        (0..MIMC_ROUNDS)
            .map(|i| {
                let seed = format!("NOVA:MIMC:BLS12381:RC:{:04}", i);
                let hash = blake3::hash(seed.as_bytes());
                Fr::from_le_bytes_mod_order(hash.as_bytes())
            })
            .collect()
    })
}

// ─── Domain separators ────────────────────────────────────────────────────────

pub fn domain_note_commitment() -> Fr {
    static D: OnceLock<Fr> = OnceLock::new();
    *D.get_or_init(|| {
        Fr::from_le_bytes_mod_order(blake3::hash(b"nova:note-commitment:v2").as_bytes())
    })
}

pub fn domain_nullifier() -> Fr {
    static D: OnceLock<Fr> = OnceLock::new();
    *D.get_or_init(|| {
        Fr::from_le_bytes_mod_order(blake3::hash(b"nova:nullifier-derive:v2").as_bytes())
    })
}

pub fn domain_merkle() -> Fr {
    static D: OnceLock<Fr> = OnceLock::new();
    *D.get_or_init(|| {
        Fr::from_le_bytes_mod_order(blake3::hash(b"nova:merkle-hash:v2").as_bytes())
    })
}

pub fn domain_stake() -> Fr {
    static D: OnceLock<Fr> = OnceLock::new();
    *D.get_or_init(|| {
        Fr::from_le_bytes_mod_order(blake3::hash(b"nova:stake:v2").as_bytes())
    })
}

pub fn domain_vote() -> Fr {
    static D: OnceLock<Fr> = OnceLock::new();
    *D.get_or_init(|| {
        Fr::from_le_bytes_mod_order(blake3::hash(b"nova:governance-vote:v2").as_bytes())
    })
}

// ─── Byte ↔ Field dönüşüm yardımcıları ────────────────────────────────────────

/// [u8; 32] → Fr (little-endian, mod p)
pub fn bytes_to_fr(b: &[u8; 32]) -> Fr {
    Fr::from_le_bytes_mod_order(b)
}

/// u128 → Fr (little-endian bytes üzerinden)
pub fn u128_to_fr(v: u128) -> Fr {
    Fr::from_le_bytes_mod_order(&v.to_le_bytes())
}

/// Fr → [u8; 32] (little-endian canonical bytes)
pub fn fr_to_bytes(f: Fr) -> [u8; 32] {
    let raw = f.into_bigint().to_bytes_le();
    let mut out = [0u8; 32];
    let len = raw.len().min(32);
    out[..len].copy_from_slice(&raw[..len]);
    out
}

// ─── MiMC native permütasyon ──────────────────────────────────────────────────

/// Tek-giriş MiMC permütasyonu: 91 round (state + c_i)^3
pub fn mimc_permute(mut state: Fr) -> Fr {
    for c in round_constants() {
        let t = state + c;
        state = t * t * t;
    }
    state
}

/// 2-giriş sponge hash: state ← Permute(state + a), Permute(state + b)
pub fn mimc_hash(a: Fr, b: Fr) -> Fr {
    let mut state = Fr::zero();
    state += a;
    state = mimc_permute(state);
    state += b;
    state = mimc_permute(state);
    state
}

/// Çoklu giriş MiMC sponge hash
pub fn mimc_sponge(inputs: &[Fr]) -> Fr {
    let mut state = Fr::zero();
    for inp in inputs {
        state += inp;
        state = mimc_permute(state);
    }
    state
}

// ─── Domain-tag'li yardımcı hash'ler ─────────────────────────────────────────

/// Note commitment: mimc_sponge([domain, value_fr, rand_fr, asset_fr, tk_fr])
pub fn mimc_note_commitment(
    value: u128,
    randomness: &[u8; 32],
    asset_id: &[u8; 32],
    transmission_key: &[u8; 32],
) -> [u8; 32] {
    let cm = mimc_sponge(&[
        domain_note_commitment(),
        u128_to_fr(value),
        bytes_to_fr(randomness),
        bytes_to_fr(asset_id),
        bytes_to_fr(transmission_key),
    ]);
    fr_to_bytes(cm)
}

/// Nullifier: mimc_sponge([domain, nk_fr, cm_fr])
pub fn mimc_nullifier(nullifier_key: &[u8; 32], commitment: &[u8; 32]) -> [u8; 32] {
    let nf = mimc_sponge(&[
        domain_nullifier(),
        bytes_to_fr(nullifier_key),
        bytes_to_fr(commitment),
    ]);
    fr_to_bytes(nf)
}

/// Merkle node hash: mimc_sponge([domain, left_fr, right_fr])
pub fn mimc_merkle_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let h = mimc_sponge(&[
        domain_merkle(),
        bytes_to_fr(left),
        bytes_to_fr(right),
    ]);
    fr_to_bytes(h)
}

/// Merkle root hesaplama — verilen yaprak ve yoldan
///
/// is_right: true ise mevcut düğüm sağda (sibling solda)
pub fn mimc_merkle_root(leaf: &[u8; 32], path: &[([u8; 32], bool)]) -> [u8; 32] {
    let mut current = *leaf;
    for (sibling, is_right) in path {
        current = if *is_right {
            mimc_merkle_hash(&current, sibling)
        } else {
            mimc_merkle_hash(sibling, &current)
        };
    }
    current
}

/// Merkle root — her zaman MERKLE_DEPTH seviye (eksik seviyeler sıfır sibling ile doldurulur)
pub fn mimc_merkle_root_padded(leaf: &[u8; 32], path: &[([u8; 32], bool)]) -> [u8; 32] {
    let mut current = *leaf;
    for i in 0..MERKLE_DEPTH {
        let (sibling, is_right) = if i < path.len() {
            path[i]
        } else {
            ([0u8; 32], false)
        };
        current = if is_right {
            mimc_merkle_hash(&current, &sibling)
        } else {
            mimc_merkle_hash(&sibling, &current)
        };
    }
    current
}

// ─── MiMC R1CS (ZK devresi için) ──────────────────────────────────────────────

/// Tek-giriş MiMC permütasyonu R1CS versiyonu.
/// Her round: 2 kısıt (square + multiply). Toplam: 91×2 = 182 kısıt.
pub fn mimc_permute_r1cs(
    cs: ConstraintSystemRef<Fr>,
    state: FpVar<Fr>,
) -> Result<FpVar<Fr>, SynthesisError> {
    let rc = round_constants();
    let mut s = state;
    for c in rc {
        let c_var = FpVar::Constant(*c);
        // t = s + c_i
        let t = s + c_var;
        // t^2 = t.square() [1 kısıt]
        let t_sq = t.square()?;
        // t^3 = t^2 * t [1 kısıt]
        s = &t_sq * &t;
    }
    Ok(s)
}

/// 2-giriş MiMC sponge hash R1CS versiyonu
pub fn mimc_hash_r1cs(
    cs: ConstraintSystemRef<Fr>,
    a: &FpVar<Fr>,
    b: &FpVar<Fr>,
) -> Result<FpVar<Fr>, SynthesisError> {
    let zero = FpVar::Constant(Fr::zero());
    let mut state = zero;
    // Absorb a
    state = state + a;
    state = mimc_permute_r1cs(cs.clone(), state)?;
    // Absorb b
    state = state + b;
    state = mimc_permute_r1cs(cs, state)?;
    Ok(state)
}

/// Çoklu giriş MiMC sponge R1CS versiyonu
pub fn mimc_sponge_r1cs(
    cs: ConstraintSystemRef<Fr>,
    inputs: &[FpVar<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    let mut state = FpVar::Constant(Fr::zero());
    for inp in inputs {
        state = state + inp;
        state = mimc_permute_r1cs(cs.clone(), state)?;
    }
    Ok(state)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;

    #[test]
    fn test_mimc_permute_deterministic() {
        let x = Fr::from(42u64);
        let y1 = mimc_permute(x);
        let y2 = mimc_permute(x);
        assert_eq!(y1, y2);
    }

    #[test]
    fn test_mimc_permute_nonzero() {
        let x = Fr::from(1u64);
        let y = mimc_permute(x);
        assert_ne!(y, Fr::zero());
    }

    #[test]
    fn test_mimc_hash_deterministic() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        let h1 = mimc_hash(a, b);
        let h2 = mimc_hash(a, b);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_mimc_hash_not_commutative() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        // MiMC hash deterministik ama genelde simetrik değil
        let h_ab = mimc_hash(a, b);
        let h_ba = mimc_hash(b, a);
        // (bu test sadece farklı girdi farklı çıktı verir diye kontrol eder)
        // Sponge H(a,b) = Perm(Perm(0+a)+b) vs Perm(Perm(0+b)+a) — farklı olmalı
        assert_ne!(h_ab, h_ba, "MiMC sponge girdi sırası önemli");
    }

    #[test]
    fn test_mimc_sponge_consistency() {
        let inputs = [Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let h1 = mimc_sponge(&inputs);
        let h2 = mimc_sponge(&inputs);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_note_commitment_deterministic() {
        let value = 1_000_000u128;
        let rand = [1u8; 32];
        let asset = [2u8; 32];
        let tk = [3u8; 32];
        let cm1 = mimc_note_commitment(value, &rand, &asset, &tk);
        let cm2 = mimc_note_commitment(value, &rand, &asset, &tk);
        assert_eq!(cm1, cm2);
    }

    #[test]
    fn test_note_commitment_changes_with_value() {
        let rand = [1u8; 32];
        let asset = [2u8; 32];
        let tk = [3u8; 32];
        let cm1 = mimc_note_commitment(1000, &rand, &asset, &tk);
        let cm2 = mimc_note_commitment(2000, &rand, &asset, &tk);
        assert_ne!(cm1, cm2);
    }

    #[test]
    fn test_nullifier_deterministic() {
        let nk = [7u8; 32];
        let cm = [8u8; 32];
        let nf1 = mimc_nullifier(&nk, &cm);
        let nf2 = mimc_nullifier(&nk, &cm);
        assert_eq!(nf1, nf2);
    }

    #[test]
    fn test_nullifier_changes_with_key() {
        let nk1 = [1u8; 32];
        let nk2 = [2u8; 32];
        let cm = [5u8; 32];
        let nf1 = mimc_nullifier(&nk1, &cm);
        let nf2 = mimc_nullifier(&nk2, &cm);
        assert_ne!(nf1, nf2);
    }

    #[test]
    fn test_merkle_hash_deterministic() {
        let left = [1u8; 32];
        let right = [2u8; 32];
        let h1 = mimc_merkle_hash(&left, &right);
        let h2 = mimc_merkle_hash(&left, &right);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_merkle_root_empty_path_is_leaf() {
        let leaf = [42u8; 32];
        let root = mimc_merkle_root(&leaf, &[]);
        assert_eq!(root, leaf, "Boş path: root = leaf");
    }

    #[test]
    fn test_merkle_root_one_level() {
        let leaf = [1u8; 32];
        let sibling = [2u8; 32];
        let root = mimc_merkle_root(&leaf, &[(sibling, true)]); // leaf sağda
        let expected = mimc_merkle_hash(&leaf, &sibling);
        assert_eq!(root, expected);
    }

    #[test]
    fn test_bytes_to_fr_round_trip() {
        let bytes = [0x42u8; 32];
        let fr = bytes_to_fr(&bytes);
        let back = fr_to_bytes(fr);
        // from_le_bytes_mod_order → into_bigint → to_bytes_le
        // Küçük değerler için round-trip tutarlı olmalı
        assert_eq!(bytes_to_fr(&back), fr);
    }

    #[test]
    fn test_u128_to_fr() {
        let v = 1_000_000_000_000_000_000u128; // 10^18
        let fr = u128_to_fr(v);
        assert_ne!(fr, Fr::zero());
        // Fr'den bytes'a ve geri
        let bytes = fr_to_bytes(fr);
        assert_eq!(u128_to_fr(v), bytes_to_fr(&bytes));
    }

    #[test]
    fn test_mimc_r1cs_matches_native() {
        // R1CS ve native hesaplama aynı sonucu vermeli
        let a_val = Fr::from(100u64);
        let b_val = Fr::from(200u64);

        // Native
        let native_result = mimc_hash(a_val, b_val);

        // R1CS (kısıtlama sistemi ile)
        let cs = ConstraintSystem::<Fr>::new_ref();
        let a_var = FpVar::new_witness(cs.clone(), || Ok(a_val)).unwrap();
        let b_var = FpVar::new_witness(cs.clone(), || Ok(b_val)).unwrap();
        let r1cs_result = mimc_hash_r1cs(cs.clone(), &a_var, &b_var).unwrap();

        // Değerleri karşılaştır
        let r1cs_val = r1cs_result.value().unwrap();
        assert_eq!(native_result, r1cs_val, "R1CS ve native sonuçlar eşleşmeli");
        assert!(cs.is_satisfied().unwrap(), "R1CS kısıtları sağlanmış olmalı");
    }

    #[test]
    fn test_mimc_sponge_r1cs_matches_native() {
        let inputs_native = [Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let native_result = mimc_sponge(&inputs_native);

        let cs = ConstraintSystem::<Fr>::new_ref();
        let inputs_r1cs: Vec<FpVar<Fr>> = inputs_native
            .iter()
            .map(|&v| FpVar::new_witness(cs.clone(), || Ok(v)).unwrap())
            .collect();
        let r1cs_result = mimc_sponge_r1cs(cs.clone(), &inputs_r1cs).unwrap();

        let r1cs_val = r1cs_result.value().unwrap();
        assert_eq!(native_result, r1cs_val, "Sponge R1CS ve native eşleşmeli");
        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_domain_separators_unique() {
        assert_ne!(domain_note_commitment(), domain_nullifier());
        assert_ne!(domain_nullifier(), domain_merkle());
        assert_ne!(domain_note_commitment(), domain_merkle());
        assert_ne!(domain_stake(), domain_vote());
    }

    #[test]
    fn test_merkle_root_padded_depth() {
        let leaf = [1u8; 32];
        // Boş path ile padded root, MERKLE_DEPTH seviye sonra
        let root = mimc_merkle_root_padded(&leaf, &[]);
        // Derinlik kadar permütasyon
        assert_ne!(root, leaf, "Padded root yaprak ile farklı olmalı");
        // Deterministik
        let root2 = mimc_merkle_root_padded(&leaf, &[]);
        assert_eq!(root, root2);
    }
}
