//! Mock depolama — key-value store simülasyonu

use dashmap::DashMap;
use std::sync::Arc;

/// Mock anahtar-değer depolama
#[derive(Clone)]
pub struct MockStorage {
    data: Arc<DashMap<Vec<u8>, Vec<u8>>>,
    read_count: Arc<parking_lot::RwLock<u64>>,
    write_count: Arc<parking_lot::RwLock<u64>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
            read_count: Arc::new(parking_lot::RwLock::new(0)),
            write_count: Arc::new(parking_lot::RwLock::new(0)),
        }
    }

    /// Değer yaz
    pub fn put(&self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.data.insert(key.into(), value.into());
        *self.write_count.write() += 1;
    }

    /// Değer oku
    pub fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        *self.read_count.write() += 1;
        self.data.get(key.as_ref()).map(|v| v.clone())
    }

    /// Değer sil
    pub fn delete(&self, key: impl AsRef<[u8]>) -> bool {
        self.data.remove(key.as_ref()).is_some()
    }

    /// Anahtar var mı?
    pub fn contains(&self, key: impl AsRef<[u8]>) -> bool {
        self.data.contains_key(key.as_ref())
    }

    /// Prefix ile tüm anahtarları getir
    pub fn keys_with_prefix(&self, prefix: impl AsRef<[u8]>) -> Vec<Vec<u8>> {
        let prefix = prefix.as_ref();
        self.data.iter()
            .filter(|e| e.key().starts_with(prefix))
            .map(|e| e.key().clone())
            .collect()
    }

    pub fn len(&self) -> usize { self.data.len() }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
    pub fn read_count(&self) -> u64 { *self.read_count.read() }
    pub fn write_count(&self) -> u64 { *self.write_count.read() }

    /// Tümünü temizle
    pub fn clear(&self) {
        self.data.clear();
    }
}

/// Typed wrapper — belirli namespace için
pub struct NamespacedStorage {
    storage: MockStorage,
    namespace: Vec<u8>,
}

impl NamespacedStorage {
    pub fn new(storage: MockStorage, namespace: impl Into<Vec<u8>>) -> Self {
        Self { storage, namespace: namespace.into() }
    }

    fn full_key(&self, key: impl AsRef<[u8]>) -> Vec<u8> {
        let mut full = self.namespace.clone();
        full.push(b'/');
        full.extend_from_slice(key.as_ref());
        full
    }

    pub fn put(&self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        let k = self.full_key(key.into());
        self.storage.put(k, value);
    }

    pub fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        let k = self.full_key(key);
        self.storage.get(&k)
    }

    pub fn delete(&self, key: impl AsRef<[u8]>) -> bool {
        let k = self.full_key(key);
        self.storage.delete(&k)
    }

    pub fn all_keys(&self) -> Vec<Vec<u8>> {
        self.storage.keys_with_prefix(&self.namespace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_put_get() {
        let s = MockStorage::new();
        s.put("key1", "value1");
        assert_eq!(s.get("key1"), Some(b"value1".to_vec()));
    }

    #[test]
    fn test_storage_not_found() {
        let s = MockStorage::new();
        assert_eq!(s.get("nonexistent"), None);
    }

    #[test]
    fn test_storage_delete() {
        let s = MockStorage::new();
        s.put("k", "v");
        assert!(s.delete("k"));
        assert!(!s.contains("k"));
        assert!(!s.delete("k")); // zaten yok
    }

    #[test]
    fn test_storage_prefix_search() {
        let s = MockStorage::new();
        s.put("block/1", "data1");
        s.put("block/2", "data2");
        s.put("tx/1", "txdata");

        let block_keys = s.keys_with_prefix("block/");
        assert_eq!(block_keys.len(), 2);
    }

    #[test]
    fn test_storage_counters() {
        let s = MockStorage::new();
        s.put("k1", "v1");
        s.put("k2", "v2");
        s.get("k1");
        s.get("k2");
        s.get("k3");

        assert_eq!(s.write_count(), 2);
        assert_eq!(s.read_count(), 3);
    }

    #[test]
    fn test_storage_clear() {
        let s = MockStorage::new();
        s.put("a", "1");
        s.put("b", "2");
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn test_namespaced_storage() {
        let base = MockStorage::new();
        let ns = NamespacedStorage::new(base.clone(), "blocks");

        ns.put("1", "block_data_1");
        ns.put("2", "block_data_2");

        assert_eq!(ns.get("1"), Some(b"block_data_1".to_vec()));
        assert_eq!(ns.all_keys().len(), 2);

        // Doğrudan erişimde namespace prefix'i var
        assert!(base.contains("blocks/1"));
    }

    #[test]
    fn test_namespaced_delete() {
        let base = MockStorage::new();
        let ns = NamespacedStorage::new(base, "tx");
        ns.put("hash1", "tx_data");
        assert!(ns.delete("hash1"));
        assert_eq!(ns.get("hash1"), None);
    }
}
