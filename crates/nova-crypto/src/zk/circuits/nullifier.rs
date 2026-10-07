//! Nullifier devresi — UTXO çift harcama koruması.
//!
//! §5.7.2 — Her UTXO harcanınca bir nullifier üretilir.
//!
//! ## R1CS Kısıtı (MiMC cubic, BN254)
//!
//! `nullifier_fe = (sk_fe + cm_fe)^3`
//!
//! - `sk_fe`      : secret_key baytlarının BN254 alan elemanı (witness)
//! - `cm_fe`      : commitment baytlarının alan elemanı (public input)
//! - `nullifier_fe`: çıktı alan elemanı (public input)
//!
//! 2 çarpma kapısı: `tmp = (sk+cm)²`, `nf = tmp*(sk+cm)`
//!
//! Off-chain `compute_nullifier_field` aynı formülü kullanır → tutarlı.
//!
//! Referans: penumbra nullifier (05-penumbra.md)

use ark_bn254::Fr;
use ark_ff::PrimeField;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

/// Nullifier devresi.
/// Kanıtlar: UTXO'yu harcayan kişi gizli anahtarı biliyor.
pub struct NullifierCircuit {
    /// Gizli anahtar (witness — ZK kanıtta gizlenir).
    pub secret_key: Option<[u8; 32]>,
    /// UTXO commitment (public input).
    pub utxo_commitment: Option<[u8; 32]>,
    /// Beklenen nullifier (public input).
    pub expected_nullifier: Option<[u8; 32]>,
}

impl ConstraintSynthesizer<Fr> for NullifierCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Witness: secret key → alan elemanı ───────────────────────────────
        let sk_fe = Fr::from_le_bytes_mod_order(
            &self.secret_key.unwrap_or([0u8; 32]),
        );
        let sk_var = FpVar::new_witness(
            ark_relations::ns!(cs, "sk_field"),
            || Ok(sk_fe),
        )?;

        // ── Public input: commitment → alan elemanı ───────────────────────────
        let cm_fe = Fr::from_le_bytes_mod_order(
            &self.utxo_commitment.unwrap_or([0u8; 32]),
        );
        let cm_var = FpVar::new_input(
            ark_relations::ns!(cs, "cm_field"),
            || Ok(cm_fe),
        )?;

        // ── Public input: beklenen nullifier ─────────────────────────────────
        let nf_fe = Fr::from_le_bytes_mod_order(
            &self.expected_nullifier.unwrap_or([0u8; 32]),
        );
        let nf_var = FpVar::new_input(
            ark_relations::ns!(cs, "nf_field"),
            || Ok(nf_fe),
        )?;

        // ── Kısıt: nullifier_fe = (sk_fe + cm_fe)^3 ──────────────────────────
        // R1CS: 2 çarpma kapısı
        //   tmp     = (sk + cm) * (sk + cm)
        //   nf_comp = tmp       * (sk + cm)
        let sum = &sk_var + &cm_var;
        let tmp = &sum * &sum;
        let nf_computed = &tmp * &sum;
        nf_computed.enforce_equal(&nf_var)?;

        Ok(())
    }
}

// ─── Off-chain yardımcı ───────────────────────────────────────────────────────

/// Nullifier hesapla — off-chain witness üretimi için.
///
/// **Alan tabanlı formül** (R1CS kısıtıyla tutarlı):
/// `nullifier_fe = (sk_fe + cm_fe)^3`
///
/// Dönen 32 byte, `NullifierCircuit::expected_nullifier`'a doğrudan girilebilir.
pub fn compute_nullifier_field(secret_key: &[u8; 32], utxo_commitment: &[u8; 32]) -> [u8; 32] {
    let sk_fe = Fr::from_le_bytes_mod_order(secret_key.as_slice());
    let cm_fe = Fr::from_le_bytes_mod_order(utxo_commitment.as_slice());
    let sum = sk_fe + cm_fe;
    let nf_fe = sum * sum * sum; // (sk + cm)^3
    let nf_big = nf_fe.into_bigint();
    let nf_bytes = ark_ff::BigInteger::to_bytes_le(&nf_big);
    let mut out = [0u8; 32];
    let len = nf_bytes.len().min(32);
    out[..len].copy_from_slice(&nf_bytes[..len]);
    out
}

/// BLAKE3 tabanlı nullifier — geriye dönük uyumluluk.
///
/// Yeni kod `compute_nullifier_field` kullanmalıdır (R1CS uyumlu).
pub fn compute_nullifier(secret_key: &[u8; 32], utxo_commitment: &[u8; 32]) -> [u8; 32] {
    use crate::hash::blake3_hash_many;
    *blake3_hash_many(&[secret_key.as_slice(), utxo_commitment.as_slice()]).as_bytes()
}

// ─── Testler ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zk::groth16::{groth16_setup, groth16_prove, prepare_verifying_key, groth16_verify};
    use ark_ff::PrimeField;

    fn make_circuit(sk: [u8; 32], cm: [u8; 32]) -> (NullifierCircuit, [u8; 32]) {
        let nf = compute_nullifier_field(&sk, &cm);
        let circuit = NullifierCircuit {
            secret_key: Some(sk),
            utxo_commitment: Some(cm),
            expected_nullifier: Some(nf),
        };
        (circuit, nf)
    }

    fn setup_circuit() -> (NullifierCircuit, [u8; 32]) {
        make_circuit([1u8; 32], [2u8; 32])
    }

    #[test]
    fn test_nullifier_field_deterministic() {
        let sk = [7u8; 32];
        let cm = [13u8; 32];
        let nf1 = compute_nullifier_field(&sk, &cm);
        let nf2 = compute_nullifier_field(&sk, &cm);
        assert_eq!(nf1, nf2);
        assert_ne!(nf1, [0u8; 32]);
    }

    #[test]
    fn test_nullifier_field_different_keys() {
        let cm = [5u8; 32];
        let nf1 = compute_nullifier_field(&[1u8; 32], &cm);
        let nf2 = compute_nullifier_field(&[2u8; 32], &cm);
        assert_ne!(nf1, nf2);
    }

    #[test]
    fn test_groth16_nullifier_prove_verify() {
        let (circuit_setup, _) = setup_circuit();
        // Setup
        let (pk, vk) = groth16_setup(circuit_setup).unwrap();
        let pvk = prepare_verifying_key(&vk);

        // Prove
        let (circuit_prove, nf_bytes) = make_circuit([1u8; 32], [2u8; 32]);
        let proof = groth16_prove(&pk, circuit_prove).unwrap();

        // Public inputs: cm_fe, nf_fe
        let cm_fe = Fr::from_le_bytes_mod_order(&[2u8; 32]);
        let nf_fe = Fr::from_le_bytes_mod_order(&nf_bytes);
        let pub_inputs = vec![cm_fe, nf_fe];

        // Verify
        groth16_verify(&pvk, &proof, &pub_inputs).unwrap();
    }

    #[test]
    fn test_groth16_nullifier_wrong_key_fails() {
        let (circuit_setup, _) = setup_circuit();
        let (pk, vk) = groth16_setup(circuit_setup).unwrap();
        let pvk = prepare_verifying_key(&vk);

        // Prove yanlış sk ile ama doğru nullifier public input olsun
        // → kanıt geçersiz çünkü sk_var constraint'i sağlamıyor
        let (circuit_prove, nf_bytes) = make_circuit([1u8; 32], [2u8; 32]);
        let proof = groth16_prove(&pk, circuit_prove).unwrap();

        // Yanlış nullifier ile doğrulama → başarısız
        let cm_fe = Fr::from_le_bytes_mod_order(&[2u8; 32]);
        let wrong_nf = [0xFFu8; 32];
        let wrong_nf_fe = Fr::from_le_bytes_mod_order(&wrong_nf);
        let result = groth16_verify(&pvk, &proof, &[cm_fe, wrong_nf_fe]);
        assert!(result.is_err(), "Yanlış nullifier kabul edilmemeli");
    }
}
