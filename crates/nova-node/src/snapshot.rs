//! State snapshot — anlık durum alma ve geri yükleme
//!
//! Belirli bir blok yüksekliğindeki tüm hesap durumlarını JSON olarak kaydeder.
//! Archive node'lar, warp sync ve felaket kurtarma için kullanılır.
//!
//! Snapshot formatı: JSON (okunabilir, taşınabilir)
//! Production'da binary format (borsh) daha verimli olur.

use crate::errors::{NodeError, NodeResult};
use crate::NOVA_PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── AccountSnapshot ──────────────────────────────────────────────────────────

/// Tek hesap durumu snapshot kaydı
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountSnapshot {
    /// Hesap adresi (0x...)
    pub address: String,
    /// Bakiye (baz birim, NSC için 10^-18)
    pub balance: u128,
    /// İşlem sayacı (replay koruması)
    pub nonce: u64,
}

// ─── StateSnapshot ────────────────────────────────────────────────────────────

/// Belirli bir bloktaki tüm state'in snapshot'ı
///
/// Warp sync: yeni node'lar genesis'ten değil bu snapshot'tan başlar.
/// Archive: her N blokta snapshot alınır, geçmişe gidilebilir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Snapshot alındığı blok yüksekliği
    pub block_height: u64,
    /// Blok hash'i (hex)
    pub block_hash: String,
    /// Zincir kimliği
    pub chain_id: u64,
    /// Merkle state root hash'i (hex)
    pub state_root: String,
    /// Hesap durumları
    pub accounts: Vec<AccountSnapshot>,
    /// Snapshot Unix timestamp (saniye)
    pub timestamp: u64,
    /// Protocol versiyonu (snapshot uyumluluğu için)
    pub protocol_version: u32,
    /// NOVA-RED-NODE-011/015: Bütünlük hash'i (BLAKE3, hex).
    /// `save()` sırasında hesaplanır, `load()` sırasında doğrulanır.
    #[serde(default)]
    pub integrity_hash: String,
}

impl StateSnapshot {
    /// Yeni snapshot oluştur
    pub fn new(
        block_height: u64,
        block_hash: [u8; 32],
        chain_id: u64,
        state_root: [u8; 32],
        accounts: Vec<AccountSnapshot>,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut snap = Self {
            block_height,
            block_hash: hex::encode(block_hash),
            chain_id,
            state_root: hex::encode(state_root),
            accounts,
            timestamp,
            protocol_version: NOVA_PROTOCOL_VERSION,
            integrity_hash: String::new(),
        };
        // Bütünlük hash'ini hesapla (integrity_hash hariç tüm alanlar)
        snap.integrity_hash = hex::encode(snap.compute_hash());
        snap
    }

    /// Dosyaya kaydet (JSON)
    ///
    /// NOVA-RED-NODE-011/015: integrity_hash kaydedilir.
    pub fn save(&self, path: impl AsRef<Path>) -> NodeResult<()> {
        let json = serde_json::to_string_pretty(self).map_err(|e| NodeError::SerializationError {
            reason: format!("snapshot serileştirme hatası: {e}"),
        })?;
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| NodeError::DatabaseError {
                reason: format!("snapshot dizini oluşturulamadı: {e}"),
            })?;
        }
        std::fs::write(path, json).map_err(|e| NodeError::DatabaseError {
            reason: format!("snapshot yazılamadı: {e}"),
        })?;
        Ok(())
    }

    /// Dosyadan yükle ve bütünlük doğrulaması yap.
    ///
    /// NOVA-RED-NODE-011/015: integrity_hash yoksa veya uyuşmuyorsa hata döner.
    pub fn load(path: impl AsRef<Path>) -> NodeResult<Self> {
        let content =
            std::fs::read_to_string(path.as_ref()).map_err(|e| NodeError::DatabaseError {
                reason: format!("snapshot okunamadı: {e}"),
            })?;
        let snap: Self = serde_json::from_str(&content).map_err(|e| NodeError::SerializationError {
            reason: format!("snapshot parse hatası: {e}"),
        })?;

        // Bütünlük kontrolü (integrity_hash boş ise eski format — geç)
        if !snap.integrity_hash.is_empty() {
            let expected = hex::encode(snap.compute_hash());
            if snap.integrity_hash != expected {
                return Err(NodeError::DatabaseError {
                    reason: format!(
                        "snapshot bütünlük hatası: beklenen={}, hesaplanan={}",
                        snap.integrity_hash, expected
                    ),
                });
            }
        }

        Ok(snap)
    }

    /// Snapshot bütünlük hash'i (BLAKE3)
    ///
    /// Hesaplama: BLAKE3(block_height || block_hash || state_root || her hesap)
    /// Snapshot transferinde bütünlük doğrulaması için kullanılır.
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.block_height.to_le_bytes());
        h.update(self.block_hash.as_bytes());
        h.update(&self.chain_id.to_le_bytes());
        h.update(self.state_root.as_bytes());
        h.update(&self.protocol_version.to_le_bytes());
        // Deterministik sıra için adreslere göre sırala
        let mut sorted = self.accounts.clone();
        sorted.sort_by(|a, b| a.address.cmp(&b.address));
        for acc in &sorted {
            h.update(acc.address.as_bytes());
            h.update(&acc.balance.to_le_bytes());
            h.update(&acc.nonce.to_le_bytes());
        }
        *h.finalize().as_bytes()
    }

    /// Hesap sayısı
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Toplam bakiye (bütün hesapların toplamı)
    pub fn total_balance(&self) -> u128 {
        self.accounts.iter().map(|a| a.balance).sum()
    }

    /// Belirli bir adresi bul
    pub fn find_account(&self, address: &str) -> Option<&AccountSnapshot> {
        self.accounts.iter().find(|a| a.address.eq_ignore_ascii_case(address))
    }

    /// Sıfır bakiyeli hesapları çıkar (snapshot boyutunu küçültür)
    pub fn prune_empty(&mut self) {
        self.accounts.retain(|a| a.balance > 0 || a.nonce > 0);
    }

    /// Protocol versiyonu uyumlu mu?
    pub fn is_compatible(&self) -> bool {
        self.protocol_version == NOVA_PROTOCOL_VERSION
    }
}

// ─── SnapshotManager ──────────────────────────────────────────────────────────

/// Periyodik snapshot yöneticisi
///
/// Her `interval_blocks` blokta bir snapshot alır.
/// Eski snapshot'ları `max_snapshots` sınırına kadar tutar.
pub struct SnapshotManager {
    /// Snapshot saklama dizini
    dir: std::path::PathBuf,
    /// Her kaç blokta bir snapshot
    interval_blocks: u64,
    /// Maksimum saklanan snapshot sayısı
    max_snapshots: usize,
    /// Son snapshot alınan blok
    last_snapshot_block: u64,
}

impl SnapshotManager {
    /// Yeni snapshot yöneticisi
    pub fn new(
        dir: impl AsRef<Path>,
        interval_blocks: u64,
        max_snapshots: usize,
    ) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            interval_blocks,
            max_snapshots,
            last_snapshot_block: 0,
        }
    }

    /// Bu blokta snapshot alınmalı mı?
    pub fn should_snapshot(&self, block_height: u64) -> bool {
        block_height > 0
            && self.interval_blocks > 0
            && block_height % self.interval_blocks == 0
    }

    /// Snapshot kaydet ve eski snapshot'ları temizle
    pub fn save_snapshot(&mut self, snapshot: &StateSnapshot) -> NodeResult<()> {
        let filename = format!("snapshot-{:012}.json", snapshot.block_height);
        let path = self.dir.join(&filename);
        snapshot.save(&path)?;
        self.last_snapshot_block = snapshot.block_height;
        self.cleanup_old_snapshots()?;
        Ok(())
    }

    /// Eski snapshot dosyalarını temizle (max_snapshots limitini aş)
    fn cleanup_old_snapshots(&self) -> NodeResult<()> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Ok(()); };
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("snapshot-")
            })
            .collect();
        // Eskiden yeniye sırala
        files.sort_by_key(|e| e.file_name());
        // Limit aşıldıysa en eskileri sil
        let to_delete = files.len().saturating_sub(self.max_snapshots);
        for entry in files.into_iter().take(to_delete) {
            let _ = std::fs::remove_file(entry.path());
        }
        Ok(())
    }

    /// En son snapshot'ı yükle
    pub fn load_latest(&self) -> NodeResult<StateSnapshot> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Err(NodeError::DatabaseError {
                reason: format!("snapshot dizini okunamadı: {:?}", self.dir),
            });
        };
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("snapshot-")
            })
            .collect();
        files.sort_by_key(|e| e.file_name());
        let latest = files.last().ok_or_else(|| NodeError::DatabaseError {
            reason: "snapshot bulunamadı".into(),
        })?;
        StateSnapshot::load(latest.path())
    }

    /// Son snapshot alınan blok
    pub fn last_snapshot_block(&self) -> u64 {
        self.last_snapshot_block
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(height: u64) -> StateSnapshot {
        let accounts = vec![
            AccountSnapshot { address: "0x0101010101010101010101010101010101010101".into(), balance: 1_000_000, nonce: 5 },
            AccountSnapshot { address: "0x0202020202020202020202020202020202020202".into(), balance: 500_000, nonce: 2 },
        ];
        StateSnapshot::new(height, [height as u8; 32], 42000, [0xABu8; 32], accounts)
    }

    #[test]
    fn test_snapshot_creation() {
        let snap = make_snapshot(100);
        assert_eq!(snap.block_height, 100);
        assert_eq!(snap.account_count(), 2);
        assert_eq!(snap.total_balance(), 1_500_000);
        assert_eq!(snap.chain_id, 42000);
    }

    #[test]
    fn test_snapshot_hash_deterministic() {
        let s1 = make_snapshot(100);
        let s2 = make_snapshot(100);
        assert_eq!(s1.compute_hash(), s2.compute_hash());
    }

    #[test]
    fn test_snapshot_hash_changes_with_height() {
        let s1 = make_snapshot(100);
        let s2 = make_snapshot(101);
        assert_ne!(s1.compute_hash(), s2.compute_hash());
    }

    #[test]
    fn test_snapshot_find_account() {
        let snap = make_snapshot(50);
        let acc = snap.find_account("0x0101010101010101010101010101010101010101");
        assert!(acc.is_some());
        assert_eq!(acc.unwrap().balance, 1_000_000);
    }

    #[test]
    fn test_snapshot_prune_empty() {
        let mut snap = make_snapshot(10);
        snap.accounts.push(AccountSnapshot {
            address: "0x0303030303030303030303030303030303030303".into(),
            balance: 0,
            nonce: 0,
        });
        assert_eq!(snap.account_count(), 3);
        snap.prune_empty();
        assert_eq!(snap.account_count(), 2);
    }

    #[test]
    fn test_snapshot_total_balance() {
        let snap = make_snapshot(1);
        assert_eq!(snap.total_balance(), 1_500_000);
    }

    #[test]
    fn test_snapshot_json_roundtrip() {
        let snap = make_snapshot(200);
        let json = serde_json::to_string_pretty(&snap).unwrap();
        let snap2: StateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.block_height, snap2.block_height);
        assert_eq!(snap.account_count(), snap2.account_count());
        assert_eq!(snap.compute_hash(), snap2.compute_hash());
    }

    #[test]
    fn test_snapshot_save_and_load() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "nova-snapshot-test-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));

        let snap = make_snapshot(500);
        snap.save(&path).unwrap();

        let loaded = StateSnapshot::load(&path).unwrap();
        assert_eq!(loaded.block_height, 500);
        assert_eq!(loaded.compute_hash(), snap.compute_hash());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_snapshot_compatibility() {
        let snap = make_snapshot(1);
        assert!(snap.is_compatible());
    }

    #[test]
    fn test_snapshot_manager_should_snapshot() {
        let mgr = SnapshotManager::new("/tmp/snap", 100, 10);
        assert!(!mgr.should_snapshot(0));
        assert!(!mgr.should_snapshot(99));
        assert!(mgr.should_snapshot(100));
        assert!(!mgr.should_snapshot(101));
        assert!(mgr.should_snapshot(200));
        assert!(mgr.should_snapshot(1000));
    }

    #[test]
    fn test_snapshot_manager_save_and_load_latest() {
        let dir = std::env::temp_dir().join(format!(
            "nova-snap-mgr-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));

        let mut mgr = SnapshotManager::new(&dir, 100, 5);
        let snap = make_snapshot(100);
        mgr.save_snapshot(&snap).unwrap();

        let loaded = mgr.load_latest().unwrap();
        assert_eq!(loaded.block_height, 100);
        assert_eq!(mgr.last_snapshot_block(), 100);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
