//! Gizlilik modları: Standard, FullShield, SemiShield (§5.7.5)

pub mod full_shield;
pub mod semi_shield;
pub mod standard;

pub use full_shield::FullShieldMode;
pub use semi_shield::SemiShieldMode;
pub use standard::StandardMode;

use serde::{Deserialize, Serialize};

/// Gizlilik modu seçeneği
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShieldMode {
    /// Standart: Mempool şifreli, zincir açık
    Standard,
    /// Tam gizlilik: Mempool + zincir şifreli
    FullShield,
    /// Yarı-gizli: Miktar gizli, adresler açık
    SemiShield,
}

impl ShieldMode {
    pub fn name(&self) -> &'static str {
        match self {
            ShieldMode::Standard => "standard",
            ShieldMode::FullShield => "full-shield",
            ShieldMode::SemiShield => "semi-shield",
        }
    }

    pub fn requires_zk_proof(&self) -> bool {
        matches!(self, ShieldMode::FullShield | ShieldMode::SemiShield)
    }

    pub fn hides_sender(&self) -> bool {
        matches!(self, ShieldMode::FullShield)
    }

    pub fn hides_amount(&self) -> bool {
        matches!(self, ShieldMode::FullShield | ShieldMode::SemiShield)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shield_mode_properties() {
        assert!(!ShieldMode::Standard.requires_zk_proof());
        assert!(ShieldMode::FullShield.requires_zk_proof());
        assert!(ShieldMode::SemiShield.requires_zk_proof());
    }

    #[test]
    fn test_full_shield_hides_everything() {
        assert!(ShieldMode::FullShield.hides_sender());
        assert!(ShieldMode::FullShield.hides_amount());
    }

    #[test]
    fn test_semi_shield_hides_amount_only() {
        assert!(!ShieldMode::SemiShield.hides_sender());
        assert!(ShieldMode::SemiShield.hides_amount());
    }

    #[test]
    fn test_standard_hides_nothing() {
        assert!(!ShieldMode::Standard.hides_sender());
        assert!(!ShieldMode::Standard.hides_amount());
    }
}
