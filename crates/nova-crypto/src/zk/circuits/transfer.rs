//! Transfer devresi — tam gizli UTXO transferi.
//!
//! §5.7 — ZK-Shield: hem gönderen hem alıcı hem miktar gizli.
//!
//! ## R1CS Kısıtları (BN254)
//!
//! 1. **Bakiye eşitliği**: `input_amount == output_amount + fee`
//! 2. **Input commitment**: `cm_in == MiMC10(input_amount, input_blinding)`
//! 3. **Nullifier türetme**: `nf == (sk + cm_in)^3`  (MiMC cubic)
//! 4. **Output commitment**: `cm_out == MiMC10(output_amount, output_blinding)`
//! 5. **Non-zero blinding**: `input_blinding * input_blinding_inv == 1`
//! 6. **Non-zero blinding**: `output_blinding * output_blinding_inv == 1`
//!
//! ## CRYPTO-004 Düzeltmesi
//!
//! Önceki uygulama: `cm = amount * blinding` (skaler alan çarpması).
//! Bu, commitment'ın HİDİNG özelliğini ihlal ediyor: `blinding` bilinirse
//! `amount = cm * blinding^{-1}` ile gizli miktar elde edilebilir.
//!
//! Yeni uygulama: `cm = MiMC10(amount, blinding)` — 10 turlu MiMC Feistel yapısı.
//! - **Hiding**: `blinding` bilinse bile `amount` trivially hesaplanamaz
//!   (MiMC tek yönlü fonksiyon olarak tasarlanmış)
//! - **Binding**: Aynı commitment'ı farklı `(amount', blinding')` ile açmak
//!   kriptografik olarak zor
//! - R1CS maliyeti: 10 tur × 2 çarpma = 20 kısıt (verimli)
//!
//! ## MiMC Tur Sabitleri
//! `BLAKE3("nova-transfer-commitment-round-{i}")` ile deterministik türetildi.
//! "Nothing up my sleeve" prensibi.

use ark_bn254::Fr;
use ark_ff::{Field, PrimeField};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

// ─── MiMC Tur Sabitleri ───────────────────────────────────────────────────────

/// MiMC turlarının sayısı — güvenlik ile verimlilik dengesi.
/// 10 tur, BN254 üzerinde yeterli karışım sağlar.
pub const MIMC_ROUNDS: usize = 10;

/// MiMC tur sabitleri — BLAKE3("nova-transfer-commitment-round-{i}") ile türetildi.
/// Little-endian bayt dizisi → `Fr::from_le_bytes_mod_order` ile alan elemanına çevrildi.
///
/// Bu sabitler her çalıştırmada yeniden hesaplanmak yerine compile-time'da
/// sabit olarak gömülüdür; `compute_mimc_round_constants()` ile doğrulanabilir.
const MIMC_CONSTANTS_BYTES: [[u8; 32]; MIMC_ROUNDS] = [
    // BLAKE3("nova-transfer-commitment-round-0")
    [0x3c,0x26,0xd6,0xd6,0x44,0x93,0x62,0xaf,0x3e,0x17,0x62,0x0a,0x12,0x15,0x60,0x00,
     0x2c,0x1b,0x2c,0x8d,0x8c,0x68,0x51,0x77,0x17,0x9d,0x05,0x1f,0xf1,0xbf,0xcc,0x77],
    // BLAKE3("nova-transfer-commitment-round-1")
    [0xa5,0xe8,0x69,0x9d,0xf4,0x5e,0x82,0x85,0xc3,0x3e,0xed,0xf4,0x6e,0x37,0x4f,0x1b,
     0x56,0x53,0x0e,0x9b,0x81,0xb7,0x46,0xf5,0x04,0x5c,0x17,0x53,0x01,0xc9,0x9f,0x03],
    // BLAKE3("nova-transfer-commitment-round-2")
    [0x71,0xc4,0x8b,0x02,0x8b,0x03,0x03,0x9a,0x14,0x90,0xbf,0x6a,0x2c,0xbf,0xa3,0xd3,
     0x27,0x93,0x55,0xa3,0xed,0x29,0xb1,0xc9,0x4e,0x29,0xd4,0x27,0x96,0x0b,0x1c,0xb9],
    // BLAKE3("nova-transfer-commitment-round-3")
    [0x2e,0x4a,0x70,0x3f,0x23,0x80,0x5c,0xf7,0x82,0xc4,0x25,0x6f,0x2b,0xc0,0x95,0x3a,
     0xf7,0x13,0xbd,0x55,0x5d,0x2c,0x21,0x17,0xb3,0x85,0xf5,0x8c,0xdf,0x59,0x92,0xeb],
    // BLAKE3("nova-transfer-commitment-round-4")
    [0x8d,0x06,0xb3,0x5b,0xde,0x06,0x6c,0x37,0x8f,0x2e,0x0c,0x97,0x81,0x59,0x5a,0x05,
     0x03,0x49,0xe9,0xce,0x29,0xbb,0xdc,0x30,0xf6,0x07,0x3e,0x79,0x77,0x25,0x04,0xa0],
    // BLAKE3("nova-transfer-commitment-round-5")
    [0xf2,0x60,0x93,0x1c,0xc4,0x85,0x7e,0xa3,0xb9,0xb0,0xad,0x33,0x1a,0x19,0x3d,0xe4,
     0x2a,0x9a,0x55,0x9c,0x22,0x20,0x63,0x74,0xe4,0x7b,0x40,0x18,0x8e,0xb6,0x73,0xde],
    // BLAKE3("nova-transfer-commitment-round-6")
    [0x1e,0x56,0xd5,0xbd,0xc3,0x73,0x4f,0x26,0x1e,0xe5,0x4e,0x19,0xb8,0x7b,0x9e,0xf6,
     0xe4,0x9b,0x2b,0x26,0xd8,0x52,0xce,0x6a,0x7f,0xc7,0x42,0xae,0x4e,0xb4,0x29,0x33],
    // BLAKE3("nova-transfer-commitment-round-7")
    [0x93,0x7a,0xa8,0x58,0x55,0x01,0x45,0x01,0xeb,0x39,0xb3,0xad,0xac,0x13,0x1a,0x13,
     0x6d,0x75,0x6e,0x4c,0x57,0x2f,0x53,0x9a,0x8f,0x56,0xf1,0x72,0x43,0x20,0x07,0x26],
    // BLAKE3("nova-transfer-commitment-round-8")
    [0x45,0x5c,0x48,0x4e,0x18,0x8c,0x69,0x5e,0x8f,0x43,0x8d,0xde,0xce,0x93,0x8c,0x50,
     0x0e,0x2e,0x7e,0x41,0x50,0x95,0x56,0x3b,0xa4,0x50,0xf3,0xc8,0x45,0x02,0xb6,0x32],
    // BLAKE3("nova-transfer-commitment-round-9")
    [0xd7,0xb8,0x2c,0xe1,0x8d,0xf8,0x4e,0x96,0x9e,0x86,0xfd,0xcc,0x32,0xce,0x52,0xe1,
     0x4e,0x73,0x74,0x22,0xf1,0x3e,0xa6,0x05,0x23,0x7c,0x17,0x6d,0xa1,0x4c,0x6f,0x41],
];

/// MiMC tur sabitleri `Fr` dizisi olarak döner.
pub fn mimc_round_constants() -> [Fr; MIMC_ROUNDS] {
    std::array::from_fn(|i| Fr::from_le_bytes_mod_order(&MIMC_CONSTANTS_BYTES[i]))
}

/// MiMC10 commitment (off-chain) — `cm = MiMC10(amount, blinding)`.
///
/// Her tur: `state = (state + key + c_i)^3`
/// Çıkış: `state + key` (Davies-Meyer strengthen)
///
/// Bu fonksiyon, devre içindeki `mimc10_commitment_gadget` ile tutarlıdır.
pub fn mimc10_commitment(amount: Fr, blinding: Fr) -> Fr {
    let constants = mimc_round_constants();
    let mut state = amount;
    for c in &constants {
        let t = state + blinding + c;
        state = t * t * t; // t^3
    }
    state + blinding
}

// ─── TransferCircuit ──────────────────────────────────────────────────────────

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

        // ── Kısıt 2: Input commitment — CRYPTO-004 düzeltmesi ─────────────────
        // cm_in == MiMC10(input_amount, input_blinding)
        // Önceki (güvensiz): cm_in == input_amount * input_blinding
        let computed_cm_in =
            mimc10_commitment_gadget(cs.clone(), &input_var, &in_blind_var)?;
        computed_cm_in.enforce_equal(&cm_in_var)?;

        // ── Kısıt 3: Nullifier türetme (MiMC cubic) ───────────────────────────
        // nf == (sk + cm_in)^3
        let sum = &sk_var + &cm_in_var;
        let tmp = &sum * &sum;
        let computed_nf = &tmp * &sum;
        computed_nf.enforce_equal(&nf_var)?;

        // ── Kısıt 4: Output commitment — CRYPTO-004 düzeltmesi ────────────────
        // cm_out == MiMC10(output_amount, output_blinding)
        // Önceki (güvensiz): cm_out == output_amount * output_blinding
        let computed_cm_out =
            mimc10_commitment_gadget(cs.clone(), &output_var, &out_blind_var)?;
        computed_cm_out.enforce_equal(&cm_out_var)?;

        // ── Kısıt 5-6: Non-zero blinding (NOVA-RED-CRYPTO-005) ───────────────
        // blinding * blinding_inv == 1 → blinding ≠ 0 garantisi.
        let one = FpVar::constant(Fr::from(1u64));

        let in_blind_inv_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input_blinding_inv"),
            || {
                let b = self.input_blinding
                    .map(|bytes| Fr::from_le_bytes_mod_order(&bytes))
                    .unwrap_or(Fr::from(1u64));
                Ok(b.inverse().unwrap_or(Fr::from(1u64)))
            },
        )?;
        (&in_blind_var * &in_blind_inv_var).enforce_equal(&one)?;

        let out_blind_inv_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output_blinding_inv"),
            || {
                let b = self.output_blinding
                    .map(|bytes| Fr::from_le_bytes_mod_order(&bytes))
                    .unwrap_or(Fr::from(1u64));
                Ok(b.inverse().unwrap_or(Fr::from(1u64)))
            },
        )?;
        (&out_blind_var * &out_blind_inv_var).enforce_equal(&one)?;

        Ok(())
    }
}

// ─── MiMC10 Gadget (R1CS içinde) ─────────────────────────────────────────────

/// MiMC10 commitment gadget — BN254 R1CS içinde çalışır.
///
/// Her tur 2 çarpma kullanır (t^2 + t^3), toplam 10 tur = 20 kısıt.
/// Tur fonksiyonu: `state = (state + key + c_i)^3`
fn mimc10_commitment_gadget(
    _cs: ConstraintSystemRef<Fr>,
    amount: &FpVar<Fr>,
    blinding: &FpVar<Fr>,
) -> Result<FpVar<Fr>, SynthesisError> {
    let constants = mimc_round_constants();
    let mut state = amount.clone();

    for c in &constants {
        let c_var = FpVar::constant(*c);
        // t = state + blinding + c_i
        let t = &state + blinding + &c_var;
        // state = t^3 = t * t * t (2 çarpma = 2 kısıt)
        let t2 = &t * &t;
        state = &t2 * &t;
    }

    // Davies-Meyer: output = state + blinding
    Ok(state + blinding)
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

    // CRYPTO-004 düzeltmesi: MiMC10 commitment
    let cm_in_fe = mimc10_commitment(in_fe, ib_fe);
    let cm_out_fe = mimc10_commitment(out_fe, ob_fe);
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

    /// CRYPTO-004: MiMC commitment — farklı (amount, blinding) çiftleri farklı cm üretir
    #[test]
    fn test_mimc_commitment_different_inputs() {
        let cm1 = mimc10_commitment(Fr::from(100u64), Fr::from_le_bytes_mod_order(&[1u8; 32]));
        let cm2 = mimc10_commitment(Fr::from(200u64), Fr::from_le_bytes_mod_order(&[1u8; 32]));
        let cm3 = mimc10_commitment(Fr::from(100u64), Fr::from_le_bytes_mod_order(&[2u8; 32]));
        assert_ne!(cm1, cm2, "Farklı miktar → farklı commitment");
        assert_ne!(cm1, cm3, "Farklı blinding → farklı commitment");
    }

    /// CRYPTO-004: MiMC commitment deterministik
    #[test]
    fn test_mimc_commitment_deterministic() {
        let amount = Fr::from(1000u64);
        let blinding = Fr::from_le_bytes_mod_order(&[7u8; 32]);
        let cm1 = mimc10_commitment(amount, blinding);
        let cm2 = mimc10_commitment(amount, blinding);
        assert_eq!(cm1, cm2);
    }

    /// CRYPTO-004: `amount=0` ile commitment sıfır DEĞİL (hiding özelliği)
    ///
    /// Önceki `amount * blinding = 0 * blinding = 0` açığı kapatıldı.
    /// MiMC(0, blinding) ≠ 0 çünkü MiMC tek yönlü karışım sağlar.
    #[test]
    fn test_mimc_zero_amount_not_zero_commitment() {
        let zero = Fr::from(0u64);
        let blinding = Fr::from_le_bytes_mod_order(&[3u8; 32]);
        let cm = mimc10_commitment(zero, blinding);
        // Commitment sıfır olmamalı (eski alan çarpmasında 0 * b = 0 idi)
        assert_ne!(cm, Fr::from(0u64), "MiMC(0, blinding) ≠ 0 — hiding özelliği");
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

    /// NOVA-RED-CRYPTO-005: Blinding=0 kısıtı ihlal eder (non-zero blinding check).
    #[test]
    fn test_transfer_zero_blinding_rejected() {
        use ark_relations::r1cs::ConstraintSystem;

        let zero_blind = [0u8; 32];
        let in_blind = [3u8; 32];

        // input_blinding=0 → kısıt ihlali (non-zero blinding check)
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
