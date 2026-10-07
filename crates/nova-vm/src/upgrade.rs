//! GÖREV 93: Forkless Runtime Upgrade — set_code extrinsic (Polkadot pattern)
//!
//! ## Mimari
//!
//! ### Polkadot `system::set_code`
//! - Governance onayından sonra çalışır
//! - Yeni WASM runtime kodu zincire yazılır
//! - Bir sonraki blok yeni kodu kullanır (forkless)
//! - `can_set_code` hook: spec_version kontrolü, WASM geçerlilik
//!
//! ### Nova Uyarlaması
//! - `SetCodeExtrinsic`: governance onaylı runtime upgrade
//! - `RuntimeCodeStore`: aktif runtime kodu + version tarihi
//! - `SetCodeAuthority`: sadece governance modülü çağırabilir
//! - WASM magic bytes doğrulama
//! - Spec version monoton artmalı (downgrade önleme)

use blake3;
use crate::errors::{VmError, VmResult};
use crate::wasm::runtime::WASM_MAGIC;
use nova_primitives::Address;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Maksimum WASM runtime kodu boyutu (5 MB)
pub const MAX_RUNTIME_CODE_SIZE: usize = 5 * 1024 * 1024;

/// Governance modülü adresi (sadece bu adres set_code çağırabilir)
pub const GOVERNANCE_MODULE_ADDR: [u8; 20] = [0x47u8; 20];

/// Kaç önceki runtime versiyonu saklanır?
pub const RUNTIME_HISTORY_DEPTH: usize = 10;

// ─── RuntimeCodeVersion ───────────────────────────────────────────────────────

/// Runtime kod versiyonu
///
/// Polkadot: `RuntimeVersion { spec_name, impl_name, authoring_version, spec_version, ... }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCodeVersion {
    /// Spec versiyonu (monoton artar)
    pub spec_version: u32,
    /// İmplementasyon versiyonu
    pub impl_version: u32,
    /// WASM kodu hash'i (BLAKE3)
    pub code_hash: [u8; 32],
    /// Kod boyutu (bayt)
    pub code_size: usize,
    /// Hangi blok yüksekliğinde etkinleşti?
    pub activated_at_block: u64,
    /// Hangi slot'ta yüklendi?
    pub uploaded_at_slot: u64,
}

impl RuntimeCodeVersion {
    /// Geçerli mi? Spec version > 0 ve hash sıfır değil
    pub fn is_valid(&self) -> bool {
        self.spec_version > 0 && self.code_hash != [0u8; 32]
    }
}

// ─── SetCodeExtrinsic ─────────────────────────────────────────────────────────

/// set_code extrinsic parametreleri
///
/// Polkadot: `frame_system::Call::set_code { code: Vec<u8> }`
#[derive(Debug, Clone)]
pub struct SetCodeExtrinsic {
    /// Yeni WASM runtime kodu
    pub code: Vec<u8>,
    /// Yeni spec versiyonu
    pub new_spec_version: u32,
    /// Yeni impl versiyonu
    pub new_impl_version: u32,
    /// Çağıranın adresi (governance modülü olmalı)
    pub caller: Address,
    /// Extrinsic'in gönderildiği slot
    pub submitted_at_slot: u64,
}

// ─── RuntimeCodeStore ─────────────────────────────────────────────────────────

/// Aktif runtime kodunu ve versiyon geçmişini saklar
///
/// Polkadot: `frame_system::CodeUpgraded` event + storage
#[derive(Debug)]
pub struct RuntimeCodeStore {
    inner: Mutex<RuntimeCodeStoreInner>,
}

#[derive(Debug)]
struct RuntimeCodeStoreInner {
    /// Aktif runtime kodu
    active_code: Option<Vec<u8>>,
    /// Aktif versiyon
    active_version: Option<RuntimeCodeVersion>,
    /// Versiyon geçmişi (en yeni önce)
    history: VecDeque<RuntimeCodeVersion>,
    /// Toplam upgrade sayısı
    upgrade_count: u64,
}

impl RuntimeCodeStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RuntimeCodeStoreInner {
                active_code: None,
                active_version: None,
                history: VecDeque::new(),
                upgrade_count: 0,
            }),
        }
    }

    /// Genesis runtime kodunu yükle (ilk versiyon, yetki kontrolü yok).
    ///
    /// VM-009 düzeltmesi: İdempotency guard eklendi. Bu fonksiyon yalnızca bir kez
    /// çağrılabilir (genesis'te). Tekrar çağrılırsa hata döner; böylece genesis kodu
    /// üzerine yazma saldırısı engellenir.
    pub fn initialize(&self, code: Vec<u8>, spec_version: u32) -> VmResult<()> {
        validate_wasm_code(&code)?;

        let code_hash = blake3_hash(&code);
        let code_size = code.len();
        let version = RuntimeCodeVersion {
            spec_version,
            impl_version: 0,
            code_hash,
            code_size,
            activated_at_block: 0,
            uploaded_at_slot: 0,
        };

        let mut inner = self.inner.lock().unwrap();

        // VM-009: Yeniden başlatma koruması — genesis kodu sadece bir kez yazılır
        if inner.active_code.is_some() {
            return Err(VmError::ExecutionFailed {
                reason: "initialize: runtime zaten başlatılmış; genesis kodu üzerine yazılamaz"
                    .to_string(),
            });
        }

        inner.active_code = Some(code);
        inner.active_version = Some(version);
        Ok(())
    }

    /// set_code extrinsic'i uygula
    ///
    /// Polkadot: `frame_executive::execute_block` → `on_runtime_upgrade`
    pub fn apply_set_code(
        &self,
        extrinsic: SetCodeExtrinsic,
        current_block: u64,
    ) -> VmResult<RuntimeCodeVersion> {
        // 1. Yetki kontrolü
        let caller_bytes = *extrinsic.caller.as_bytes();
        if caller_bytes != GOVERNANCE_MODULE_ADDR {
            return Err(VmError::ExecutionFailed {
                reason: "set_code: unauthorized caller (governance module required)".to_string()
            });
        }

        // 2. WASM kodu geçerlilik kontrolü
        validate_wasm_code(&extrinsic.code)?;

        // 3. Boyut kontrolü
        if extrinsic.code.len() > MAX_RUNTIME_CODE_SIZE {
            return Err(VmError::ExecutionFailed {
                reason: format!("set_code: code too large ({} > {})", extrinsic.code.len(), MAX_RUNTIME_CODE_SIZE)
            });
        }

        let code_hash = blake3_hash(&extrinsic.code);
        let code_size = extrinsic.code.len();

        let mut inner = self.inner.lock().unwrap();

        // 4. Spec version monoton artış kontrolü
        if let Some(ref active) = inner.active_version {
            if extrinsic.new_spec_version <= active.spec_version {
                return Err(VmError::ExecutionFailed {
                    reason: format!("set_code: spec_version must increase (current={}, new={})", active.spec_version, extrinsic.new_spec_version)
                });
            }
        }

        // 5. Mevcut versiyonu geçmişe al
        if let Some(old_version) = inner.active_version.take() {
            inner.history.push_front(old_version);
            if inner.history.len() > RUNTIME_HISTORY_DEPTH {
                inner.history.pop_back();
            }
        }

        let new_version = RuntimeCodeVersion {
            spec_version: extrinsic.new_spec_version,
            impl_version: extrinsic.new_impl_version,
            code_hash,
            code_size,
            activated_at_block: current_block,
            uploaded_at_slot: extrinsic.submitted_at_slot,
        };

        inner.active_code = Some(extrinsic.code);
        inner.active_version = Some(new_version.clone());
        inner.upgrade_count += 1;

        Ok(new_version)
    }

    /// Aktif runtime kodunu al
    pub fn active_code(&self) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().active_code.clone()
    }

    /// Aktif versiyonu al
    pub fn active_version(&self) -> Option<RuntimeCodeVersion> {
        self.inner.lock().unwrap().active_version.clone()
    }

    /// Versiyon geçmişi
    pub fn history(&self) -> Vec<RuntimeCodeVersion> {
        self.inner.lock().unwrap().history.iter().cloned().collect()
    }

    /// Toplam upgrade sayısı
    pub fn upgrade_count(&self) -> u64 {
        self.inner.lock().unwrap().upgrade_count
    }

    /// Belirli spec_version'ın geçmişte var mı?
    pub fn has_version(&self, spec_version: u32) -> bool {
        let inner = self.inner.lock().unwrap();
        if let Some(ref v) = inner.active_version {
            if v.spec_version == spec_version { return true; }
        }
        inner.history.iter().any(|v| v.spec_version == spec_version)
    }
}

impl Default for RuntimeCodeStore {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

/// WASM kodu geçerli mi?
fn validate_wasm_code(code: &[u8]) -> VmResult<()> {
    if code.len() < WASM_MAGIC.len() {
        return Err(VmError::InvalidBytecode { reason: "not a valid WASM module".to_string() });
    }
    if &code[..WASM_MAGIC.len()] != WASM_MAGIC {
        return Err(VmError::InvalidBytecode { reason: "not a valid WASM module".to_string() });
    }
    Ok(())
}

fn blake3_hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

// ─── SetCodeHandler ───────────────────────────────────────────────────────────

/// set_code extrinsic'i yöneten handler
///
/// VM executor ile entegrasyon için
pub struct SetCodeHandler {
    store: Arc<RuntimeCodeStore>,
}

impl SetCodeHandler {
    pub fn new(store: Arc<RuntimeCodeStore>) -> Self {
        Self { store }
    }

    pub fn handle(&self, extrinsic: SetCodeExtrinsic, current_block: u64) -> VmResult<RuntimeCodeVersion> {
        self.store.apply_set_code(extrinsic, current_block)
    }

    pub fn active_version(&self) -> Option<RuntimeCodeVersion> {
        self.store.active_version()
    }

    pub fn store(&self) -> Arc<RuntimeCodeStore> {
        Arc::clone(&self.store)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::Address;

    fn gov_addr() -> Address {
        Address::from_bytes(GOVERNANCE_MODULE_ADDR)
    }

    fn user_addr() -> Address {
        Address::from_bytes([0xABu8; 20])
    }

    fn valid_wasm(extra: u8) -> Vec<u8> {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(&[extra; 64]);
        v
    }

    fn make_extrinsic(code: Vec<u8>, new_spec: u32, caller: Address) -> SetCodeExtrinsic {
        SetCodeExtrinsic {
            code,
            new_spec_version: new_spec,
            new_impl_version: 0,
            caller,
            submitted_at_slot: 100,
        }
    }

    // 1. Initialize
    #[test]
    fn test_initialize() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();
        let v = store.active_version().unwrap();
        assert_eq!(v.spec_version, 1);
        assert!(v.is_valid());
    }

    // 2. Geçersiz WASM initialize başarısız
    #[test]
    fn test_initialize_invalid_wasm() {
        let store = RuntimeCodeStore::new();
        assert!(store.initialize(vec![0u8; 10], 1).is_err());
    }

    // VM-009: Çift initialize reddedilir
    #[test]
    fn test_initialize_idempotency_guard() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();
        // İkinci çağrı hata vermeli — genesis kodu üzerine yazılamaz
        let err = store.initialize(valid_wasm(1), 2);
        assert!(err.is_err(), "İkinci initialize çağrısı reddedilmeli");
        // Aktif versiyon değişmemeli
        assert_eq!(store.active_version().unwrap().spec_version, 1);
    }

    // 3. set_code başarılı upgrade
    #[test]
    fn test_set_code_success() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        let ext = make_extrinsic(valid_wasm(1), 2, gov_addr());
        let new_v = store.apply_set_code(ext, 100).unwrap();

        assert_eq!(new_v.spec_version, 2);
        assert_eq!(store.upgrade_count(), 1);
        assert_eq!(store.active_version().unwrap().spec_version, 2);
    }

    // 4. Yetkisiz çağıran reddedilir
    #[test]
    fn test_set_code_unauthorized() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        let ext = make_extrinsic(valid_wasm(1), 2, user_addr());
        assert!(store.apply_set_code(ext, 100).is_err());
    }

    // 5. Spec version downgrade reddedilir
    #[test]
    fn test_set_code_spec_version_downgrade() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 5).unwrap();

        let ext = make_extrinsic(valid_wasm(1), 4, gov_addr()); // 4 < 5
        assert!(store.apply_set_code(ext, 100).is_err());
    }

    // 6. Aynı spec version reddedilir
    #[test]
    fn test_set_code_same_spec_version() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 3).unwrap();

        let ext = make_extrinsic(valid_wasm(1), 3, gov_addr()); // 3 == 3
        assert!(store.apply_set_code(ext, 100).is_err());
    }

    // 7. Geçersiz WASM kodu reddedilir
    #[test]
    fn test_set_code_invalid_wasm() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        let ext = make_extrinsic(vec![0xDE, 0xAD, 0xBE, 0xEF], 2, gov_addr());
        assert!(store.apply_set_code(ext, 100).is_err());
    }

    // 8. Büyük kod reddedilir
    #[test]
    fn test_set_code_too_large() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        let mut big_code = WASM_MAGIC.to_vec();
        big_code.extend_from_slice(&vec![0u8; MAX_RUNTIME_CODE_SIZE + 1]);
        let ext = make_extrinsic(big_code, 2, gov_addr());
        assert!(store.apply_set_code(ext, 100).is_err());
    }

    // 9. Versiyon geçmişi
    #[test]
    fn test_version_history() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        for v in 2..=5u32 {
            let ext = make_extrinsic(valid_wasm(v as u8), v, gov_addr());
            store.apply_set_code(ext, v as u64 * 100).unwrap();
        }

        let history = store.history();
        assert_eq!(history.len(), 4); // v1, v2, v3, v4 (v5 active)
        assert!(store.has_version(1));
        assert!(store.has_version(5)); // active
        assert!(!store.has_version(99));
    }

    // 10. SetCodeHandler
    #[test]
    fn test_set_code_handler() {
        let store = Arc::new(RuntimeCodeStore::new());
        store.initialize(valid_wasm(0), 1).unwrap();

        let handler = SetCodeHandler::new(Arc::clone(&store));
        let ext = make_extrinsic(valid_wasm(2), 2, gov_addr());
        let new_v = handler.handle(ext, 50).unwrap();
        assert_eq!(new_v.spec_version, 2);
        assert_eq!(handler.active_version().unwrap().spec_version, 2);
    }

    // 11. Çoklu upgrade, history depth
    #[test]
    fn test_history_depth_capping() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0), 1).unwrap();

        for v in 2..=(RUNTIME_HISTORY_DEPTH as u32 + 5) {
            let ext = make_extrinsic(valid_wasm(v as u8 % 200), v, gov_addr());
            store.apply_set_code(ext, v as u64 * 10).unwrap();
        }

        assert!(store.history().len() <= RUNTIME_HISTORY_DEPTH);
    }

    // 12. Code hash değişiyor
    #[test]
    fn test_code_hash_changes_on_upgrade() {
        let store = RuntimeCodeStore::new();
        store.initialize(valid_wasm(0xAA), 1).unwrap();
        let hash1 = store.active_version().unwrap().code_hash;

        let ext = make_extrinsic(valid_wasm(0xBB), 2, gov_addr());
        store.apply_set_code(ext, 10).unwrap();
        let hash2 = store.active_version().unwrap().code_hash;

        assert_ne!(hash1, hash2);
    }

    // 13. active_code doğru kodu döndürür
    #[test]
    fn test_active_code_returns_latest() {
        let store = RuntimeCodeStore::new();
        let code_v1 = valid_wasm(0x01);
        store.initialize(code_v1.clone(), 1).unwrap();

        let code_v2 = valid_wasm(0x02);
        let ext = make_extrinsic(code_v2.clone(), 2, gov_addr());
        store.apply_set_code(ext, 10).unwrap();

        assert_eq!(store.active_code().unwrap(), code_v2);
    }
}
