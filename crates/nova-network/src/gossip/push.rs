//! CRDS Push Gossip — Solana pattern
//!
//! Solana `crds_gossip_push.rs`:
//! - CRDS_GOSSIP_PUSH_FANOUT = 9
//! - CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS = 30_000
//! - `PushActiveSet`: aktif gönderim hedefleri
//! - `ReceivedCache`: tekilleştirme için yakın zamanda alınanlar
//! - Plumtree tabanlı push protokolü

use crate::gossip::crds::CrdsValue;
use crate::peer::info::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Push fanout — Solana: CRDS_GOSSIP_PUSH_FANOUT=9
pub const PUSH_FANOUT: usize = 9;

/// Push timeout ms — Solana: CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS=30_000
pub const PUSH_MSG_TIMEOUT_MS: u64 = 30_000;

/// Maksimum push entry boyutu
pub const MAX_PUSH_SIZE: usize = 64;

// ─── PushMessage ──────────────────────────────────────────────────────────────

/// Push mesajı — CRDS değerleri paketi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushMessage {
    pub from: [u8; 32],
    pub values: Vec<CrdsValue>,
}

// ─── ReceivedCache ────────────────────────────────────────────────────────────

/// Yakın zamanda alınan değerlerin cache'i — tekilleştirme
/// Solana: `ReceivedCache { cache: HashMap<Hash, (Instant, SmallVec<PeerId>)> }`
#[derive(Debug, Default)]
pub struct ReceivedCache {
    /// hash → (wallclock, alınan_peer_id'ler)
    cache: HashMap<[u8; 32], (u64, HashSet<[u8; 32]>)>,
    timeout_ms: u64,
}

impl ReceivedCache {
    pub fn new(timeout_ms: u64) -> Self {
        Self { cache: HashMap::new(), timeout_ms }
    }

    /// Değer daha önce alındı mı?
    pub fn is_duplicate(&self, hash: &[u8; 32], now_ms: u64) -> bool {
        match self.cache.get(hash) {
            None => false,
            Some((ts, _)) => now_ms.saturating_sub(*ts) < self.timeout_ms,
        }
    }

    /// Alınan değeri kaydet — peer_id ile birlikte
    pub fn record(&mut self, hash: [u8; 32], from: [u8; 32], now_ms: u64) {
        let entry = self.cache.entry(hash).or_insert_with(|| (now_ms, HashSet::new()));
        entry.1.insert(from);
    }

    /// Eski kayıtları temizle
    pub fn prune(&mut self, now_ms: u64) {
        self.cache.retain(|_, (ts, _)| now_ms.saturating_sub(*ts) < self.timeout_ms);
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

// ─── PushActiveSet ────────────────────────────────────────────────────────────

/// Aktif push hedef seti — Solana `PushActiveSet`
/// Plumtree: eager push tree (FANOUT kadar peer'e gönder)
#[derive(Debug, Default)]
pub struct PushActiveSet {
    /// Aktif hedefler (peer_id → son push slot)
    peers: HashMap<[u8; 32], u64>,
    max_size: usize,
}

impl PushActiveSet {
    pub fn new(max_size: usize) -> Self {
        Self { peers: HashMap::new(), max_size }
    }

    /// Peer ekle
    pub fn add(&mut self, peer_id: [u8; 32], slot: u64) {
        if self.peers.len() < self.max_size || self.peers.contains_key(&peer_id) {
            self.peers.insert(peer_id, slot);
        }
    }

    /// Peer çıkar
    pub fn remove(&mut self, peer_id: &[u8; 32]) {
        self.peers.remove(peer_id);
    }

    /// Fanout kadar peer seç (rastgele)
    pub fn select_fanout(&self, fanout: usize, exclude: &HashSet<[u8; 32]>) -> Vec<[u8; 32]> {
        self.peers
            .keys()
            .filter(|id| !exclude.contains(*id))
            .take(fanout)
            .copied()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn contains(&self, peer_id: &[u8; 32]) -> bool {
        self.peers.contains_key(peer_id)
    }
}

// ─── CrdsGossipPush ───────────────────────────────────────────────────────────

/// CRDS Push protokolü
/// Solana `CrdsGossipPush` benzeri
#[derive(Debug)]
pub struct CrdsGossipPush {
    /// Yerel node ID
    local_id: [u8; 32],
    /// Aktif push hedefleri
    active_set: PushActiveSet,
    /// Tekilleştirme cache'i
    received_cache: ReceivedCache,
    /// Bekleyen push kuyruğu
    pending: VecDeque<CrdsValue>,
    /// Fanout
    fanout: usize,
}

impl CrdsGossipPush {
    pub fn new(local_id: [u8; 32], fanout: usize) -> Self {
        Self {
            local_id,
            active_set: PushActiveSet::new(fanout * 3),
            received_cache: ReceivedCache::new(PUSH_MSG_TIMEOUT_MS),
            pending: VecDeque::new(),
            fanout,
        }
    }

    /// Push hedef ekle
    pub fn add_peer(&mut self, peer_id: [u8; 32], slot: u64) {
        self.active_set.add(peer_id, slot);
    }

    /// Push hedef çıkar
    pub fn remove_peer(&mut self, peer_id: &[u8; 32]) {
        self.active_set.remove(peer_id);
    }

    /// Yeni değer kuyruğa ekle
    pub fn push_value(&mut self, value: CrdsValue) {
        if self.pending.len() < MAX_PUSH_SIZE {
            self.pending.push_back(value);
        }
    }

    /// Push döngüsü — fanout kadar peer'e mesajlar oluştur
    /// Solana: `new_push_messages(crds, now)` → `HashMap<Pubkey, Vec<CrdsValue>>`
    pub fn new_push_messages(&mut self, now_ms: u64) -> HashMap<[u8; 32], PushMessage> {
        let mut result: HashMap<[u8; 32], PushMessage> = HashMap::new();
        let exclude: HashSet<[u8; 32]> = HashSet::new();
        let targets = self.active_set.select_fanout(self.fanout, &exclude);

        let values: Vec<CrdsValue> = self.pending.drain(..).collect();

        for target in &targets {
            if !values.is_empty() {
                result.insert(*target, PushMessage {
                    from: self.local_id,
                    values: values.clone(),
                });
            }
        }

        // Alınan cache'e ekle
        for v in &values {
            self.received_cache.record(v.hash(), self.local_id, now_ms);
        }

        result
    }

    /// Gelen push mesajını işle
    /// Solana: `process_push_message(crds, messages, now)` → `Vec<CrdsValue>` (yeni olanlar)
    pub fn process_push_message(
        &mut self,
        msg: &PushMessage,
        now_ms: u64,
    ) -> Vec<CrdsValue> {
        let mut new_values = Vec::new();
        for value in &msg.values {
            let hash = value.hash();
            if !self.received_cache.is_duplicate(&hash, now_ms) {
                self.received_cache.record(hash, msg.from, now_ms);
                if value.verify() {
                    new_values.push(value.clone());
                }
            }
        }
        new_values
    }

    /// Eski cache kayıtlarını temizle
    pub fn prune(&mut self, now_ms: u64) {
        self.received_cache.prune(now_ms);
    }

    pub fn active_peer_count(&self) -> usize {
        self.active_set.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::crds::{CrdsData, CrdsValue};

    fn node(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn contact_value(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: node(n),
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 1000 + n as u64,
        })
    }

    #[test]
    fn test_push_new_messages() {
        let mut push = CrdsGossipPush::new(node(0), PUSH_FANOUT);
        // Peer ekle
        for i in 1u8..=5 {
            push.add_peer(node(i), 100);
        }
        // Değer kuyruğa ekle
        push.push_value(contact_value(1));
        push.push_value(contact_value(2));

        let messages = push.new_push_messages(1000);
        // Fanout=9 ama sadece 5 peer var, 5 mesaj
        assert!(messages.len() <= 5);
        // Her mesaj 2 değer içermeli
        for (_, msg) in &messages {
            assert_eq!(msg.values.len(), 2);
        }
    }

    #[test]
    fn test_push_deduplication() {
        let mut push = CrdsGossipPush::new(node(0), PUSH_FANOUT);
        let v = contact_value(1);
        let hash = v.hash();
        push.received_cache.record(hash, node(2), 1000);

        let msg = PushMessage { from: node(2), values: vec![v] };
        let new_vals = push.process_push_message(&msg, 1000);
        assert!(new_vals.is_empty()); // Tekrar, reddedilmeli
    }

    #[test]
    fn test_push_new_value_accepted() {
        let mut push = CrdsGossipPush::new(node(0), PUSH_FANOUT);
        let v = contact_value(1);
        let msg = PushMessage { from: node(2), values: vec![v.clone()] };
        let new_vals = push.process_push_message(&msg, 1000);
        assert_eq!(new_vals.len(), 1);
    }

    #[test]
    fn test_push_active_set() {
        let mut push = CrdsGossipPush::new(node(0), PUSH_FANOUT);
        push.add_peer(node(1), 100);
        push.add_peer(node(2), 100);
        assert_eq!(push.active_peer_count(), 2);
        push.remove_peer(&node(1));
        assert_eq!(push.active_peer_count(), 1);
    }

    #[test]
    fn test_received_cache_prune() {
        let mut cache = ReceivedCache::new(1000);
        let h = [1u8; 32];
        cache.record(h, [2u8; 32], 0);
        assert!(cache.is_duplicate(&h, 500));
        cache.prune(2000); // süresi doldu
        assert!(!cache.is_duplicate(&h, 2000));
    }

    #[test]
    fn test_push_max_pending() {
        let mut push = CrdsGossipPush::new(node(0), PUSH_FANOUT);
        // MAX_PUSH_SIZE'dan fazla değer ekle
        for i in 0u8..100 {
            push.push_value(contact_value(i % 200));
        }
        assert!(push.pending_count() <= MAX_PUSH_SIZE);
    }

    #[test]
    fn test_push_fanout_constant() {
        assert_eq!(PUSH_FANOUT, 9); // Solana değeri
    }

    #[test]
    fn test_push_timeout_constant() {
        assert_eq!(PUSH_MSG_TIMEOUT_MS, 30_000);
    }
}
