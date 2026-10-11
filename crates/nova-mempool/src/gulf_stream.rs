//! GÖREV 88: Gulf Stream TX Forward (Solana pattern)
//!
//! ## Mimari
//!
//! ### Solana Gulf Stream
//! - TX'ler validator'lara **slotdan önce** gönderilir
//! - Her validator kendi yerel mempool'unda TX'leri tutar
//! - Lider değişimine göre TX'ler bir sonraki lidere forward edilir
//! - Globalmatik TTL: TX belirli süre sonra geçersiz (MaxAge)
//!
//! ### Nova Uyarlaması
//! - Şifreli TX her zaman hedefe gönderilir
//! - `GulfStreamRouter`: gelecekteki slot liderini bilir, önceden forward eder
//! - `ForwardedTxCache`: her validator kendi gelen forwarded TX cache'i
//! - TX'ler `max_age_slots` sonunda drop edilir

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use parking_lot::Mutex;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// TX cache TTL (slot cinsinden)
pub const DEFAULT_MAX_AGE_SLOTS: u64 = 150;

/// Bir validator'ın cache edebileceği max forwarded TX sayısı
pub const MAX_FORWARDED_TX_PER_VALIDATOR: usize = 10_000;

/// Forward fanout: kaç validator'a forward edilir
pub const FORWARD_FANOUT: usize = 4;

// ─── ForwardedTx ──────────────────────────────────────────────────────────────

/// Forward edilmiş işlem
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedTx {
    /// TX hash (şifreli TX commitment hash'i)
    pub tx_hash: [u8; 32],
    /// Ham TX verisi (şifreli)
    pub data: Vec<u8>,
    /// TX'in geçerli olduğu ilk slot (gönderilmeden önce reddedilir)
    pub target_slot: u64,
    /// TX'in geçerliliğini yitireceği slot (MaxAge)
    pub expires_at_slot: u64,
    /// Gönderen validator ID
    pub from_validator: u64,
    /// TX önceliği (fee tabanlı)
    pub priority_fee: u128,
}

impl ForwardedTx {
    pub fn new(
        tx_hash: [u8; 32],
        data: Vec<u8>,
        target_slot: u64,
        from_validator: u64,
        priority_fee: u128,
        max_age_slots: u64,
    ) -> Self {
        Self {
            tx_hash,
            data,
            target_slot,
            expires_at_slot: target_slot + max_age_slots,
            from_validator,
            priority_fee,
        }
    }

    pub fn is_expired(&self, current_slot: u64) -> bool {
        current_slot > self.expires_at_slot
    }

    pub fn is_ready(&self, current_slot: u64) -> bool {
        current_slot >= self.target_slot && !self.is_expired(current_slot)
    }
}

// ─── LeaderSchedule ───────────────────────────────────────────────────────────

/// Lider çizelgesi — hangi slot'ta hangi validator lider
///
/// Solana pattern: `leader_schedule_cache`
/// Gerçek implementasyonda VRF + stake tabanlı; burada round-robin mock
#[derive(Debug, Clone)]
pub struct LeaderSchedule {
    /// slot → validator_id
    schedule: BTreeMap<u64, u64>,
    validator_count: u64,
}

impl LeaderSchedule {
    pub fn new(validator_count: u64) -> Self {
        Self {
            schedule: BTreeMap::new(),
            validator_count,
        }
    }

    /// Belirli slot için lideri bul (round-robin fallback)
    pub fn leader_for_slot(&self, slot: u64) -> u64 {
        if let Some(&leader) = self.schedule.get(&slot) {
            return leader;
        }
        // Round-robin fallback (validator_id 1'den başlar)
        (slot % self.validator_count) + 1
    }

    /// Slot aralığındaki liderleri listele
    pub fn leaders_in_range(&self, from_slot: u64, to_slot: u64) -> Vec<(u64, u64)> {
        (from_slot..=to_slot)
            .map(|slot| (slot, self.leader_for_slot(slot)))
            .collect()
    }

    /// Belirli validatorın lider olduğu ilk slotu bul (current_slot'tan itibaren)
    pub fn next_leader_slot(&self, validator_id: u64, from_slot: u64) -> u64 {
        for offset in 0..self.validator_count {
            let slot = from_slot + offset;
            if self.leader_for_slot(slot) == validator_id {
                return slot;
            }
        }
        from_slot + self.validator_count
    }

    /// Manuel slot→lider atama (test/genesis için)
    pub fn set_leader(&mut self, slot: u64, validator_id: u64) {
        self.schedule.insert(slot, validator_id);
    }
}

// ─── ForwardedTxCache ─────────────────────────────────────────────────────────

/// Bir validatörün gelen forwarded TX cache'i
#[derive(Debug, Default)]
pub struct ForwardedTxCache {
    /// tx_hash → ForwardedTx
    txs: HashMap<[u8; 32], ForwardedTx>,
    /// Arrival sırasını takip et (FIFO drop için)
    arrival_order: VecDeque<[u8; 32]>,
}

impl ForwardedTxCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// TX ekle
    pub fn insert(&mut self, tx: ForwardedTx) -> bool {
        if self.txs.contains_key(&tx.tx_hash) {
            return false; // duplicate
        }

        // Kapasite kontrolü
        while self.txs.len() >= MAX_FORWARDED_TX_PER_VALIDATOR {
            if let Some(oldest) = self.arrival_order.pop_front() {
                self.txs.remove(&oldest);
            } else {
                break;
            }
        }

        self.arrival_order.push_back(tx.tx_hash);
        self.txs.insert(tx.tx_hash, tx);
        true
    }

    /// Belirli slot için hazır TX'leri al (süresi dolmayanlar, hedef slota ulaşanlar)
    pub fn drain_ready(&mut self, current_slot: u64) -> Vec<ForwardedTx> {
        let ready_hashes: Vec<[u8; 32]> = self.txs.values()
            .filter(|tx| tx.is_ready(current_slot))
            .map(|tx| tx.tx_hash)
            .collect();

        let mut result = Vec::new();
        for hash in ready_hashes {
            if let Some(tx) = self.txs.remove(&hash) {
                self.arrival_order.retain(|h| h != &hash);
                result.push(tx);
            }
        }

        // Önceliğe göre sırala (yüksek fee önce)
        result.sort_by(|a, b| b.priority_fee.cmp(&a.priority_fee));
        result
    }

    /// Süresi dolan TX'leri temizle
    pub fn prune_expired(&mut self, current_slot: u64) -> usize {
        let expired: Vec<[u8; 32]> = self.txs.values()
            .filter(|tx| tx.is_expired(current_slot))
            .map(|tx| tx.tx_hash)
            .collect();

        let count = expired.len();
        for hash in expired {
            self.txs.remove(&hash);
            self.arrival_order.retain(|h| h != &hash);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    pub fn contains(&self, tx_hash: &[u8; 32]) -> bool {
        self.txs.contains_key(tx_hash)
    }
}

// ─── GulfStreamRouter ─────────────────────────────────────────────────────────

/// Gulf Stream TX yönlendirici
///
/// Solana pattern: `ClusterInfo::push_transaction_to_validators`
/// - Gelen TX'i mevcut ve gelecek liderlere forward et
/// - FORWARD_FANOUT kadar validator'a gönder
#[derive(Debug)]
pub struct GulfStreamRouter {
    /// Kendi validator ID'si
    validator_id: u64,
    /// Lider çizelgesi
    leader_schedule: Arc<LeaderSchedule>,
    /// Her validator'ın gelen TX cache'i
    peer_caches: Arc<Mutex<HashMap<u64, Vec<ForwardedTx>>>>,
    /// Kendi gelen TX cache'i
    local_cache: ForwardedTxCache,
    /// İstatistikler
    stats: GulfStreamStats,
}

#[derive(Debug, Default, Clone)]
pub struct GulfStreamStats {
    pub forwarded: u64,
    pub received: u64,
    pub duplicates_dropped: u64,
    pub expired_pruned: u64,
}

impl GulfStreamRouter {
    pub fn new(validator_id: u64, leader_schedule: Arc<LeaderSchedule>) -> Self {
        Self {
            validator_id,
            leader_schedule,
            peer_caches: Arc::new(Mutex::new(HashMap::new())),
            local_cache: ForwardedTxCache::new(),
            stats: GulfStreamStats::default(),
        }
    }

    /// TX'i uygun validatorlara forward et
    ///
    /// Solana: mevcut lider + gelecekteki FORWARD_FANOUT lider
    pub fn forward_tx(&mut self, tx: ForwardedTx, current_slot: u64) {
        // Gelecekteki FORWARD_FANOUT slot liderini bul
        let mut targets: Vec<u64> = Vec::new();
        let mut seen_validators = std::collections::HashSet::new();

        for offset in 0..=(FORWARD_FANOUT as u64) {
            let slot = tx.target_slot + offset;
            let leader = self.leader_schedule.leader_for_slot(slot);
            if leader != self.validator_id && seen_validators.insert(leader) {
                targets.push(leader);
                if targets.len() >= FORWARD_FANOUT {
                    break;
                }
            }
        }

        // Her hedefe TX'i "gönder" (simülasyon: peer cache'e yaz)
        // NOVA-RED-MEMPOOL-005: peer_caches büyüklük sınırı
        let mut peer_caches = self.peer_caches.lock();
        for target in &targets {
            let cache = peer_caches.entry(*target).or_default();
            if cache.len() < MAX_FORWARDED_TX_PER_VALIDATOR {
                cache.push(tx.clone());
                self.stats.forwarded += 1;
            }
        }
    }

    /// Başka validator'dan gelen TX'i al
    pub fn receive_forwarded_tx(&mut self, tx: ForwardedTx) {
        if self.local_cache.insert(tx) {
            self.stats.received += 1;
        } else {
            self.stats.duplicates_dropped += 1;
        }
    }

    /// Kendi local cache'indeki hazır TX'leri mempool'a aktar
    pub fn drain_ready_for_slot(&mut self, current_slot: u64) -> Vec<ForwardedTx> {
        self.local_cache.drain_ready(current_slot)
    }

    /// Cache temizliği
    pub fn prune(&mut self, current_slot: u64) {
        let pruned = self.local_cache.prune_expired(current_slot);
        self.stats.expired_pruned += pruned as u64;
    }

    /// Mevcut slot için lider mi?
    pub fn is_leader_for(&self, slot: u64) -> bool {
        self.leader_schedule.leader_for_slot(slot) == self.validator_id
    }

    pub fn stats(&self) -> &GulfStreamStats {
        &self.stats
    }

    pub fn local_cache_size(&self) -> usize {
        self.local_cache.len()
    }

    /// Peer caches'i simüle et (test için)
    pub fn simulate_peer_deliveries(&mut self) {
        let mut peer_caches = self.peer_caches.lock();
        if let Some(txs) = peer_caches.remove(&self.validator_id) {
            drop(peer_caches);
            for tx in txs {
                self.receive_forwarded_tx(tx);
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(hash_byte: u8, target_slot: u64, fee: u128) -> ForwardedTx {
        ForwardedTx::new(
            [hash_byte; 32],
            vec![hash_byte; 64],
            target_slot,
            1, // from validator 1
            fee,
            DEFAULT_MAX_AGE_SLOTS,
        )
    }

    fn make_schedule(validators: u64) -> Arc<LeaderSchedule> {
        Arc::new(LeaderSchedule::new(validators))
    }

    // 1. TX hazır olup olmadığı kontrolü
    #[test]
    fn tx_ready_in_correct_slot() {
        let tx = make_tx(1, 100, 1000);
        assert!(!tx.is_ready(99)); // çok erken
        assert!(tx.is_ready(100)); // tam zamanında
        assert!(tx.is_ready(150)); // hâlâ geçerli
        assert!(!tx.is_ready(100 + DEFAULT_MAX_AGE_SLOTS + 1)); // süresi doldu
    }

    // 2. TX süresi dolma kontrolü
    #[test]
    fn tx_expires_correctly() {
        let tx = make_tx(2, 100, 1000);
        let expiry = tx.expires_at_slot;
        assert!(!tx.is_expired(expiry));
        assert!(tx.is_expired(expiry + 1));
    }

    // 3. Cache'e TX ekleme
    #[test]
    fn cache_insert_and_contains() {
        let mut cache = ForwardedTxCache::new();
        let tx = make_tx(3, 100, 1000);
        let hash = tx.tx_hash;
        assert!(cache.insert(tx));
        assert!(cache.contains(&hash));
    }

    // 4. Duplicate TX reddedilir
    #[test]
    fn cache_rejects_duplicate() {
        let mut cache = ForwardedTxCache::new();
        let tx = make_tx(4, 100, 1000);
        assert!(cache.insert(tx.clone()));
        assert!(!cache.insert(tx)); // duplicate
    }

    // 5. Hazır TX'ler drain edilir
    #[test]
    fn cache_drains_ready_txs() {
        let mut cache = ForwardedTxCache::new();
        cache.insert(make_tx(5, 100, 1000)); // target slot 100
        cache.insert(make_tx(6, 200, 500)); // target slot 200

        let ready = cache.drain_ready(100); // sadece slot 100 hazır
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].tx_hash, [5u8; 32]);
        assert_eq!(cache.len(), 1); // slot 200'deki kaldı
    }

    // 6. Öncelik sıralaması
    #[test]
    fn cache_drains_by_priority() {
        let mut cache = ForwardedTxCache::new();
        cache.insert(make_tx(7, 100, 100));  // düşük fee
        cache.insert(make_tx(8, 100, 1000)); // yüksek fee
        cache.insert(make_tx(9, 100, 500));  // orta fee

        let ready = cache.drain_ready(100);
        assert_eq!(ready.len(), 3);
        // Yüksek fee önce
        assert_eq!(ready[0].priority_fee, 1000);
        assert_eq!(ready[1].priority_fee, 500);
        assert_eq!(ready[2].priority_fee, 100);
    }

    // 7. Süresi dolan TX'leri temizle
    #[test]
    fn cache_prunes_expired() {
        let mut cache = ForwardedTxCache::new();
        cache.insert(make_tx(10, 100, 1000)); // expires at 250
        cache.insert(make_tx(11, 200, 500));  // expires at 350

        let pruned = cache.prune_expired(300);
        assert_eq!(pruned, 1); // slot 100'ün olanı temizlendi
        assert_eq!(cache.len(), 1);
    }

    // 8. Lider çizelgesi round-robin
    #[test]
    fn leader_schedule_round_robin() {
        let schedule = LeaderSchedule::new(5);
        // 5 validatorlü sistem: slot 0→val1, 1→val2, 2→val3, 3→val4, 4→val5, 5→val1...
        assert_eq!(schedule.leader_for_slot(0), 1);
        assert_eq!(schedule.leader_for_slot(4), 5);
        assert_eq!(schedule.leader_for_slot(5), 1); // döngü
    }

    // 9. Manuel lider atama
    #[test]
    fn leader_schedule_manual_override() {
        let mut schedule = LeaderSchedule::new(5);
        schedule.set_leader(10, 3);
        assert_eq!(schedule.leader_for_slot(10), 3);
    }

    // 10. GulfStream router forward eder
    #[test]
    fn router_forwards_to_future_leaders() {
        let schedule = make_schedule(5);
        let mut router = GulfStreamRouter::new(1, schedule.clone());
        let tx = make_tx(20, 10, 1000);

        router.forward_tx(tx, 5);

        // Forward edilmeli (validator 1 kendisine forward etmez)
        assert!(router.stats().forwarded > 0);
    }

    // 11. Router lider kontrolü
    #[test]
    fn router_detects_leader_slot() {
        let schedule = make_schedule(5);
        let router = GulfStreamRouter::new(1, schedule);
        assert!(router.is_leader_for(0)); // slot 0 → validator 1
        assert!(!router.is_leader_for(1)); // slot 1 → validator 2
    }

    // 12. TX cache→router entegrasyonu
    #[test]
    fn router_receives_and_drains_txs() {
        let schedule = make_schedule(5);
        let mut router = GulfStreamRouter::new(2, schedule);

        // Validator 1'den forwarded TX gönder
        router.receive_forwarded_tx(make_tx(30, 100, 2000));
        router.receive_forwarded_tx(make_tx(31, 100, 1500));
        assert_eq!(router.local_cache_size(), 2);

        let ready = router.drain_ready_for_slot(100);
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].priority_fee, 2000); // yüksek fee önce
        assert_eq!(router.local_cache_size(), 0);
    }

    // 13. Peer delivery simülasyonu
    #[test]
    fn router_simulate_peer_delivery() {
        let schedule = make_schedule(5);
        let mut router1 = GulfStreamRouter::new(1, schedule.clone());
        let tx = make_tx(40, 5, 3000); // slot 5 → lider validator 1 (0%5+1=1)

        // Router1'den forward: hedef slot 5'te lider kim? validator 1
        // Ama validator 1 kendi kendine forward etmez → diğerlerine gider
        router1.forward_tx(tx.clone(), 0);

        // Peer cache'de validator 2 için TX var mı?
        {
            let caches = router1.peer_caches.lock();
            // En az bir peer'a gönderilmeli
            let total: usize = caches.values().map(|v| v.len()).sum();
            assert!(total > 0, "hiç peer'a forward yapılmadı");
        }
    }
}
