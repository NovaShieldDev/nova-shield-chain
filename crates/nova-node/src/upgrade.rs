//! Runtime upgrade yöneticisi — on-chain governance kararı ile runtime güncelleme
//!
//! Polkadot Substrate `RuntimeUpgrade` + Avalanchego upgrade.json pattern:
//! - `PendingUpgrade`: governance tarafından oylanmış, henüz uygulanmamış yükseltme
//! - `UpgradeRecord`: geçmişte uygulanan yükseltmelerin kaydı
//! - `UpgradeManager`: her blokta `check()` → bekleyen upgrade varsa `apply()`
//!
//! Upgrade koşulları:
//! 1. Blok yüksekliği `activate_at_block`'a ulaştığında aktif hale gelir.
//! 2. `is_emergency=true` ise hemen uygulanır (governance bypass — güvenlik açığı yamalarında).
//! 3. Aynı upgrade iki kez uygulanamaz (idempotency guarantee).

use crate::errors::{NodeError, NodeResult};
use crate::NOVA_PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── UpgradeKind ──────────────────────────────────────────────────────────────

/// Upgrade türü
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeKind {
    /// Protocol versiyonu artışı (consensus kuralları değişiyor)
    ProtocolVersion {
        /// Yeni protocol versiyonu
        new_version: u32,
    },
    /// Parametre güncellemesi (ör. gas limiti, blok boyutu)
    ParameterChange {
        /// Güncellenen parametre adı
        key: String,
        /// Yeni parametre değeri (JSON)
        value: String,
    },
    /// Acil güvenlik yaması (emergency upgrade)
    SecurityPatch {
        /// Yama açıklaması
        description: String,
    },
    /// Genel runtime yükseltme (WASM hash ile)
    RuntimeWasm {
        /// Yeni WASM modülü hash'i (hex, BLAKE3)
        wasm_hash: String,
    },
}

// ─── PendingUpgrade ───────────────────────────────────────────────────────────

/// Governance tarafından onaylanmış, henüz uygulanmamış upgrade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUpgrade {
    /// Benzersiz upgrade kimliği
    pub id: u64,
    /// Upgrade türü
    pub kind: UpgradeKind,
    /// Bu blokta veya sonrasında aktif olur
    pub activate_at_block: u64,
    /// Acil durum — governance bypass
    pub is_emergency: bool,
    /// Upgrade açıklaması (governance proposal metni)
    pub description: String,
    /// Governance proposal hash'i (onay kanıtı)
    pub proposal_hash: String,
}

impl PendingUpgrade {
    /// Belirtilen blok yüksekliğinde aktif mi?
    pub fn is_active_at(&self, block_height: u64) -> bool {
        self.is_emergency || block_height >= self.activate_at_block
    }
}

// ─── UpgradeRecord ────────────────────────────────────────────────────────────

/// Uygulanmış bir upgrade'in kaydı (audit trail)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeRecord {
    /// Upgrade kimliği
    pub id: u64,
    /// Upgrade türü
    pub kind: UpgradeKind,
    /// Uygulama bloğu
    pub applied_at_block: u64,
    /// Uygulama timestamp (Unix saniye)
    pub applied_at_ts: u64,
    /// Uygulama öncesi protocol versiyonu
    pub prev_version: u32,
    /// Uygulama sonrası protocol versiyonu
    pub new_version: u32,
}

// ─── UpgradeState ─────────────────────────────────────────────────────────────

/// Upgrade yöneticisinin anlık durumu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeState {
    /// Geçerli protocol versiyonu
    pub current_version: u32,
    /// Beklleyen upgrade listesi
    pub pending: Vec<PendingUpgrade>,
    /// Uygulanan upgrade geçmişi
    pub history: Vec<UpgradeRecord>,
}

impl UpgradeState {
    fn new() -> Self {
        Self {
            current_version: NOVA_PROTOCOL_VERSION,
            pending: Vec::new(),
            history: Vec::new(),
        }
    }
}

// ─── UpgradeManager ───────────────────────────────────────────────────────────

/// Runtime upgrade yöneticisi
///
/// Her blok sonunda `check(block_height)` çağrılır.
/// Aktif upgrade varsa `apply()` ile uygulanır ve `UpgradeRecord` oluşturulur.
pub struct UpgradeManager {
    state: UpgradeState,
    /// Upgrade state dosyası (kalıcı depolama)
    state_path: Option<std::path::PathBuf>,
}

impl UpgradeManager {
    /// Yeni upgrade yöneticisi (in-memory)
    pub fn new() -> Self {
        Self {
            state: UpgradeState::new(),
            state_path: None,
        }
    }

    /// Dosyadan yükleyerek başlat
    ///
    /// Dosya yoksa yeni state oluşturulur.
    pub fn load(path: impl AsRef<Path>) -> NodeResult<Self> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| NodeError::DatabaseError {
                    reason: format!("upgrade state okunamadı: {e}"),
                })?;
            serde_json::from_str(&content).map_err(|e| NodeError::SerializationError {
                reason: format!("upgrade state parse hatası: {e}"),
            })?
        } else {
            UpgradeState::new()
        };
        Ok(Self { state, state_path: Some(path) })
    }

    /// Geçerli protocol versiyonu
    pub fn current_version(&self) -> u32 {
        self.state.current_version
    }

    /// Bekleyen upgrade sayısı
    pub fn pending_count(&self) -> usize {
        self.state.pending.len()
    }

    /// Uygulanan upgrade geçmişi
    pub fn history(&self) -> &[UpgradeRecord] {
        &self.state.history
    }

    /// Governance upgrade ekle
    ///
    /// Aynı ID'li upgrade zaten varsa `Err` döner.
    pub fn schedule_upgrade(&mut self, upgrade: PendingUpgrade) -> NodeResult<()> {
        if self.state.pending.iter().any(|u| u.id == upgrade.id) {
            return Err(NodeError::ConfigError {
                reason: format!("upgrade {} zaten planlanmış", upgrade.id),
            });
        }
        self.state.pending.push(upgrade);
        // ID'ye göre sırala (deterministik uygulama sırası)
        self.state.pending.sort_by_key(|u| u.id);
        self.persist()?;
        Ok(())
    }

    /// Upgrade iptal et
    pub fn cancel_upgrade(&mut self, id: u64) -> NodeResult<()> {
        let before = self.state.pending.len();
        self.state.pending.retain(|u| u.id != id);
        if self.state.pending.len() == before {
            return Err(NodeError::ConfigError {
                reason: format!("upgrade {id} bulunamadı"),
            });
        }
        self.persist()?;
        Ok(())
    }

    /// Bu blokta uygulanması gereken upgrade var mı?
    ///
    /// Returns: aktif upgrade listesi (ID sırasıyla)
    pub fn check(&self, block_height: u64) -> Vec<&PendingUpgrade> {
        self.state
            .pending
            .iter()
            .filter(|u| u.is_active_at(block_height))
            .collect()
    }

    /// Belirtilen blok yüksekliğindeki tüm bekleyen upgrade'leri uygula
    ///
    /// Her upgrade için:
    /// 1. `UpgradeRecord` oluştur
    /// 2. Protocol versiyonunu güncelle (ProtocolVersion türündeyse)
    /// 3. Pending listesinden çıkar
    /// 4. History'ye ekle
    pub fn apply_upgrades(&mut self, block_height: u64) -> NodeResult<Vec<UpgradeRecord>> {
        let active_ids: Vec<u64> = self
            .state
            .pending
            .iter()
            .filter(|u| u.is_active_at(block_height))
            .map(|u| u.id)
            .collect();

        if active_ids.is_empty() {
            return Ok(vec![]);
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut applied = Vec::new();
        for id in active_ids {
            let idx = self.state.pending.iter().position(|u| u.id == id).unwrap();
            let upgrade = self.state.pending.remove(idx);
            let prev_version = self.state.current_version;

            // Protocol versiyonunu güncelle
            if let UpgradeKind::ProtocolVersion { new_version } = &upgrade.kind {
                self.state.current_version = *new_version;
            }

            let record = UpgradeRecord {
                id: upgrade.id,
                kind: upgrade.kind,
                applied_at_block: block_height,
                applied_at_ts: ts,
                prev_version,
                new_version: self.state.current_version,
            };
            self.state.history.push(record.clone());
            applied.push(record);
        }

        self.persist()?;
        Ok(applied)
    }

    /// State'i dosyaya kaydet
    fn persist(&self) -> NodeResult<()> {
        let Some(ref path) = self.state_path else { return Ok(()); };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NodeError::DatabaseError {
                reason: format!("upgrade dizini oluşturulamadı: {e}"),
            })?;
        }
        let json =
            serde_json::to_string_pretty(&self.state).map_err(|e| NodeError::SerializationError {
                reason: format!("upgrade state serileştirme hatası: {e}"),
            })?;
        std::fs::write(path, json).map_err(|e| NodeError::DatabaseError {
            reason: format!("upgrade state yazılamadı: {e}"),
        })?;
        Ok(())
    }
}

impl Default for UpgradeManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_upgrade(id: u64, activate_at: u64) -> PendingUpgrade {
        PendingUpgrade {
            id,
            kind: UpgradeKind::ProtocolVersion { new_version: NOVA_PROTOCOL_VERSION + id as u32 },
            activate_at_block: activate_at,
            is_emergency: false,
            description: format!("Test upgrade {id}"),
            proposal_hash: format!("{:064x}", id),
        }
    }

    #[test]
    fn test_upgrade_manager_new() {
        let mgr = UpgradeManager::new();
        assert_eq!(mgr.current_version(), NOVA_PROTOCOL_VERSION);
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(mgr.history().len(), 0);
    }

    #[test]
    fn test_schedule_upgrade() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn test_schedule_duplicate_fails() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        assert!(mgr.schedule_upgrade(make_upgrade(1, 200)).is_err());
    }

    #[test]
    fn test_cancel_upgrade() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        mgr.cancel_upgrade(1).unwrap();
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn test_cancel_nonexistent_fails() {
        let mut mgr = UpgradeManager::new();
        assert!(mgr.cancel_upgrade(99).is_err());
    }

    #[test]
    fn test_check_returns_active_only() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        mgr.schedule_upgrade(make_upgrade(2, 200)).unwrap();

        let active = mgr.check(100);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, 1);

        let active2 = mgr.check(200);
        assert_eq!(active2.len(), 2);
    }

    #[test]
    fn test_check_none_before_activation() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        assert!(mgr.check(99).is_empty());
    }

    #[test]
    fn test_emergency_upgrade_immediate() {
        let mut mgr = UpgradeManager::new();
        let emergency = PendingUpgrade {
            id: 1,
            kind: UpgradeKind::SecurityPatch { description: "CVE-0001".into() },
            activate_at_block: 9_999_999,
            is_emergency: true,
            description: "Emergency patch".into(),
            proposal_hash: "0".repeat(64),
        };
        mgr.schedule_upgrade(emergency).unwrap();
        // Blok 1'de dahi aktif olmalı
        assert_eq!(mgr.check(1).len(), 1);
    }

    #[test]
    fn test_apply_upgrades_protocol_version() {
        let mut mgr = UpgradeManager::new();
        let new_ver = NOVA_PROTOCOL_VERSION + 1;
        let upgrade = PendingUpgrade {
            id: 1,
            kind: UpgradeKind::ProtocolVersion { new_version: new_ver },
            activate_at_block: 100,
            is_emergency: false,
            description: "v2 upgrade".into(),
            proposal_hash: "a".repeat(64),
        };
        mgr.schedule_upgrade(upgrade).unwrap();
        let records = mgr.apply_upgrades(100).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(mgr.current_version(), new_ver);
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(mgr.history().len(), 1);
    }

    #[test]
    fn test_apply_upgrades_no_active_returns_empty() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 200)).unwrap();
        let records = mgr.apply_upgrades(100).unwrap();
        assert!(records.is_empty());
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn test_apply_multiple_upgrades() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        mgr.schedule_upgrade(make_upgrade(2, 100)).unwrap();
        let records = mgr.apply_upgrades(100).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(mgr.pending_count(), 0);
        assert_eq!(mgr.history().len(), 2);
    }

    #[test]
    fn test_apply_idempotent() {
        let mut mgr = UpgradeManager::new();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        mgr.apply_upgrades(100).unwrap();
        // İkinci çağrıda upgrade zaten uygulanmış, pending yok
        let records2 = mgr.apply_upgrades(100).unwrap();
        assert!(records2.is_empty());
    }

    #[test]
    fn test_upgrade_record_fields() {
        let mut mgr = UpgradeManager::new();
        let new_ver = NOVA_PROTOCOL_VERSION + 5;
        let upgrade = PendingUpgrade {
            id: 42,
            kind: UpgradeKind::ProtocolVersion { new_version: new_ver },
            activate_at_block: 500,
            is_emergency: false,
            description: "Big upgrade".into(),
            proposal_hash: "b".repeat(64),
        };
        mgr.schedule_upgrade(upgrade).unwrap();
        let records = mgr.apply_upgrades(500).unwrap();
        let rec = &records[0];
        assert_eq!(rec.id, 42);
        assert_eq!(rec.applied_at_block, 500);
        assert_eq!(rec.prev_version, NOVA_PROTOCOL_VERSION);
        assert_eq!(rec.new_version, new_ver);
    }

    #[test]
    fn test_upgrade_manager_save_and_load() {
        let dir = std::env::temp_dir().join(format!(
            "nova-upgrade-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let path = dir.join("upgrade_state.json");

        let mut mgr = UpgradeManager::load(&path).unwrap();
        mgr.schedule_upgrade(make_upgrade(1, 100)).unwrap();
        mgr.apply_upgrades(100).unwrap();

        let mgr2 = UpgradeManager::load(&path).unwrap();
        assert_eq!(mgr2.history().len(), 1);
        assert_eq!(mgr2.pending_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parameter_change_upgrade() {
        let mut mgr = UpgradeManager::new();
        let upgrade = PendingUpgrade {
            id: 1,
            kind: UpgradeKind::ParameterChange {
                key: "max_block_gas".into(),
                value: "60000000".into(),
            },
            activate_at_block: 50,
            is_emergency: false,
            description: "Gas limit artışı".into(),
            proposal_hash: "c".repeat(64),
        };
        mgr.schedule_upgrade(upgrade).unwrap();
        let records = mgr.apply_upgrades(50).unwrap();
        assert_eq!(records.len(), 1);
        // ParameterChange versiyonu değiştirmez
        assert_eq!(mgr.current_version(), NOVA_PROTOCOL_VERSION);
    }
}
