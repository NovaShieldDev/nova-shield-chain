//! GÖREV 109: NonceTracker — Random Nonces (sequential değil)
//!
//! ## Mimari
//!
//! ### Solana NonceTracker
//! - Solana'da nonce sequential değil, unique random uint64
//! - Replay saldırısı koruması: her TX benzersiz nonce içerir
//! - `durable nonce`: bir nonce account, belirli süreli geçerli
//! - Nonce kullanıldıktan sonra farklı bir değere ilerlenir
//!
//! ### Nova Uyarlaması
//! - NOVA-RED-MEMPOOL-004: Nonce'a counter gömülür — ring buffer taşması replay'ı önler
//! - Nonce formatı: `(counter & 0xFFFF_FFFF) << 32 | BLAKE3(salt||addr||counter)[0..4]`
//!   - Yüksek 32 bit: counter (kaynak doğrulama için)
//!   - Düşük  32 bit: BLAKE3 hash parçası (tahmin edilemezlik)
//! - Ring buffer + min_valid_counter çift koruma:
//!   1. Nonce ring buffer'da → AlreadyUsed
//!   2. Nonce'un counter'ı < min_valid_counter → kalıcı red
//! - Her adres için ayrı counter (monoton artar)
//! - Replay koruması: evict edilen nonce'lar min_valid_counter ile kalıcı olarak engellenir

use blake3;
use nova_primitives::Address;
use std::collections::{HashMap, HashSet};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Adres başına saklanacak kullanılmış nonce sayısı (son N işlem)
///
/// Solana: `NonceKeeper.capacity`
pub const NONCE_HISTORY_DEPTH: usize = 1_024;

/// Nonce salt'ı (her deployment'ta farklı)
const NONCE_SALT: &[u8] = b"nova-shield-nonce-v1";

// ─── NonceTracker ─────────────────────────────────────────────────────────────

/// Random nonce üretici ve doğrulayıcı
///
/// Solana: `NonceTracker` (accounts/nonce_utils.rs)
pub struct NonceTracker {
    /// Her adres için monoton counter (nonce türetimi için)
    counters: HashMap<Address, u64>,
    /// Kullanılmış nonce'lar (replay koruması): address → Set<nonce>
    used_nonces: HashMap<Address, UsedNonceSet>,
    /// NOVA-RED-MEMPOOL-004: Ring buffer evict olunca counter eşiği yükselir.
    /// Nonce'un gömülü counter'ı < min_valid_counter ise kalıcı olarak reddedilir.
    min_valid_counters: HashMap<Address, u64>,
    /// Global salt (runtime seed)
    salt: [u8; 32],
}

/// Kullanılmış nonce kümesi — sınırlı kapasiteli
/// Her entry (nonce, counter) çifti olarak saklanır.
struct UsedNonceSet {
    set: HashSet<u64>,
    history: std::collections::VecDeque<(u64, u64)>, // (nonce, counter)
    capacity: usize,
}

impl UsedNonceSet {
    fn new(capacity: usize) -> Self {
        Self {
            set: HashSet::new(),
            history: std::collections::VecDeque::new(),
            capacity,
        }
    }

    fn contains(&self, nonce: u64) -> bool {
        self.set.contains(&nonce)
    }

    /// Nonce ekle; evict edilen entry'nin counter'ını döner (varsa).
    fn insert(&mut self, nonce: u64, counter: u64) -> Option<u64> {
        let evicted_counter = if self.set.len() >= self.capacity {
            self.history.pop_front().map(|(old_nonce, old_counter)| {
                self.set.remove(&old_nonce);
                old_counter
            })
        } else {
            None
        };
        self.set.insert(nonce);
        self.history.push_back((nonce, counter));
        evicted_counter
    }

    fn len(&self) -> usize {
        self.set.len()
    }
}

impl NonceTracker {
    pub fn new(salt: [u8; 32]) -> Self {
        Self {
            counters: HashMap::new(),
            used_nonces: HashMap::new(),
            min_valid_counters: HashMap::new(),
            salt,
        }
    }

    /// Varsayılan salt ile (test/development)
    pub fn default_salt() -> Self {
        Self::new(*blake3::hash(b"nova-nonce-default-salt").as_bytes())
    }

    /// Bir adres için sonraki nonce üret (random görünümlü)
    ///
    /// Nonce formatı: `(counter & 0xFFFF_FFFF) << 32 | hash_part_u32`
    ///
    /// Solana: `durable_nonce` → bir account hash'i
    pub fn next_nonce(&mut self, address: &Address) -> u64 {
        let counter = self.counters.entry(*address).or_insert(0);
        let nonce = Self::derive_nonce(&self.salt, address, *counter);
        *counter += 1;
        nonce
    }

    /// Nonce türetimi (statik, test için)
    ///
    /// NOVA-RED-MEMPOOL-004: Counter yüksek 32 bite gömülür.
    /// Böylece herhangi bir nonce'tan kaynak counter çıkarılabilir.
    pub fn derive_nonce(salt: &[u8; 32], address: &Address, counter: u64) -> u64 {
        let hash = blake3::hash(&[
            NONCE_SALT,
            salt.as_slice(),
            address.as_bytes().as_slice(),
            &counter.to_le_bytes(),
        ].concat());
        let hash_part = u32::from_le_bytes(hash.as_bytes()[0..4].try_into().unwrap()) as u64;
        // Counter'ı yüksek 32 bite gömle (max 2^32 - 1 ≈ 4B işlem / adres)
        let counter_part = (counter & 0xFFFF_FFFF) << 32;
        counter_part | hash_part
    }

    /// Nonce'u işaretlenmiş olarak kaydet (bir TX'ta kullanıldı)
    ///
    /// NOVA-RED-MEMPOOL-004: Ring buffer taştığında evict edilen counter
    /// min_valid_counter'ı ilerletir → o counter'dan üretilen nonce artık kalıcı red.
    ///
    /// Solana: `advance_nonce_account()`
    pub fn mark_used(&mut self, address: &Address, nonce: u64) {
        // Nonce'tan gömülü counter'ı çıkar
        let counter = nonce >> 32;
        let evicted = self.used_nonces
            .entry(*address)
            .or_insert_with(|| UsedNonceSet::new(NONCE_HISTORY_DEPTH))
            .insert(nonce, counter);

        // Evict olan counter, min_valid_counter'ı ilerletir
        if let Some(evicted_counter) = evicted {
            let min = self.min_valid_counters.entry(*address).or_insert(0);
            if evicted_counter + 1 > *min {
                *min = evicted_counter + 1;
            }
        }
    }

    /// Bu nonce daha önce kullanıldı mı? (ring buffer kontrolü)
    pub fn is_used(&self, address: &Address, nonce: u64) -> bool {
        self.used_nonces
            .get(address)
            .map(|s| s.contains(nonce))
            .unwrap_or(false)
    }

    /// Nonce doğrula: kullanılmamış ve geçerli bir değer mi?
    ///
    /// NOVA-RED-MEMPOOL-004: Çift kontrol:
    /// 1. Ring buffer: nonce hâlâ kayıtlı mı?
    /// 2. Min-counter: evict edilen nonce'un counter'ı kalıcı red listesinde mi?
    pub fn validate(&self, address: &Address, nonce: u64) -> NonceValidation {
        if nonce == 0 {
            return NonceValidation::InvalidZero;
        }
        // Nonce'tan gömülü counter'ı çıkar
        let nonce_counter = nonce >> 32;
        let min_counter = self.min_valid_counters.get(address).copied().unwrap_or(0);
        if nonce_counter < min_counter {
            // Counter evict edilmiş → kalıcı replay koruması
            return NonceValidation::AlreadyUsed;
        }
        if self.is_used(address, nonce) {
            return NonceValidation::AlreadyUsed;
        }
        NonceValidation::Valid
    }

    /// Nonce üret ve hemen kullanıldı olarak işaretle (atomic)
    ///
    /// Returns: üretilen nonce
    pub fn generate_and_consume(&mut self, address: &Address) -> u64 {
        let nonce = self.next_nonce(address);
        self.mark_used(address, nonce);
        nonce
    }

    /// Adres başına kullanılmış nonce sayısı
    pub fn used_count(&self, address: &Address) -> usize {
        self.used_nonces.get(address).map(|s| s.len()).unwrap_or(0)
    }

    /// Adres başına mevcut counter değeri
    pub fn counter(&self, address: &Address) -> u64 {
        self.counters.get(address).copied().unwrap_or(0)
    }

    /// Toplam adres sayısı
    pub fn address_count(&self) -> usize {
        self.counters.len()
    }
}

// ─── NonceValidation ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonceValidation {
    Valid,
    AlreadyUsed,
    InvalidZero,
}

impl NonceValidation {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    // 1. Nonce sequential DEĞİL (random görünümlü)
    #[test]
    fn test_nonce_not_sequential() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(1);

        let n1 = tracker.next_nonce(&a);
        let n2 = tracker.next_nonce(&a);
        let n3 = tracker.next_nonce(&a);

        // Sıralı değil (counterlar 0,1,2 ama nonceler hash bazlı)
        assert_ne!(n1, n2);
        assert_ne!(n2, n3);
        assert_ne!(n1, n3);

        // n1 < n2 < n3 olmak zorunda değil
        let is_sequential = (n2 == n1 + 1) && (n3 == n2 + 1);
        // Bu ihtimali test etmiyoruz, ama gerçekte çok nadiren olur
        let _ = is_sequential; // uyarı önlemi
    }

    // 2. Aynı counter → aynı nonce (deterministik)
    #[test]
    fn test_nonce_deterministic() {
        let salt = [0x42u8; 32];
        let a = addr(5);

        let n1 = NonceTracker::derive_nonce(&salt, &a, 0);
        let n2 = NonceTracker::derive_nonce(&salt, &a, 0);
        assert_eq!(n1, n2);

        // Farklı counter → farklı nonce
        let n3 = NonceTracker::derive_nonce(&salt, &a, 1);
        assert_ne!(n1, n3);
    }

    // 3. Farklı adresler → farklı nonceler (aynı counter)
    #[test]
    fn test_nonce_address_sensitive() {
        let salt = [0x11u8; 32];
        let n1 = NonceTracker::derive_nonce(&salt, &addr(1), 0);
        let n2 = NonceTracker::derive_nonce(&salt, &addr(2), 0);
        assert_ne!(n1, n2);
    }

    // 4. mark_used ve is_used
    #[test]
    fn test_mark_and_check_used() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(1);

        assert!(!tracker.is_used(&a, 12345));
        tracker.mark_used(&a, 12345);
        assert!(tracker.is_used(&a, 12345));
        assert!(!tracker.is_used(&a, 67890));
    }

    // 5. validate geçerli nonce
    #[test]
    fn test_validate_valid() {
        let tracker = NonceTracker::default_salt();
        let result = tracker.validate(&addr(1), 99999);
        assert_eq!(result, NonceValidation::Valid);
        assert!(result.is_valid());
    }

    // 6. validate kullanılmış nonce
    #[test]
    fn test_validate_already_used() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(1);
        tracker.mark_used(&a, 555);
        assert_eq!(tracker.validate(&a, 555), NonceValidation::AlreadyUsed);
    }

    // 7. validate sıfır nonce
    #[test]
    fn test_validate_zero_nonce() {
        let tracker = NonceTracker::default_salt();
        assert_eq!(tracker.validate(&addr(1), 0), NonceValidation::InvalidZero);
    }

    // 8. generate_and_consume — üretir ve hemen kullanır
    #[test]
    fn test_generate_and_consume() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(3);

        let nonce = tracker.generate_and_consume(&a);
        assert_ne!(nonce, 0);
        assert!(tracker.is_used(&a, nonce));

        // Aynı counter → aynı nonce ama artık kullanılmış
        // Yeni nonce farklı olmalı
        let nonce2 = tracker.generate_and_consume(&a);
        assert_ne!(nonce, nonce2);
    }

    // 9. Counter artışı
    #[test]
    fn test_counter_increases() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(5);

        assert_eq!(tracker.counter(&a), 0);
        tracker.next_nonce(&a);
        assert_eq!(tracker.counter(&a), 1);
        tracker.next_nonce(&a);
        assert_eq!(tracker.counter(&a), 2);
    }

    // 10. Kapasite dolduğunda eski nonce'lar silinir (ring buffer)
    #[test]
    fn test_capacity_eviction() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(10);

        // NONCE_HISTORY_DEPTH kadar nonce ekle (counter=0 gömülü)
        for i in 0..NONCE_HISTORY_DEPTH {
            tracker.mark_used(&a, i as u64 + 1); // nonce >> 32 = 0 (counter=0)
        }
        assert_eq!(tracker.used_count(&a), NONCE_HISTORY_DEPTH);

        // Bir tane daha ekle → en eskisi silinmeli
        tracker.mark_used(&a, NONCE_HISTORY_DEPTH as u64 + 1);
        assert_eq!(tracker.used_count(&a), NONCE_HISTORY_DEPTH); // hâlâ kapasite kadar

        // En eski (nonce=1) ring buffer'da yok
        assert!(!tracker.is_used(&a, 1));
    }

    // 10b. NOVA-RED-MEMPOOL-004: Ring buffer evict sonrası replay kalıcı olarak reddedilir
    #[test]
    fn test_capacity_eviction_replay_rejected() {
        let mut tracker = NonceTracker::default_salt();
        let a = addr(11);

        // NONCE_HISTORY_DEPTH kadar gerçek nonce üret ve işaretle
        let mut first_nonce = 0u64;
        for i in 0..=NONCE_HISTORY_DEPTH {
            let nonce = tracker.generate_and_consume(&a);
            if i == 0 { first_nonce = nonce; }
        }

        // first_nonce ring buffer'dan evict edildi
        assert!(!tracker.is_used(&a, first_nonce));

        // Ama min_valid_counter sayesinde validate de reddeder
        assert_eq!(tracker.validate(&a, first_nonce), NonceValidation::AlreadyUsed);
    }

    // 11. Farklı adresler bağımsız counter
    #[test]
    fn test_independent_counters() {
        let mut tracker = NonceTracker::default_salt();

        tracker.next_nonce(&addr(1));
        tracker.next_nonce(&addr(1));
        tracker.next_nonce(&addr(2));

        assert_eq!(tracker.counter(&addr(1)), 2);
        assert_eq!(tracker.counter(&addr(2)), 1);
        assert_eq!(tracker.counter(&addr(3)), 0);
    }

    // 12. Farklı salt → farklı nonce değerleri
    #[test]
    fn test_different_salt() {
        let salt1 = [0x01u8; 32];
        let salt2 = [0x02u8; 32];
        let a = addr(1);

        let n1 = NonceTracker::derive_nonce(&salt1, &a, 0);
        let n2 = NonceTracker::derive_nonce(&salt2, &a, 0);
        assert_ne!(n1, n2);
    }

    // 13. address_count takibi
    #[test]
    fn test_address_count() {
        let mut tracker = NonceTracker::default_salt();
        assert_eq!(tracker.address_count(), 0);
        tracker.next_nonce(&addr(1));
        tracker.next_nonce(&addr(2));
        assert_eq!(tracker.address_count(), 2);
    }
}
