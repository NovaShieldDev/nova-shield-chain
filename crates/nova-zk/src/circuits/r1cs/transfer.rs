//! TransferCircuit R1CS — Shielded → Shielded (§5.7.3)
//!
//! Kanıt: "girdi notumu harcadım (nullifier), çıktı notunu oluşturdum (commitment),
//!         değer dengesi: input_value = output_value + fee"
//!
//! Public inputs:
//!   1. input_nullifier_fr
//!   2. output_commitment_fr
//!   3. value_balance_fr (girdi - çıktı = fee)
//!   4. input_commitment_fr — NOVA-RED-ZK-002: on-chain pool.has_commitment() için
//!
//! Private witness:
//!   input_note (value, rand, asset, tk), nullifier_key,
//!   output_note (value, rand, asset, tk)
//!
//! Kısıtlar:
//!   1. input_cm = mimc_sponge(domain_note, in_value, in_rand, in_asset, in_tk)
//!                                                  → input_cm == input_commitment_fr (NOVA-RED-ZK-002)
//!   2. nf = mimc_sponge(domain_nf, nk, input_cm)  → nf == input_nullifier_fr
//!   3. output_cm = mimc_sponge(domain_note, out_value, out_rand, out_asset, out_tk)
//!                                                  → output_cm == output_commitment_fr
//!   4. in_value == out_value + value_balance (değer dengesi)
//!   5. in_asset_id == out_asset_id (cross-asset sahteciliği önlemi — NOVA-RED-ZK-001)

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_note_commitment, domain_nullifier, mimc_sponge_r1cs, u128_to_fr,
};

// ─── TransferCircuit ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TransferCircuit {
    // Public inputs
    pub input_nullifier:    Option<[u8; 32]>,
    pub output_commitment:  Option<[u8; 32]>,
    pub value_balance:      Option<u128>,   // fee (girdi - çıktı)
    /// NOVA-RED-ZK-002: input commitment'ı public yaparak on-chain pool.has_commitment() kontrolüne izin verir.
    /// Commitment zaten blockchain'de mint sırasında kayıtlı → privacy kırılmaz.
    pub input_commitment:   Option<[u8; 32]>,
    // Private witness — girdi notu
    pub in_value:           Option<u128>,
    pub in_randomness:      Option<[u8; 32]>,
    pub in_asset_id:        Option<[u8; 32]>,
    pub in_transmission_key:Option<[u8; 32]>,
    pub nullifier_key:      Option<[u8; 32]>,
    // Private witness — çıktı notu
    pub out_value:          Option<u128>,
    pub out_randomness:     Option<[u8; 32]>,
    pub out_asset_id:       Option<[u8; 32]>,
    pub out_transmission_key:Option<[u8; 32]>,
}

impl TransferCircuit {
    pub fn new(
        in_value: u128,
        in_randomness: [u8; 32],
        in_asset_id: [u8; 32],
        in_transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        out_value: u128,
        out_randomness: [u8; 32],
        out_asset_id: [u8; 32],
        out_transmission_key: [u8; 32],
        input_nullifier: [u8; 32],
        output_commitment: [u8; 32],
        value_balance: u128,
    ) -> Self {
        // input_commitment = mimc_note_commitment(in_value, in_rand, in_asset, in_tk)
        // Circuit içinde de hesaplanır ve public input ile enforce_equal yapılır.
        use crate::circuits::mimc::mimc_note_commitment;
        let input_commitment = mimc_note_commitment(in_value, &in_randomness, &in_asset_id, &in_transmission_key);
        Self {
            input_nullifier: Some(input_nullifier),
            output_commitment: Some(output_commitment),
            value_balance: Some(value_balance),
            input_commitment: Some(input_commitment),
            in_value: Some(in_value),
            in_randomness: Some(in_randomness),
            in_asset_id: Some(in_asset_id),
            in_transmission_key: Some(in_transmission_key),
            nullifier_key: Some(nullifier_key),
            out_value: Some(out_value),
            out_randomness: Some(out_randomness),
            out_asset_id: Some(out_asset_id),
            out_transmission_key: Some(out_transmission_key),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            input_nullifier: None,
            output_commitment: None,
            value_balance: None,
            input_commitment: None,
            in_value: None,
            in_randomness: None,
            in_asset_id: None,
            in_transmission_key: None,
            nullifier_key: None,
            out_value: None,
            out_randomness: None,
            out_asset_id: None,
            out_transmission_key: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for TransferCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private witness — girdi notu ───────────────────────────────────────
        let in_val_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.in_value.unwrap_or(0)))
        })?;
        let in_rand_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.in_randomness.unwrap_or([0u8; 32])))
        })?;
        let in_asset_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.in_asset_id.unwrap_or([0u8; 32])))
        })?;
        // NOVA-RED-ZK-001 fix: Asset ID eşitliği için kopyasını sakla
        let in_asset_for_eq = in_asset_var.clone();
        let in_tk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.in_transmission_key.unwrap_or([0u8; 32])))
        })?;
        let nk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier_key.unwrap_or([0u8; 32])))
        })?;

        // ── Private witness — çıktı notu ───────────────────────────────────────
        let out_val_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.out_value.unwrap_or(0)))
        })?;
        let out_rand_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.out_randomness.unwrap_or([0u8; 32])))
        })?;
        let out_asset_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.out_asset_id.unwrap_or([0u8; 32])))
        })?;
        // NOVA-RED-ZK-001 fix: Asset ID eşitliği için kopyasını sakla
        let out_asset_for_eq = out_asset_var.clone();
        let out_tk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.out_transmission_key.unwrap_or([0u8; 32])))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.input_nullifier.unwrap_or([0u8; 32])))
        })?;
        let out_cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.output_commitment.unwrap_or([0u8; 32])))
        })?;
        let balance_pub = FpVar::new_input(cs.clone(), || {
            Ok(u128_to_fr(self.value_balance.unwrap_or(0)))
        })?;
        // NOVA-RED-ZK-002: input_commitment public input — on-chain pool.has_commitment() için
        let in_cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.input_commitment.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt 1: girdi commitment ──────────────────────────────────────────
        // NOVA-RED-ZK-002: in_cm hem nullifier için kullanılır hem de public input ile
        // enforce_equal yapılır → prover gerçek commitment'ı kanıtlamak zorundadır.
        let domain_note = FpVar::Constant(domain_note_commitment());
        let in_cm = mimc_sponge_r1cs(
            cs.clone(),
            &[
                domain_note.clone(),
                in_val_var.clone(),
                in_rand_var,
                in_asset_var,
                in_tk_var,
            ],
        )?;
        // Computed commitment == public input (on-chain pool.has_commitment() için)
        in_cm.enforce_equal(&in_cm_pub)?;

        // ── Kısıt 2: nullifier türetme ────────────────────────────────────────
        let domain_nf = FpVar::Constant(domain_nullifier());
        let nf_computed = mimc_sponge_r1cs(cs.clone(), &[domain_nf, nk_var, in_cm_pub.clone()])?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: çıktı commitment ─────────────────────────────────────────
        let out_cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, out_val_var.clone(), out_rand_var, out_asset_var, out_tk_var],
        )?;
        out_cm.enforce_equal(&out_cm_pub)?;

        // ── Kısıt 4: değer dengesi (in_value = out_value + balance) ──────────
        // in_val_var == out_val_var + balance_pub
        let sum = &out_val_var + &balance_pub;
        in_val_var.enforce_equal(&sum)?;

        // ── Kısıt 5: Asset ID eşitliği (NOVA-RED-ZK-001) ─────────────────────
        // in_asset_id == out_asset_id zorunluluğu.
        // Bu kısıt olmadan: A asset'i harcayıp B asset'i üretmek mümkün →
        // cross-asset token sahteciliği. R1CS kanıt sisteminde bu matematiksel
        // olarak imkansız hale getirilmelidir.
        in_asset_for_eq.enforce_equal(&out_asset_for_eq)?;

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::{mimc_note_commitment, mimc_nullifier};

    fn nsc_asset() -> [u8; 32] {
        let mut a = [0u8; 32];
        a[0] = 0x4E; a[1] = 0x53; a[2] = 0x43; a[3] = 0x01;
        a
    }

    fn build_transfer(in_value: u128, out_value: u128) -> TransferCircuit {
        let asset = nsc_asset();
        let in_rand = [0x11u8; 32];
        let in_tk   = [0x22u8; 32];
        let nk      = [0x33u8; 32];
        let out_rand = [0x44u8; 32];
        let out_tk   = [0x55u8; 32];
        let fee = in_value - out_value;

        let in_cm = mimc_note_commitment(in_value, &in_rand, &asset, &in_tk);
        let nf = mimc_nullifier(&nk, &in_cm);
        let out_cm = mimc_note_commitment(out_value, &out_rand, &asset, &out_tk);

        TransferCircuit::new(
            in_value, in_rand, asset, in_tk,
            nk,
            out_value, out_rand, asset, out_tk,
            nf, out_cm, fee,
        )
    }

    #[test]
    fn test_transfer_circuit_satisfied() {
        let circuit = build_transfer(1_000, 800); // 200 fee
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
        println!("TransferCircuit constraint: {}", cs.num_constraints());
    }

    #[test]
    fn test_transfer_zero_fee_satisfied() {
        let circuit = build_transfer(500, 500); // sıfır fee
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_transfer_wrong_balance_fails() {
        let asset = nsc_asset();
        let in_rand = [0x11u8; 32];
        let in_tk   = [0x22u8; 32];
        let nk      = [0x33u8; 32];
        let out_rand = [0x44u8; 32];
        let out_tk   = [0x55u8; 32];

        let in_cm = mimc_note_commitment(1000, &in_rand, &asset, &in_tk);
        let nf = mimc_nullifier(&nk, &in_cm);
        let out_cm = mimc_note_commitment(800, &out_rand, &asset, &out_tk);
        let wrong_balance = 100u128; // gerçekte 200 olmalı

        let circuit = TransferCircuit::new(
            1000, in_rand, asset, in_tk,
            nk,
            800, out_rand, asset, out_tk,
            nf, out_cm, wrong_balance,
        );
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış denge kısıtı bozmalı");
    }

    // ── NOVA-RED-ZK-001 regression testleri ──────────────────────────────────

    #[test]
    fn test_cross_asset_transfer_rejected() {
        // NOVA-RED-ZK-001: Farklı asset_id ile transfer REDDEDILMELI
        let nsc_asset = nsc_asset();
        let other_asset = {
            let mut a = [0u8; 32];
            a[0] = 0xFF; // Farklı asset
            a
        };

        let in_rand = [0x11u8; 32];
        let in_tk   = [0x22u8; 32];
        let nk      = [0x33u8; 32];
        let out_rand = [0x44u8; 32];
        let out_tk   = [0x55u8; 32];

        let in_cm = mimc_note_commitment(1000, &in_rand, &nsc_asset, &in_tk);
        let nf = mimc_nullifier(&nk, &in_cm);
        // Çıktı: farklı asset
        let out_cm = mimc_note_commitment(1000, &out_rand, &other_asset, &out_tk);

        let circuit = TransferCircuit::new(
            1000, in_rand, nsc_asset, in_tk,
            nk,
            1000, out_rand, other_asset, out_tk, // ← farklı asset
            nf, out_cm, 0,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "Farklı asset_id ile transfer R1CS kısıtını kırmalı (cross-asset sahteciliği önlenir)"
        );
    }

    #[test]
    fn test_same_asset_transfer_still_works() {
        // Aynı asset_id — normal transfer çalışmalı
        let circuit = build_transfer(1000, 800);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap(), "Aynı asset ile transfer hala çalışmalı");
    }

    #[test]
    fn test_transfer_setup_dummy() {
        let circuit = TransferCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
