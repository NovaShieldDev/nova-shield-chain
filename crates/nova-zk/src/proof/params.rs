//! Trusted setup parametreleri — Groth16/PLONK için
//!
//! Groth16: Her circuit için bir kez yapılan trusted setup (proving key + verification key)
//! PLONK: Universal trusted setup — tüm circuit'ler için tek SRS
//!
//! Production: Gerçek MPC-based trusted setup
//! Bu implementasyon: BLAKE3 tabanlı mock (aynı API, farklı güvenlik modeli)

use serde::{Deserialize, Serialize};

// ─── CircuitType ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CircuitType {
    /// Transparent → Shielded
    Mint,
    /// Shielded → Shielded
    Transfer,
    /// Shielded → Transparent
    Burn,
    /// Shielded not sahipliği + nullifier + Merkle
    Spend,
    /// Shielded staking
    Stake,
    /// Shielded unstaking
    Unstake,
    /// Kimlik gizli governance oyu
    GovernanceVote,
    /// Bağımsız nullifier türetme kanıtı
    NullifierDerivation,
}

impl CircuitType {
    pub fn name(&self) -> &'static str {
        match self {
            CircuitType::Mint               => "mint",
            CircuitType::Transfer           => "transfer",
            CircuitType::Burn               => "burn",
            CircuitType::Spend              => "spend",
            CircuitType::Stake              => "stake",
            CircuitType::Unstake            => "unstake",
            CircuitType::GovernanceVote     => "governance_vote",
            CircuitType::NullifierDerivation => "nullifier_derivation",
        }
    }

    /// Tüm devre tipleri
    pub fn all() -> &'static [CircuitType] {
        &[
            CircuitType::Mint,
            CircuitType::Transfer,
            CircuitType::Burn,
            CircuitType::Spend,
            CircuitType::Stake,
            CircuitType::Unstake,
            CircuitType::GovernanceVote,
            CircuitType::NullifierDerivation,
        ]
    }
}

// ─── ProofSystemType ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofSystemType {
    /// Groth16: Hızlı doğrulama, küçük proof, circuit-specific setup
    Groth16,
    /// PLONK: Universal setup, daha büyük proof, esnek
    Plonk,
    /// Mock: Test için BLAKE3 tabanlı simülasyon
    Mock,
}

// ─── TrustedSetupParams ───────────────────────────────────────────────────────

/// Trusted setup parametreleri (production'da MPC ceremony output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedSetupParams {
    pub circuit_type: CircuitType,
    pub proof_system: ProofSystemType,
    /// Setup ceremony hash (güven kanıtı)
    pub setup_hash: [u8; 32],
}

impl TrustedSetupParams {
    /// Mock setup — test ve development için
    pub fn mock(circuit_type: CircuitType) -> Self {
        let setup_hash = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:trusted-setup-mock:v1");
            h.update(circuit_type.name().as_bytes());
            *h.finalize().as_bytes()
        };
        Self { circuit_type, proof_system: ProofSystemType::Mock, setup_hash }
    }

    /// Groth16 setup — production'da kullanılacak
    pub fn groth16(circuit_type: CircuitType) -> Self {
        let setup_hash = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:groth16-setup:v1!!!!!");
            h.update(circuit_type.name().as_bytes());
            *h.finalize().as_bytes()
        };
        Self { circuit_type, proof_system: ProofSystemType::Groth16, setup_hash }
    }

    pub fn setup_hash(&self) -> &[u8; 32] {
        &self.setup_hash
    }

    pub fn is_mock(&self) -> bool {
        self.proof_system == ProofSystemType::Mock
    }

    /// Production ortamı için güvenli mi?
    /// Mock setup production'da KULLANILMAMALI.
    pub fn is_production_ready(&self) -> bool {
        self.proof_system != ProofSystemType::Mock
    }

    /// Production ortamında mock setup kullanılmaya çalışılıyorsa hata döner.
    pub fn assert_not_mock(&self) -> crate::errors::ZkResult<()> {
        if self.is_mock() {
            Err(crate::errors::ZkError::MockSetupInProduction)
        } else {
            Ok(())
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_setup_deterministic() {
        let p1 = TrustedSetupParams::mock(CircuitType::Mint);
        let p2 = TrustedSetupParams::mock(CircuitType::Mint);
        assert_eq!(p1.setup_hash, p2.setup_hash);
    }

    #[test]
    fn test_different_circuits_different_setup() {
        let mint = TrustedSetupParams::mock(CircuitType::Mint);
        let transfer = TrustedSetupParams::mock(CircuitType::Transfer);
        let burn = TrustedSetupParams::mock(CircuitType::Burn);
        assert_ne!(mint.setup_hash, transfer.setup_hash);
        assert_ne!(transfer.setup_hash, burn.setup_hash);
    }

    #[test]
    fn test_new_circuit_types_have_names() {
        assert_eq!(CircuitType::Stake.name(), "stake");
        assert_eq!(CircuitType::Unstake.name(), "unstake");
        assert_eq!(CircuitType::GovernanceVote.name(), "governance_vote");
        assert_eq!(CircuitType::NullifierDerivation.name(), "nullifier_derivation");
    }

    #[test]
    fn test_all_circuits_listed() {
        assert_eq!(CircuitType::all().len(), 8);
    }

    #[test]
    fn test_is_mock() {
        let p = TrustedSetupParams::mock(CircuitType::Mint);
        assert!(p.is_mock());
        let p2 = TrustedSetupParams::groth16(CircuitType::Mint);
        assert!(!p2.is_mock());
    }

    #[test]
    fn test_mock_setup_not_production_ready() {
        let mock = TrustedSetupParams::mock(CircuitType::Mint);
        assert!(!mock.is_production_ready(), "Mock setup production için uygun olmamalı");
        assert!(mock.assert_not_mock().is_err(), "Mock setup production'da hata vermeli");
    }

    #[test]
    fn test_groth16_setup_production_ready() {
        let groth = TrustedSetupParams::groth16(CircuitType::Spend);
        assert!(groth.is_production_ready(), "Groth16 setup production için uygun");
        assert!(groth.assert_not_mock().is_ok(), "Groth16 setup mock değil");
    }

    #[test]
    fn test_assert_not_mock_returns_correct_error() {
        use crate::errors::ZkError;
        let mock = TrustedSetupParams::mock(CircuitType::Transfer);
        let err = mock.assert_not_mock().unwrap_err();
        assert!(matches!(err, ZkError::MockSetupInProduction));
    }
}
