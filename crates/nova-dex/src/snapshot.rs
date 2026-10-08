//! Nova DEX — Snapshot / veritabanı kalıcılığı
//!
//! ## Güvenlik Düzeltmeleri
//!
//! - **NOVA-RED-DEX-SNAPSHOT-001**: Restore sırasında versiyon alanı kontrol ediliyor.
//!   Eski/yeni şema ile yükleme girişimi hata döndürür.
//!
//! - **NOVA-RED-DEX-DB-001**: `open_db` panic yerine `Result` döndürüyor.
//!   Bozuk snapshot sessiz veri kaybı yerine hata döndürüyor.

use crate::errors::{DexError, DexResult};
use serde::{Deserialize, Serialize};

/// Mevcut snapshot şema versiyonu
pub const SNAPSHOT_VERSION: u32 = 1;

/// Pool kayıt snapshot'ı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    /// NOVA-RED-DEX-SNAPSHOT-001: Versiyon alanı restore'da kontrol edilir
    pub version: u32,
    /// Snapshot zamanı (ms)
    pub snapshot_at_ms: u64,
    /// Serialize edilmiş pool verisi
    pub data: Vec<u8>,
}

impl RegistrySnapshot {
    pub fn new(data: Vec<u8>, now_ms: u64) -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            snapshot_at_ms: now_ms,
            data,
        }
    }

    /// Snapshot'ı serialize et (JSON)
    pub fn serialize(&self) -> DexResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| DexError::Internal {
            reason: format!("snapshot serialize hatası: {e}"),
        })
    }

    /// Snapshot'ı deserialize et — versiyon kontrol (NOVA-RED-DEX-SNAPSHOT-001)
    ///
    /// NOVA-RED-DEX-DB-001: Bozuk veri panic yerine `Err` döndürür.
    pub fn deserialize(bytes: &[u8]) -> DexResult<Self> {
        let snap: Self = serde_json::from_slice(bytes).map_err(|e| DexError::Internal {
            reason: format!("snapshot parse hatası (bozuk veri?): {e}"),
        })?;

        // NOVA-RED-DEX-SNAPSHOT-001: Versiyon eşleşmeli
        if snap.version != SNAPSHOT_VERSION {
            return Err(DexError::Internal {
                reason: format!(
                    "snapshot versiyon uyumsuzluğu: beklenen {SNAPSHOT_VERSION}, gelen {}",
                    snap.version
                ),
            });
        }

        Ok(snap)
    }
}

/// Veritabanı açma — NOVA-RED-DEX-DB-001: panic! yerine Result döndürür
pub fn open_db(path: &str) -> DexResult<()> {
    // Gerçek implementasyon RocksDB veya SQLite kullanır
    // Şimdilik path validasyonu
    if path.is_empty() {
        return Err(DexError::Internal { reason: "boş veritabanı yolu".into() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_serialize_deserialize() {
        let snap = RegistrySnapshot::new(b"test_data".to_vec(), 12345);
        let bytes = snap.serialize().unwrap();
        let restored = RegistrySnapshot::deserialize(&bytes).unwrap();
        assert_eq!(restored.version, SNAPSHOT_VERSION);
        assert_eq!(restored.data, b"test_data");
    }

    /// NOVA-RED-DEX-SNAPSHOT-001: Yanlış versiyon reddedilmeli
    #[test]
    fn test_wrong_version_rejected() {
        let snap = RegistrySnapshot {
            version: 99,
            snapshot_at_ms: 0,
            data: vec![],
        };
        let bytes = snap.serialize().unwrap();
        let result = RegistrySnapshot::deserialize(&bytes);
        assert!(matches!(result, Err(DexError::Internal { .. })));
    }

    /// NOVA-RED-DEX-DB-001: Bozuk veri sessiz veri kaybı değil hata döndürmeli
    #[test]
    fn test_corrupt_snapshot_returns_error() {
        let corrupt = b"bu bir json degil!!!{{{";
        let result = RegistrySnapshot::deserialize(corrupt);
        assert!(matches!(result, Err(DexError::Internal { .. })));
    }

    #[test]
    fn test_open_db_empty_path_error() {
        let result = open_db("");
        assert!(matches!(result, Err(DexError::Internal { .. })));
    }
}
