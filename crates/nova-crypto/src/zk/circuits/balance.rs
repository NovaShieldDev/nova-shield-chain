//! Bakiye devresi — gizli transfer bakiye doğrulama.
//!
//! Giriş tutarı == Çıkış tutarı + Fee olduğunu ZK olarak kanıtla.

use ark_bn254::Fr;
use ark_ff::{PrimeField, Field};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

/// Bakiye eşitlik devresi.
/// `input_amount == output_amount + fee` kısıtlaması.
pub struct BalanceCircuit {
    /// Giriş miktarı (gizli witness).
    pub input_amount: Option<u64>,
    /// Çıkış miktarı (gizli witness).
    pub output_amount: Option<u64>,
    /// Ücret (public input).
    pub fee: Option<u64>,
}

impl ConstraintSynthesizer<Fr> for BalanceCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // Witness: input ve output miktarlar
        let input_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input"),
            || self.input_amount.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;

        let output_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output"),
            || self.output_amount.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;

        // Public input: fee
        let fee_var = FpVar::new_input(
            ark_relations::ns!(cs, "fee"),
            || self.fee.map(Fr::from).ok_or(SynthesisError::AssignmentMissing),
        )?;

        // Kısıt: input == output + fee
        input_var.enforce_equal(&(output_var + fee_var))?;

        Ok(())
    }
}
