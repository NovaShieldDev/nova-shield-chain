//! Guardian doğrulama — KYC/AML ve SLA onayı

use crate::errors::GuardianResult;
use crate::registry::categories::KycStatus;
use crate::registry::registration::{GuardianRegistry, GuardianStatus};
use nova_primitives::Address;
use serde::{Deserialize, Serialize};

// ─── SlaTerms ─────────────────────────────────────────────────────────────────

/// Hizmet Seviyesi Anlaşması — on-chain yazılı taahhütler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaTerms {
    /// Maksimum yanıt süresi (slot cinsinden): örn. 24h = 216_000 slot
    pub max_response_slots: u64,
    /// Minimum uptime yüzdesi (BPS): 9500 = %95
    pub min_uptime_bps: u32,
    /// SLA ihlali başına ceza (NSC)
    pub penalty_per_violation: u128,
    /// SLA versiyon/kimlik hash'i
    pub sla_hash: [u8; 32],
}

impl SlaTerms {
    pub fn new(
        max_response_slots: u64,
        min_uptime_bps: u32,
        penalty_per_violation: u128,
    ) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(&max_response_slots.to_le_bytes());
        h.update(&min_uptime_bps.to_le_bytes());
        h.update(&penalty_per_violation.to_le_bytes());
        Self {
            max_response_slots,
            min_uptime_bps,
            penalty_per_violation,
            sla_hash: *h.finalize().as_bytes(),
        }
    }

    /// Yanıt süresi SLA'yı ihlal ediyor mu?
    pub fn is_response_time_violated(&self, actual_response_slots: u64) -> bool {
        actual_response_slots > self.max_response_slots
    }
}

// ─── KycVerifier ──────────────────────────────────────────────────────────────

/// KYC doğrulama motoru (production'da gerçek KYC API ile entegre olur)
pub struct KycVerifier;

impl KycVerifier {
    /// Kurumsal guardian KYC'yi onayla
    pub fn approve(
        registry: &GuardianRegistry,
        guardian_id: &Address,
    ) -> GuardianResult<()> {
        let g = registry.get(guardian_id)?;
        if g.kyc_status == KycStatus::Verified {
            return Ok(()); // zaten onaylı
        }
        registry.update(guardian_id, |guard| {
            guard.kyc_status = KycStatus::Verified;
            guard.status = GuardianStatus::Active;
        })
    }

    /// KYC reddet
    pub fn reject(
        registry: &GuardianRegistry,
        guardian_id: &Address,
    ) -> GuardianResult<()> {
        registry.update(guardian_id, |guard| {
            guard.kyc_status = KycStatus::Rejected;
            guard.status = GuardianStatus::Deactivated;
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::categories::{GuardianCategory, MIN_STAKE_INSTITUTIONAL};
    use crate::registry::registration::Guardian;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn institutional_guardian(n: u8) -> Guardian {
        Guardian::new(addr(n), GuardianCategory::Institutional, MIN_STAKE_INSTITUTIONAL, 0)
            .unwrap()
    }

    #[test]
    fn test_kyc_approve() {
        let registry = GuardianRegistry::new();
        registry.register(institutional_guardian(1)).unwrap();

        KycVerifier::approve(&registry, &addr(1)).unwrap();
        let g = registry.get(&addr(1)).unwrap();
        assert_eq!(g.kyc_status, KycStatus::Verified);
        assert!(g.status.is_active());
    }

    #[test]
    fn test_kyc_reject() {
        let registry = GuardianRegistry::new();
        registry.register(institutional_guardian(2)).unwrap();
        KycVerifier::reject(&registry, &addr(2)).unwrap();
        let g = registry.get(&addr(2)).unwrap();
        assert_eq!(g.kyc_status, KycStatus::Rejected);
        assert!(matches!(g.status, GuardianStatus::Deactivated));
    }

    #[test]
    fn test_sla_terms() {
        let sla = SlaTerms::new(216_000, 9_500, 100 * 1_000_000_000_000_000_000);
        assert!(!sla.is_response_time_violated(100_000));
        assert!(sla.is_response_time_violated(300_000));
    }

    #[test]
    fn test_sla_hash_deterministic() {
        let s1 = SlaTerms::new(100, 9_000, 50);
        let s2 = SlaTerms::new(100, 9_000, 50);
        assert_eq!(s1.sla_hash, s2.sla_hash);
    }
}
