//! Transfer devresi — tam gizli UTXO transferi.
//!
//! §5.7 — ZK-Shield: hem gönderen hem alıcı hem miktar gizli.
//!
//! ## R1CS Kısıtları (BN254)
//!
//! 1. **Bakiye eşitliği**: `input_amount == output_amount + fee`
//! 2. **Input commitment**: `cm_in_fe == input_amount_fe * input_blinding_fe`
//! 3. **Nullifier türetme**: `nf_fe == (sk_fe + cm_in_fe)^3`  (MiMC cubic)
//! 4. **Output commitment**: `cm_out_fe == output_amount_fe * output_blinding_fe`
//!
//! Her kısıt en fazla 2 çarpma kapısı kullanır → toplam ~8 kısıt.

use ark_bn254::Fr;
use ark_ff::{PrimeField, Field};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

/// Tam transfer devresi.
pub struct TransferCircuit {
    // ─── Witness (gizli) ────────────────────────────────────────────────────
    /// Harcanan UTXO miktarı.
    pub input_amount: Option<u64>,
    /// Gönderenin blinding factor'ü.
    pub input_blinding: Option<[u8; 32]>,
    /// Çıkış UTXO miktarı.
    pub output_amount: Option<u64>,
    /// Alıcının blinding factor'ü.
    pub output_blinding: Option<[u8; 32]>,
    /// Gönderenin gizli anahtarı (nullifier için).
    pub sender_secret_key: Option<[u8; 32]>,

    // ─── Public inputs (kamuya açık) ─────────────────────────────────────────
    /// Harcanan UTXO commitment (zincirde kayıtlı).
    pub input_commitment: Option<[u8; 32]>,
    /// Üretilen nullifier (çift harcamayı önler).
    pub nullifier: Option<[u8; 32]>,
    /// Çıkış UTXO commitment (yeni UTXO).
    pub output_commitment: Option<[u8; 32]>,
    /// İşlem ücreti.
    pub fee: Option<u64>,
}

impl ConstraintSynthesizer<Fr> for TransferCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Witness alan değişkenleri ─────────────────────────────────────────

        let input_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input_amount"),
            || self.input_amount.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;
        let output_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output_amount"),
            || self.output_amount.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;
        let in_blind_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input_blinding"),
            || {
                let b = self.input_blinding.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&b))
            },
        )?;
        let out_blind_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output_blinding"),
            || {
                let b = self.output_blinding.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&b))
            },
        )?;
        let sk_var = FpVar::new_witness(
            ark_relations::ns!(cs, "sender_sk"),
            || {
                let sk = self.sender_secret_key.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&sk))
            },
        )?;

        // ── Public inputs ─────────────────────────────────────────────────────

        let fee_var = FpVar::new_input(
            ark_relations::ns!(cs, "fee"),
            || self.fee.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;
        let cm_in_var = FpVar::new_input(
            ark_relations::ns!(cs, "input_commitment"),
            || {
                let b = self.input_commitment.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&b))
            },
        )?;
        let nf_var = FpVar::new_input(
            ark_relations::ns!(cs, "nullifier"),
            || {
                let b = self.nullifier.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&b))
            },
        )?;
        let cm_out_var = FpVar::new_input(
            ark_relations::ns!(cs, "output_commitment"),
            || {
                let b = self.output_commitment.unwrap_or([0u8; 32]);
                Ok(Fr::from_le_bytes_mod_order(&b))
            },
        )?;

        // ── Kısıt 1: Bakiye eşitliği ──────────────────────────────────────────
        // input == output + fee
        input_var.enforce_equal(&(output_var.clone() + fee_var))?;

        // ── Kısıt 2: Input commitment ─────────────────────────────────────────
        // cm_in_fe == input_amount_fe * input_blinding_fe
        let computed_cm_in = &input_var * &in_blind_var;
        computed_cm_in.enforce_equal(&cm_in_var)?;

        // ── Kısıt 3: Nullifier türetme (MiMC cubic) ───────────────────────────
        // nf_fe == (sk_fe + cm_in_fe)^3
        let sum = &sk_var + &cm_in_var;
        let tmp = &sum * &sum;
        let computed_nf = &tmp * &sum;
        computed_nf.enforce_equal(&nf_var)?;

        // ── Kısıt 4: Output commitment ────────────────────────────────────────
        // cm_out_fe == output_amount_fe * output_blinding_fe
        let computed_cm_out = &output_var * &out_blind_var;
        computed_cm_out.enforce_equal(&cm_out_var)?;

        // ── Kısıt 5-6 (NOVA-RED-CRYPTO-005): Non-zero blinding ───────────────
        // blinding=0 ise cm=0 — herhangi bir miktar sıfır commitment'ı satisfy eder.
        // Inverse trick: blinding * blinding_inv == 1 → blinding ≠ 0 garantisi.
        let one = FpVar::constant(Fr::from(1u64));

        let in_blind_inv_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input_blinding_inv"),
            || {
                let b = self.input_blinding
                    .map(|bytes| Fr::from_le_bytes_mod_order(&bytes))
                    .unwrap_or(Fr::from(1u64)); // setup dummy: 1 (terslenebilir)
                Ok(b.inverse().unwrap_or(Fr::from(1u64)))
            },
        )?;
        (&in_blind_var * &in_blind_inv_var).enforce_equal(&one)?;

        let out_blind_inv_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output_blinding_inv"),
            || {
                let b = self.output_blinding
                    .map(|bytes| Fr::from_le_bytes_mod_order(&bytes))
                    .unwrap_or(Fr::from(1u64)); // setup dummy: 1 (terslenebilir)
                Ok(b.inverse().unwrap_or(Fr::from(1u64)))
            },
        )?;
        (&out_blind_var * &out_blind_inv_var).enforce_equal(&one)?;

        Ok(())
    }
}

// ─── Off-chain yardımcılar ────────────────────────────────────────────────────

/// Transfer için public input'ları hesapla — Groth16 kanıtına girilebilir.
pub fn compute_transfer_public_inputs(
    input_amount: u64,
    input_blinding: &[u8; 32],
    output_amount: u64,
    output_blinding: &[u8; 32],
    sender_sk: &[u8; 32],
    fee: u64,
) -> TransferPublicInputs {
    let in_fe = Fr::from(input_amount);
    let ib_fe = Fr::from_le_bytes_mod_order(input_blinding.as_slice());
    let out_fe = Fr::from(output_amount);
    let ob_fe = Fr::from_le_bytes_mod_order(output_blinding.as_slice());
    let sk_fe = Fr::from_le_bytes_mod_order(sender_sk.as_slice());

    let cm_in_fe = in_fe * ib_fe;
    let cm_out_fe = out_fe * ob_fe;
    let sum = sk_fe + cm_in_fe;
    let nf_fe = sum * sum * sum;

    TransferPublicInputs {
        fee,
        input_commitment: fe_to_bytes(&cm_in_fe),
        nullifier: fe_to_bytes(&nf_fe),
        output_commitment: fe_to_bytes(&cm_out_fe),
    }
}

/// Transfer devresi public input'ları
#[derive(Debug, Clone)]
pub struct TransferPublicInputs {
    pub fee: u64,
    pub input_commitment: [u8; 32],
    pub nullifier: [u8; 32],
    pub output_commitment: [u8; 32],
}

/// Fr alan elemanını 32 byte little-endian'a dönüştür
fn fe_to_bytes(fe: &Fr) -> [u8; 32] {
    let big = fe.into_bigint();
    let bytes = ark_ff::BigInteger::to_bytes_le(&big);
    let mut out = [0u8; 32];
    let len = bytes.len().min(32);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

// ─── Testler ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zk::groth16::{groth16_setup, groth16_prove, prepare_verifying_key, groth16_verify};

    fn make_circuit_and_inputs(
        in_amt: u64,
        out_amt: u64,
        fee: u64,
    ) -> (TransferCircuit, Vec<Fr>) {
        let in_blind = [3u8; 32];
        let out_blind = [4u8; 32];
        let sk = [5u8; 32];

        let inputs = compute_transfer_public_inputs(in_amt, &in_blind, out_amt, &out_blind, &sk, fee);

        let circuit = TransferCircuit {
            input_amount: Some(in_amt),
            input_blinding: Some(in_blind),
            output_amount: Some(out_amt),
            output_blinding: Some(out_blind),
            sender_secret_key: Some(sk),
            input_commitment: Some(inputs.input_commitment),
            nullifier: Some(inputs.nullifier),
            output_commitment: Some(inputs.output_commitment),
            fee: Some(fee),
        };

        let pub_inputs = vec![
            Fr::from(fee),
            Fr::from_le_bytes_mod_order(&inputs.input_commitment),
            Fr::from_le_bytes_mod_order(&inputs.nullifier),
            Fr::from_le_bytes_mod_order(&inputs.output_commitment),
        ];

        (circuit, pub_inputs)
    }

    fn setup_dummy() -> (TransferCircuit, Vec<Fr>) {
        make_circuit_and_inputs(1000, 900, 100)
    }

    #[test]
    fn test_transfer_public_inputs_balance() {
        let result = compute_transfer_public_inputs(
            500, &[1u8; 32], 400, &[2u8; 32], &[3u8; 32], 100,
        );
        assert_eq!(result.fee, 100);
        assert_ne!(result.input_commitment, [0u8; 32]);
        assert_ne!(result.nullifier, [0u8; 32]);
        assert_ne!(result.output_commitment, [0u8; 32]);
    }

    #[test]
    fn test_transfer_nullifier_deterministic() {
        let p1 = compute_transfer_public_inputs(
            500, &[1u8; 32], 400, &[2u8; 32], &[3u8; 32], 100,
        );
        let p2 = compute_transfer_public_inputs(
            500, &[1u8; 32], 400, &[2u8; 32], &[3u8; 32], 100,
        );
        assert_eq!(p1.nullifier, p2.nullifier);
    }

    #[test]
    fn test_groth16_transfer_prove_verify() {
        let (setup_circuit, _) = setup_dummy();
        let (pk, vk) = groth16_setup(setup_circuit).unwrap();
        let pvk = prepare_verifying_key(&vk);

        let (prove_circuit, pub_inputs) = make_circuit_and_inputs(1000, 900, 100);
        let proof = groth16_prove(&pk, prove_circuit).unwrap();

        groth16_verify(&pvk, &proof, &pub_inputs).unwrap();
    }

    #[test]
    fn test_groth16_transfer_wrong_fee_fails() {
        let (setup_circuit, _) = setup_dummy();
        let (pk, vk) = groth16_setup(setup_circuit).unwrap();
        let pvk = prepare_verifying_key(&vk);

        let (prove_circuit, mut pub_inputs) = make_circuit_and_inputs(1000, 900, 100);
        let proof = groth16_prove(&pk, prove_circuit).unwrap();

        // Yanlış fee ile doğrulama → başarısız
        pub_inputs[0] = Fr::from(200u64);
        let result = groth16_verify(&pvk, &proof, &pub_inputs);
        assert!(result.is_err(), "Yanlış fee kabul edilmemeli");
    }

    /// NOVA-RED-CRYPTO-005: Blinding=0 kısıtı ihlal eder.
    #[test]
    fn test_transfer_zero_blinding_rejected() {
        use ark_relations::r1cs::ConstraintSystem;

        let zero_blind = [0u8; 32];
        let in_blind = [3u8; 32];

        // input_blinding=0 → kısıt ihlali
        let inputs_with_zero_in = compute_transfer_public_inputs(
            1000, &zero_blind, 900, &in_blind, &[5u8; 32], 100,
        );
        let circuit = TransferCircuit {
            input_amount: Some(1000),
            input_blinding: Some(zero_blind),
            output_amount: Some(900),
            output_blinding: Some(in_blind),
            sender_secret_key: Some([5u8; 32]),
            input_commitment: Some(inputs_with_zero_in.input_commitment),
            nullifier: Some(inputs_with_zero_in.nullifier),
            output_commitment: Some(inputs_with_zero_in.output_commitment),
            fee: Some(100),
        };
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "input_blinding=0 kısıt ihlal etmeli");
    }
}
