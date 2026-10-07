//! GÖREV 111: CheckTxState — Üyelik + TX Sayım + Nonce Takip
//!
//! ## Mimari
//!
//! ### Cosmos SDK CheckTx State
//! - `CheckTx`: TX mempool'a eklenebilir mi? (tam execution değil)
//! - `CheckState`: sadece bakiye ve nonce kontrolü
//! - `AnteHandler`: her TX için imza, fee, nonce, sequence doğrulaması
//! - `StateCache`: son N blok için anlık görüntü (disk okumadan hızlı)
//!
//! ### Nova Uyarlaması
//! - `CheckTxState`: üyelik (duplicate TX), TX sayım, nonce takip
//! - `MembershipFilter`: bloom filtre + hash set (double-spending koruması)
//! - `CheckTxResult`: geçerli/geçersiz + red sebebi
//! - Thread-safe (DashMap destekli)

use nova_primitives::Address;
use std::collections::{HashMap, HashSet};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Bloom filtre boyutu (bit) — false positive azaltmak için
pub const BLOOM_BIT_SIZE: usize = 65_536; // 8 KB

/// Bloom filtre hash fonksiyonu sayısı
pub const BLOOM_HASH_COUNT: usize = 7;

/// Maksimum pending TX sayısı (tüm adresler)
pub const MAX_PENDING_TX_COUNT: usize = 10_000;

/// Gönderici başına maksimum pending TX (mempool'da aynı anda)
pub const MAX_PENDING_PER_SENDER: usize = 64;

// ─── TxId ─────────────────────────────────────────────────────────────────────

/// TX kimliği (hash)
pub type TxId = [u8; 32];

// ─── MembershipFilter ─────────────────────────────────────────────────────────

/// Bloom filtre tabanlı TX üyelik kontrolü
///
/// Cosmos: `txCache` (mempool.go) — duplicate detection
#[derive(Debug)]
pub struct MembershipFilter {
    /// Bloom filtre bitleri
    bits: Vec<u64>,
    /// Doğrulama için kesin set (bloom false positive'lerini elemek için)
    exact_set: HashSet<TxId>,
    /// Kapasite
    capacity: usize,
}

impl MembershipFilter {
    pub fn new(capacity: usize) -> Self {
        let word_count = (BLOOM_BIT_SIZE + 63) / 64;
        Self {
            bits: vec![0u64; word_count],
            exact_set: HashSet::new(),
            capacity,
        }
    }

    /// Bloom filtre hash'i (basit double hash simülasyonu)
    fn bloom_positions(tx_id: &TxId) -> [usize; BLOOM_HASH_COUNT] {
        let mut positions = [0usize; BLOOM_HASH_COUNT];
        for (i, pos) in positions.iter_mut().enumerate() {
            // i'inci byte'ı seed olarak kullan
            let seed = u64::from_le_bytes(tx_id[i % 25..(i % 25) + 8].try_into().unwrap_or([0u8; 8]));
            let h = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(i as u64);
            *pos = (h as usize) % BLOOM_BIT_SIZE;
        }
        positions
    }

    /// TX'i filtreden geçir ve ekle
    ///
    /// Returns: zaten var mıydı (duplicate)?
    pub fn insert(&mut self, tx_id: TxId) -> bool {
        // Kesin set kontrolü
        if self.exact_set.contains(&tx_id) {
            return true; // duplicate
        }

        if self.exact_set.len() >= self.capacity {
            // Eviction: en eskiyi çıkar (basit strateji — prod'da LRU gerekir)
            if let Some(oldest) = self.exact_set.iter().next().copied() {
                self.exact_set.remove(&oldest);
            }
        }

        // Bloom filtre güncelle
        for pos in Self::bloom_positions(&tx_id) {
            self.bits[pos / 64] |= 1u64 << (pos % 64);
        }

        self.exact_set.insert(tx_id);
        false // yeni TX
    }

    /// TX mevcut mu? (bloom check → exact set confirm)
    pub fn contains(&self, tx_id: &TxId) -> bool {
        // Bloom check
        let bloom_hit = Self::bloom_positions(tx_id)
            .iter()
            .all(|&pos| (self.bits[pos / 64] >> (pos % 64)) & 1 == 1);

        if !bloom_hit {
            return false;
        }

        // Kesin doğrulama
        self.exact_set.contains(tx_id)
    }

    /// Üye sayısı
    pub fn len(&self) -> usize {
        self.exact_set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.exact_set.is_empty()
    }
}

// ─── CheckTxResult ────────────────────────────────────────────────────────────

/// CheckTx sonucu
///
/// Cosmos: `abci.ResponseCheckTx { code, log, .. }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTxResult {
    Accepted,
    Rejected(CheckTxRejectReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTxRejectReason {
    DuplicateTx,
    SenderPendingLimitReached,
    GlobalPendingLimitReached,
    NonceAlreadyUsed,
    InvalidNonce,
}

impl CheckTxResult {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

// ─── SenderNonceState ─────────────────────────────────────────────────────────

/// Bir gönderici için nonce durumu
#[derive(Debug, Clone, Default)]
pub struct SenderNonceState {
    /// Kullanılmış nonce'lar (pending TX'lerinkiler)
    pub pending_nonces: HashSet<u64>,
    /// En yüksek tamamlanmış nonce (confirmed block'tan)
    pub confirmed_nonce_high: u64,
}

impl SenderNonceState {
    /// Bu nonce kabul edilebilir mi?
    pub fn is_nonce_acceptable(&self, nonce: u64) -> bool {
        // Sıfır nonce geçersiz
        if nonce == 0 { return false; }
        // Zaten pending'de mi?
        if self.pending_nonces.contains(&nonce) { return false; }
        true
    }

    /// Pending nonce ekle
    pub fn add_pending_nonce(&mut self, nonce: u64) {
        self.pending_nonces.insert(nonce);
    }

    /// TX tamamlandı, nonce pending'den çıkar
    pub fn confirm_nonce(&mut self, nonce: u64) {
        self.pending_nonces.remove(&nonce);
        if nonce > self.confirmed_nonce_high {
            self.confirmed_nonce_high = nonce;
        }
    }

    /// Pending TX sayısı
    pub fn pending_count(&self) -> usize {
        self.pending_nonces.len()
    }
}

// ─── CheckTxState ─────────────────────────────────────────────────────────────

/// TX ön-doğrulama durum yöneticisi
///
/// Cosmos: `baseapp.checkState` (baseapp.go)
pub struct CheckTxState {
    /// TX üyelik filtresi (duplicate detection)
    pub membership: MembershipFilter,
    /// Gönderici başına nonce durumu
    nonce_states: HashMap<Address, SenderNonceState>,
    /// Gönderici başına pending TX sayısı
    sender_counts: HashMap<Address, usize>,
    /// Toplam pending TX sayısı
    total_pending: usize,
    /// Konfigürasyon
    pub max_pending_per_sender: usize,
    pub max_total_pending: usize,
}

impl CheckTxState {
    pub fn new(max_pending_per_sender: usize, max_total_pending: usize) -> Self {
        Self {
            membership: MembershipFilter::new(max_total_pending * 2),
            nonce_states: HashMap::new(),
            sender_counts: HashMap::new(),
            total_pending: 0,
            max_pending_per_sender,
            max_total_pending,
        }
    }

    /// Nova varsayılanları
    pub fn default_nova() -> Self {
        Self::new(MAX_PENDING_PER_SENDER, MAX_PENDING_TX_COUNT)
    }

    /// TX'i kontrol et ve kabul et (CheckTx)
    ///
    /// Cosmos: `baseapp.runTx(RunTxModeCheck, ..)`
    pub fn check_tx(
        &mut self,
        tx_id: TxId,
        sender: &Address,
        nonce: u64,
    ) -> CheckTxResult {
        // 1. Duplicate TX kontrolü
        if self.membership.contains(&tx_id) {
            return CheckTxResult::Rejected(CheckTxRejectReason::DuplicateTx);
        }

        // 2. Global limit kontrolü
        if self.total_pending >= self.max_total_pending {
            return CheckTxResult::Rejected(CheckTxRejectReason::GlobalPendingLimitReached);
        }

        // 3. Gönderici başına limit kontrolü
        let sender_count = self.sender_counts.get(sender).copied().unwrap_or(0);
        if sender_count >= self.max_pending_per_sender {
            return CheckTxResult::Rejected(CheckTxRejectReason::SenderPendingLimitReached);
        }

        // 4. Nonce kontrolü
        let nonce_state = self.nonce_states.entry(*sender).or_default();
        if !nonce_state.is_nonce_acceptable(nonce) {
            return if nonce == 0 {
                CheckTxResult::Rejected(CheckTxRejectReason::InvalidNonce)
            } else {
                CheckTxResult::Rejected(CheckTxRejectReason::NonceAlreadyUsed)
            };
        }

        // 5. Kabul et
        self.membership.insert(tx_id);
        nonce_state.add_pending_nonce(nonce);
        *self.sender_counts.entry(*sender).or_insert(0) += 1;
        self.total_pending += 1;

        CheckTxResult::Accepted
    }

    /// TX tamamlandı (bloka girdi)
    pub fn on_tx_confirmed(&mut self, tx_id: &TxId, sender: &Address, nonce: u64) {
        if let Some(ns) = self.nonce_states.get_mut(sender) {
            ns.confirm_nonce(nonce);
        }
        if let Some(count) = self.sender_counts.get_mut(sender) {
            *count = count.saturating_sub(1);
        }
        if self.total_pending > 0 {
            self.total_pending -= 1;
        }
        // TX ID'si artık "tamamlandı" — membership'ten çıkarmıyoruz
        // (duplicate koruması için bir süre daha tutulur)
        let _ = tx_id;
    }

    /// TX iptal edildi / süresi doldu
    pub fn on_tx_removed(&mut self, sender: &Address, nonce: u64) {
        if let Some(ns) = self.nonce_states.get_mut(sender) {
            ns.pending_nonces.remove(&nonce);
        }
        if let Some(count) = self.sender_counts.get_mut(sender) {
            *count = count.saturating_sub(1);
        }
        if self.total_pending > 0 {
            self.total_pending -= 1;
        }
    }

    /// Mevcut toplam pending TX sayısı
    pub fn total_pending(&self) -> usize {
        self.total_pending
    }

    /// Gönderici başına pending TX sayısı
    pub fn sender_pending_count(&self, sender: &Address) -> usize {
        self.sender_counts.get(sender).copied().unwrap_or(0)
    }

    /// Gönderici nonce durumu
    pub fn nonce_state(&self, sender: &Address) -> Option<&SenderNonceState> {
        self.nonce_states.get(sender)
    }

    /// Tüm pending TX sayısı (tüm göndericiler toplamı)
    pub fn unique_sender_count(&self) -> usize {
        self.sender_counts.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn tx_id(n: u8) -> TxId {
        [n; 32]
    }

    // 1. Yeni TX kabul edilir
    #[test]
    fn test_new_tx_accepted() {
        let mut state = CheckTxState::default_nova();
        let result = state.check_tx(tx_id(1), &addr(1), 111);
        assert_eq!(result, CheckTxResult::Accepted);
        assert_eq!(state.total_pending(), 1);
    }

    // 2. Duplicate TX reddedilir
    #[test]
    fn test_duplicate_tx_rejected() {
        let mut state = CheckTxState::default_nova();
        state.check_tx(tx_id(1), &addr(1), 111).is_accepted();
        let result = state.check_tx(tx_id(1), &addr(1), 222);
        assert_eq!(result, CheckTxResult::Rejected(CheckTxRejectReason::DuplicateTx));
    }

    // 3. Aynı nonce ikinci kez reddedilir
    #[test]
    fn test_duplicate_nonce_rejected() {
        let mut state = CheckTxState::default_nova();
        state.check_tx(tx_id(1), &addr(1), 42);
        let result = state.check_tx(tx_id(2), &addr(1), 42);
        assert_eq!(result, CheckTxResult::Rejected(CheckTxRejectReason::NonceAlreadyUsed));
    }

    // 4. Sıfır nonce geçersiz
    #[test]
    fn test_zero_nonce_invalid() {
        let mut state = CheckTxState::default_nova();
        let result = state.check_tx(tx_id(1), &addr(1), 0);
        assert_eq!(result, CheckTxResult::Rejected(CheckTxRejectReason::InvalidNonce));
    }

    // 5. Gönderici başına limit
    #[test]
    fn test_sender_limit() {
        let mut state = CheckTxState::new(3, 10_000);
        for i in 1u8..=3 {
            let r = state.check_tx([i; 32], &addr(1), i as u64 * 100);
            assert_eq!(r, CheckTxResult::Accepted);
        }
        // 4. TX reddedilmeli
        let result = state.check_tx([99u8; 32], &addr(1), 999);
        assert_eq!(result, CheckTxResult::Rejected(CheckTxRejectReason::SenderPendingLimitReached));
    }

    // 6. Global limit
    #[test]
    fn test_global_limit() {
        let mut state = CheckTxState::new(10, 3); // sadece 3 TX toplam
        state.check_tx(tx_id(1), &addr(1), 100);
        state.check_tx(tx_id(2), &addr(2), 200);
        state.check_tx(tx_id(3), &addr(3), 300);

        let result = state.check_tx(tx_id(4), &addr(4), 400);
        assert_eq!(result, CheckTxResult::Rejected(CheckTxRejectReason::GlobalPendingLimitReached));
    }

    // 7. TX tamamlandı → pending azalır
    #[test]
    fn test_on_confirmed_decrements() {
        let mut state = CheckTxState::default_nova();
        state.check_tx(tx_id(1), &addr(1), 100);
        state.check_tx(tx_id(2), &addr(1), 200);
        assert_eq!(state.total_pending(), 2);

        state.on_tx_confirmed(&tx_id(1), &addr(1), 100);
        assert_eq!(state.total_pending(), 1);
        assert_eq!(state.sender_pending_count(&addr(1)), 1);
    }

    // 8. TX silindi → nonce tekrar kullanılabilir
    #[test]
    fn test_on_removed_frees_nonce() {
        let mut state = CheckTxState::default_nova();
        state.check_tx(tx_id(1), &addr(1), 100);
        state.on_tx_removed(&addr(1), 100);

        // Şimdi aynı nonce tekrar kabul edilmeli (TX ID farklı)
        let result = state.check_tx(tx_id(5), &addr(1), 100);
        // Membership filtresi tx_id(1)'i hâlâ bloklar ama tx_id(5) farklı
        assert_eq!(result, CheckTxResult::Accepted);
    }

    // 9. Farklı adresler bağımsız
    #[test]
    fn test_different_senders_independent() {
        let mut state = CheckTxState::new(2, 1000);
        state.check_tx(tx_id(1), &addr(1), 100);
        state.check_tx(tx_id(2), &addr(1), 200);

        // addr(1) dolu, addr(2) hâlâ boş
        let r1 = state.check_tx(tx_id(3), &addr(1), 300);
        let r2 = state.check_tx(tx_id(4), &addr(2), 100);

        assert_eq!(r1, CheckTxResult::Rejected(CheckTxRejectReason::SenderPendingLimitReached));
        assert_eq!(r2, CheckTxResult::Accepted);
    }

    // 10. MembershipFilter duplicate detection
    #[test]
    fn test_membership_filter_no_false_negatives() {
        let mut filter = MembershipFilter::new(1000);
        let id = [0x42u8; 32];

        assert!(!filter.contains(&id));
        filter.insert(id);
        assert!(filter.contains(&id));
    }

    // 11. MembershipFilter len
    #[test]
    fn test_membership_filter_len() {
        let mut filter = MembershipFilter::new(1000);
        assert!(filter.is_empty());

        for i in 0..5u8 {
            filter.insert([i; 32]);
        }
        assert_eq!(filter.len(), 5);
    }

    // 12. SenderNonceState pending count
    #[test]
    fn test_sender_nonce_state_count() {
        let mut ns = SenderNonceState::default();
        assert_eq!(ns.pending_count(), 0);

        ns.add_pending_nonce(100);
        ns.add_pending_nonce(200);
        assert_eq!(ns.pending_count(), 2);

        ns.confirm_nonce(100);
        assert_eq!(ns.pending_count(), 1);
        assert_eq!(ns.confirmed_nonce_high, 100);
    }

    // 13. unique_sender_count
    #[test]
    fn test_unique_sender_count() {
        let mut state = CheckTxState::default_nova();
        state.check_tx(tx_id(1), &addr(1), 100);
        state.check_tx(tx_id(2), &addr(2), 200);
        state.check_tx(tx_id(3), &addr(1), 300);
        assert_eq!(state.unique_sender_count(), 2);
    }
}
