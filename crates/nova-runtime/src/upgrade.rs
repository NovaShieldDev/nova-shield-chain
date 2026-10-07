//! Forkless Runtime Upgrade — Polkadot set_code pattern'inden ilham
//!
//! Polkadot'ta runtime upgrade akışı:
//!   1. Governance: set_code(new_wasm) extrinsic onaylanır
//!   2. Bir sonraki bloktan itibaren yeni runtime devrede
//!
//! Nova Shield uyarlaması:
//!   1. Governance önerisi: UpgradeRuntime { new_version }
//!   2. Öneri geçti → ScheduledUpgrade kaydedilir
//!   3. activation_slot'ta → upgrade uygulanır

use crate::errors::{RuntimeError, RuntimeResult};
use nova_crypto::hash::blake3_hash;
use nova_primitives::{Address, crypto::Hash};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── Runtime Versiyon ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl RuntimeVersion {
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    pub fn as_u32(&self) -> u32 {
        (self.major as u32) << 16 | (self.minor as u32) << 8 | (self.patch as u32)
    }

    pub fn from_u32(v: u32) -> Self {
        Self {
            major: ((v >> 16) & 0xFF) as u16,
            minor: ((v >> 8) & 0xFF) as u16,
            patch: (v & 0xFF) as u16,
        }
    }
}

impl std::fmt::Display for RuntimeVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ─── Zamanlanmış Upgrade ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledUpgrade {
    pub from_version: RuntimeVersion,
    pub to_version: RuntimeVersion,
    pub activation_slot: u64,
    pub approved_by_proposal: u64,
    pub scheduled_at_slot: u64,
}

// ─── CodeUpgradeAuthorization (İki Fazlı) ─────────────────────────────────────

/// İki fazlı upgrade yetkilendirme — Polkadot `authorize_upgrade` pattern.
///
/// Faz 1: `authorize_upgrade(code_hash)` — hash governance tarafından onaylanır
/// Faz 2: `apply_authorized_upgrade(code)` — gerçek kod yüklenir, hash doğrulanır
///
/// Bu pattern şunları sağlar:
/// - Hash onayı ve kod yüklemesi ayrı zamanlarda yapılabilir
/// - Kod supply chain saldırılarına karşı hash binding
/// - Downgrade koruması (version check)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeUpgradeAuthorization {
    /// Onaylanan kodun BLAKE3 hash'i
    pub code_hash: Hash,
    /// Yükseltilecek hedef versiyon
    pub to_version: RuntimeVersion,
    /// Hash onayını gerçekleştiren governance proposal ID
    pub proposal_id: u64,
    /// Yetkilendirme slot'u
    pub authorized_at_slot: u64,
}

// ─── UpgradeManager ───────────────────────────────────────────────────────────

/// Governance modülünün authority adresi (yalnızca bu adres upgrade başlatabilir)
/// 0x47 = 'G' (Governance)
pub const GOVERNANCE_MODULE_ADDRESS: [u8; 20] = [0x47u8; 20];

/// Upgrade için minimum bekleme süresi: 1 epoch = 24h = 216_000 slot
pub const MIN_UPGRADE_DELAY_SLOTS: u64 = 216_000;

#[derive(Debug)]
pub struct UpgradeManager {
    current_version: Arc<RwLock<RuntimeVersion>>,
    pending_upgrade: Arc<RwLock<Option<ScheduledUpgrade>>>,
    applied_upgrades: Arc<RwLock<Vec<ScheduledUpgrade>>>,
    /// İki fazlı upgrade için yetkilendirilmiş hash
    authorized_upgrade: Arc<RwLock<Option<CodeUpgradeAuthorization>>>,
}

impl Default for UpgradeManager {
    fn default() -> Self {
        Self::new(RuntimeVersion::new(1, 0, 0))
    }
}

impl UpgradeManager {
    pub fn new(initial_version: RuntimeVersion) -> Self {
        Self {
            current_version: Arc::new(RwLock::new(initial_version)),
            pending_upgrade: Arc::new(RwLock::new(None)),
            applied_upgrades: Arc::new(RwLock::new(Vec::new())),
            authorized_upgrade: Arc::new(RwLock::new(None)),
        }
    }

    pub fn current_version(&self) -> RuntimeVersion {
        self.current_version.read().clone()
    }

    /// Governance onayından sonra upgrade zamanla
    pub fn schedule_upgrade(
        &self,
        to_version: RuntimeVersion,
        activation_slot: u64,
        proposal_id: u64,
        current_slot: u64,
        caller: Address,
    ) -> RuntimeResult<()> {
        // Yalnızca governance modülü upgrade başlatabilir
        if *caller.as_bytes() != GOVERNANCE_MODULE_ADDRESS {
            return Err(RuntimeError::UpgradeUnauthorized);
        }

        // Minimum bekleme süresi
        if activation_slot < current_slot + MIN_UPGRADE_DELAY_SLOTS {
            return Err(RuntimeError::UpgradeNotReady {
                remaining_slots: (current_slot + MIN_UPGRADE_DELAY_SLOTS).saturating_sub(activation_slot),
            });
        }

        let current = self.current_version.read().clone();

        // Yeni versiyon mevcut versiyondan büyük olmalı
        if to_version <= current {
            return Err(RuntimeError::InvalidRuntimeVersion);
        }

        let upgrade = ScheduledUpgrade {
            from_version: current,
            to_version,
            activation_slot,
            approved_by_proposal: proposal_id,
            scheduled_at_slot: current_slot,
        };

        *self.pending_upgrade.write() = Some(upgrade);
        Ok(())
    }

    /// Her slot'ta kontrol et — aktivasyon zamanı geldiyse uygula
    pub fn try_apply(&self, current_slot: u64) -> Option<RuntimeVersion> {
        let should_apply = {
            let pending = self.pending_upgrade.read();
            pending
                .as_ref()
                .map(|u| current_slot >= u.activation_slot)
                .unwrap_or(false)
        };

        if should_apply {
            let upgrade = self.pending_upgrade.write().take()?;
            let new_version = upgrade.to_version.clone();
            *self.current_version.write() = new_version.clone();
            self.applied_upgrades.write().push(upgrade);
            Some(new_version)
        } else {
            None
        }
    }

    pub fn pending_upgrade(&self) -> Option<ScheduledUpgrade> {
        self.pending_upgrade.read().clone()
    }

    pub fn applied_count(&self) -> usize {
        self.applied_upgrades.read().len()
    }

    pub fn cancel_pending(&self) {
        *self.pending_upgrade.write() = None;
    }

    // ─── İki Fazlı Upgrade (Polkadot CodeUpgradeAuthorization) ────────────────

    /// FAZ 1: Kod hash'ini yetkilendir.
    ///
    /// Governance öneri onayından sonra çağrılır.
    /// Gerçek kodu yüklemeden önce hash'i teyit eder.
    ///
    /// - `code_hash`: BLAKE3 hash of the new runtime code
    /// - `to_version`: hedef versiyon (downgrade koruması)
    /// - Caller: governance modülü olmak zorunda
    pub fn authorize_upgrade(
        &self,
        code_hash: Hash,
        to_version: RuntimeVersion,
        proposal_id: u64,
        current_slot: u64,
        caller: Address,
    ) -> RuntimeResult<()> {
        if *caller.as_bytes() != GOVERNANCE_MODULE_ADDRESS {
            return Err(RuntimeError::UpgradeUnauthorized);
        }

        let current = self.current_version.read().clone();
        if to_version <= current {
            return Err(RuntimeError::InvalidRuntimeVersion);
        }

        let auth = CodeUpgradeAuthorization {
            code_hash,
            to_version,
            proposal_id,
            authorized_at_slot: current_slot,
        };
        *self.authorized_upgrade.write() = Some(auth);
        Ok(())
    }

    /// FAZ 2: Yetkilendirilmiş kodu uygula.
    ///
    /// - `code`: gerçek runtime binary (hash'i authorize_upgrade'de onaylananla eşleşmeli)
    /// - Hash uyuşmazlığı → `UpgradeHashMismatch` → supply chain saldırısı önleme
    /// - Başarılı → versiyonu güncelle, yetkilendirmeyi temizle
    pub fn apply_authorized_upgrade(
        &self,
        code: &[u8],
        current_slot: u64,
    ) -> RuntimeResult<RuntimeVersion> {
        let auth = {
            let lock = self.authorized_upgrade.read();
            lock.clone().ok_or(RuntimeError::UpgradeNotAuthorized)?
        };

        // Hash doğrulama: BLAKE3(code) == auth.code_hash
        let actual_hash = blake3_hash(code);
        if actual_hash != auth.code_hash {
            let expected = hex_str(auth.code_hash.as_bytes());
            let got = hex_str(actual_hash.as_bytes());
            return Err(RuntimeError::UpgradeHashMismatch { expected, got });
        }

        // Version güncelle
        let new_version = auth.to_version.clone();
        let old_version = self.current_version.read().clone();
        *self.current_version.write() = new_version.clone();

        // Yetkilendirmeyi temizle
        *self.authorized_upgrade.write() = None;

        // Geçmişe kaydet
        let upgrade = ScheduledUpgrade {
            from_version: old_version,
            to_version: new_version.clone(),
            activation_slot: current_slot,
            approved_by_proposal: auth.proposal_id,
            scheduled_at_slot: auth.authorized_at_slot,
        };
        self.applied_upgrades.write().push(upgrade);

        Ok(new_version)
    }

    /// Bekleyen iki fazlı yetkilendirmeyi kontrol et
    pub fn pending_authorization(&self) -> Option<CodeUpgradeAuthorization> {
        self.authorized_upgrade.read().clone()
    }
}

/// Küçük hex yardımcısı (ilk 8 byte)
fn hex_str(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gov_addr() -> Address {
        Address::from_bytes(GOVERNANCE_MODULE_ADDRESS)
    }

    fn random_addr() -> Address {
        Address::from_bytes([0xAAu8; 20])
    }

    #[test]
    fn test_initial_version() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 0, 0));
    }

    #[test]
    fn test_schedule_upgrade_success() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        mgr.schedule_upgrade(
            RuntimeVersion::new(1, 1, 0),
            MIN_UPGRADE_DELAY_SLOTS + 100,
            1,
            0,
            gov_addr(),
        )
        .unwrap();
        assert!(mgr.pending_upgrade().is_some());
    }

    #[test]
    fn test_schedule_upgrade_unauthorized() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let err = mgr.schedule_upgrade(
            RuntimeVersion::new(1, 1, 0),
            MIN_UPGRADE_DELAY_SLOTS + 100,
            1,
            0,
            random_addr(),
        );
        assert!(matches!(err, Err(RuntimeError::UpgradeUnauthorized)));
    }

    #[test]
    fn test_schedule_upgrade_too_soon() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let err = mgr.schedule_upgrade(
            RuntimeVersion::new(1, 1, 0),
            100, // çok yakın
            1,
            0,
            gov_addr(),
        );
        assert!(matches!(err, Err(RuntimeError::UpgradeNotReady { .. })));
    }

    #[test]
    fn test_schedule_downgrade_rejected() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(2, 0, 0));
        let err = mgr.schedule_upgrade(
            RuntimeVersion::new(1, 0, 0), // Downgrade
            MIN_UPGRADE_DELAY_SLOTS + 100,
            1,
            0,
            gov_addr(),
        );
        assert!(matches!(err, Err(RuntimeError::InvalidRuntimeVersion)));
    }

    #[test]
    fn test_try_apply_not_yet() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        mgr.schedule_upgrade(
            RuntimeVersion::new(1, 1, 0),
            MIN_UPGRADE_DELAY_SLOTS + 1_000,
            1,
            0,
            gov_addr(),
        )
        .unwrap();
        let result = mgr.try_apply(100);
        assert!(result.is_none());
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 0, 0));
    }

    #[test]
    fn test_try_apply_activates() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let activation = MIN_UPGRADE_DELAY_SLOTS + 100;
        mgr.schedule_upgrade(RuntimeVersion::new(1, 1, 0), activation, 1, 0, gov_addr()).unwrap();
        let result = mgr.try_apply(activation);
        assert!(result.is_some());
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 1, 0));
        assert_eq!(mgr.applied_count(), 1);
        assert!(mgr.pending_upgrade().is_none());
    }

    #[test]
    fn test_cancel_pending() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        mgr.schedule_upgrade(
            RuntimeVersion::new(1, 1, 0),
            MIN_UPGRADE_DELAY_SLOTS + 100,
            1,
            0,
            gov_addr(),
        )
        .unwrap();
        mgr.cancel_pending();
        assert!(mgr.pending_upgrade().is_none());
    }

    #[test]
    fn test_version_ordering() {
        assert!(RuntimeVersion::new(1, 1, 0) > RuntimeVersion::new(1, 0, 0));
        assert!(RuntimeVersion::new(2, 0, 0) > RuntimeVersion::new(1, 9, 9));
        assert_eq!(RuntimeVersion::new(1, 0, 0), RuntimeVersion::new(1, 0, 0));
    }

    #[test]
    fn test_version_as_u32_roundtrip() {
        let v = RuntimeVersion::new(1, 2, 3);
        let n = v.as_u32();
        let v2 = RuntimeVersion::from_u32(n);
        assert_eq!(v, v2);
    }

    #[test]
    fn test_version_display() {
        assert_eq!(RuntimeVersion::new(1, 2, 3).to_string(), "1.2.3");
    }

    // ─── İki Fazlı Upgrade Testleri ──────────────────────────────────────────

    #[test]
    fn test_authorize_upgrade_success() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let code = b"new runtime binary v1.1.0";
        let code_hash = nova_crypto::hash::blake3_hash(code);

        mgr.authorize_upgrade(
            code_hash.clone(),
            RuntimeVersion::new(1, 1, 0),
            42,
            100,
            gov_addr(),
        ).unwrap();

        let auth = mgr.pending_authorization().unwrap();
        assert_eq!(auth.code_hash, code_hash);
        assert_eq!(auth.to_version, RuntimeVersion::new(1, 1, 0));
        assert_eq!(auth.proposal_id, 42);
    }

    #[test]
    fn test_authorize_upgrade_unauthorized() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let code_hash = nova_crypto::hash::blake3_hash(b"dummy");
        let err = mgr.authorize_upgrade(
            code_hash,
            RuntimeVersion::new(1, 1, 0),
            1,
            100,
            random_addr(),
        );
        assert!(matches!(err, Err(RuntimeError::UpgradeUnauthorized)));
    }

    #[test]
    fn test_authorize_upgrade_downgrade_rejected() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(2, 0, 0));
        let code_hash = nova_crypto::hash::blake3_hash(b"dummy");
        let err = mgr.authorize_upgrade(
            code_hash,
            RuntimeVersion::new(1, 0, 0),
            1,
            100,
            gov_addr(),
        );
        assert!(matches!(err, Err(RuntimeError::InvalidRuntimeVersion)));
    }

    #[test]
    fn test_apply_authorized_upgrade_success() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let code = b"new runtime binary";
        let code_hash = nova_crypto::hash::blake3_hash(code);

        mgr.authorize_upgrade(code_hash.clone(), RuntimeVersion::new(1, 1, 0), 1, 100, gov_addr()).unwrap();

        let new_ver = mgr.apply_authorized_upgrade(code, 200).unwrap();
        assert_eq!(new_ver, RuntimeVersion::new(1, 1, 0));
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 1, 0));
        assert!(mgr.pending_authorization().is_none(), "yetkilendirme temizlenmeli");
        assert_eq!(mgr.applied_count(), 1);
    }

    #[test]
    fn test_apply_authorized_upgrade_hash_mismatch() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let correct_code = b"correct runtime binary";
        let wrong_code = b"tampered runtime binary";
        let code_hash = nova_crypto::hash::blake3_hash(correct_code);

        mgr.authorize_upgrade(code_hash.clone(), RuntimeVersion::new(1, 1, 0), 1, 100, gov_addr()).unwrap();

        let err = mgr.apply_authorized_upgrade(wrong_code, 200);
        assert!(
            matches!(err, Err(RuntimeError::UpgradeHashMismatch { .. })),
            "hash uyuşmazlığı reddedilmeli: {err:?}"
        );
        // Versiyon değişmemeli
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 0, 0));
    }

    #[test]
    fn test_apply_without_authorization_rejected() {
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let code = b"some code";
        let err = mgr.apply_authorized_upgrade(code, 100);
        assert!(matches!(err, Err(RuntimeError::UpgradeNotAuthorized)));
    }

    #[test]
    fn test_two_phase_prevents_supply_chain_attack() {
        // Governance hash'i onaylıyor, sonra biri farklı kod yüklemeye çalışıyor
        let mgr = UpgradeManager::new(RuntimeVersion::new(1, 0, 0));
        let legitimate_code = b"legitimate v1.1 code";
        let malicious_code = b"backdoored code";

        let code_hash = nova_crypto::hash::blake3_hash(legitimate_code);
        mgr.authorize_upgrade(code_hash.clone(), RuntimeVersion::new(1, 1, 0), 99, 100, gov_addr()).unwrap();

        // Saldırgan farklı kod yüklemeye çalışıyor
        let err = mgr.apply_authorized_upgrade(malicious_code, 200);
        assert!(matches!(err, Err(RuntimeError::UpgradeHashMismatch { .. })),
            "supply chain saldırısı önlenmeli");

        // Versiyon değişmemeli
        assert_eq!(mgr.current_version(), RuntimeVersion::new(1, 0, 0));
    }
}
