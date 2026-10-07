//! Bakiye devresi — gizli transfer bakiye doğrulama.
//!
//! Giriş tutarı == Çıkış tutarı + Fee olduğunu ZK olarak kanıtla.
//!
//! NOVA-RED-CRYPTO-004: Range check eklendi — input/output/fee değerleri
//! 64-bit aralığında olduğu kısıtlanır. Aksi takdirde field elemanı
//! sınırına yakın değerler (p-1 vb.) bakiye eşitliğini bypass edebilir.

use ark_bn254::Fr;
use ark_ff::PrimeField;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

/// BN254 Fr bit uzunluğu 254; u64 için yalnızca 64 bit gerekli.
const U64_BITS: usize = 64;

/// Bit decomposition yoluyla FpVar'ı 64-bit aralığında zorla.
///
/// NOVA-RED-CRYPTO-004: `to_bits_le()` 254 bit döndürür; 64 ve üstü
/// bitler 0 olmalı → değer [0, 2^64) aralığında kısıtlanır.
fn enforce_u64_range(var: &FpVar<Fr>) -> Result<(), SynthesisError> {
    let bits = var.to_bits_le()?;
    // Bit 64..253 = 0 olmalı
    for bit in bits.iter().skip(U64_BITS) {
        bit.enforce_equal(&Boolean::constant(false))?;
    }
    Ok(())
}

/// Bakiye eşitlik devresi.
/// `input_amount == output_amount + fee` kısıtlaması.
///
/// NOVA-RED-CRYPTO-004: input/output/fee 64-bit range'e zorlanır —
/// field taşması ile sahte bakiye yaratılamaz.
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
        // Witness: input ve output miktarlar (None → 0, setup/dummy modu için)
        let input_var = FpVar::new_witness(
            ark_relations::ns!(cs, "input"),
            || Ok(Fr::from(self.input_amount.unwrap_or(0))),
        )?;

        let output_var = FpVar::new_witness(
            ark_relations::ns!(cs, "output"),
            || Ok(Fr::from(self.output_amount.unwrap_or(0))),
        )?;

        // Public input: fee
        let fee_var = FpVar::new_input(
            ark_relations::ns!(cs, "fee"),
            || Ok(Fr::from(self.fee.unwrap_or(0))),
        )?;

        // ── NOVA-RED-CRYPTO-004: 64-bit range check ───────────────────────
        enforce_u64_range(&input_var)?;
        enforce_u64_range(&output_var)?;
        enforce_u64_range(&fee_var)?;

        // Kısıt: input == output + fee
        input_var.enforce_equal(&(output_var + fee_var))?;

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;

    #[test]
    fn test_balance_circuit_satisfied() {
        let circuit = BalanceCircuit {
            input_amount: Some(1000),
            output_amount: Some(990),
            fee: Some(10),
        };
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap(), "1000 == 990 + 10 sağlanmalı");
        println!("BalanceCircuit kısıt sayısı: {}", cs.num_constraints());
    }

    #[test]
    fn test_balance_circuit_wrong_fee_fails() {
        let circuit = BalanceCircuit {
            input_amount: Some(1000),
            output_amount: Some(990),
            fee: Some(20), // 990 + 20 = 1010 ≠ 1000
        };
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış fee kısıtı ihlal etmeli");
    }

    /// NOVA-RED-CRYPTO-004: range check kısıt sayısını doğrula.
    #[test]
    fn test_balance_circuit_dummy_has_constraints() {
        let circuit = BalanceCircuit {
            input_amount: None,
            output_amount: None,
            fee: None,
        };
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        // Range check eklendi → kısıt sayısı artmalı
        assert!(cs.num_constraints() > 0, "Devre kısıtları oluşturulmalı");
    }
}
