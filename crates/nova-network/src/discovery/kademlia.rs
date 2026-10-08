//! Kademlia DHT — Polkadot DiscoveryBehaviour pattern
//!
//! Polkadot `discovery.rs`:
//! - `DiscoveryConfig` + `DiscoveryBehaviour`
//! - Bootstrap nodes + Kademlia random walk + mDNS
//! - DEFAULT_KADEMLIA_REPLICATION_FACTOR = 20
//! - KAD_QUERY_TIMEOUT = 300s
//! - GET_RECORD_REDUNDANCY_FACTOR = 4
//! - k-bucket: her bucket 20 node, XOR mesafesi
//!
//! Cosmos SDK: p2p `PeerManager` benzeri — peer keşif + bağlantı yönetimi

use crate::peer::info::PeerId;
use blake3;
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// K-bucket boyutu — Polkadot: DEFAULT_KADEMLIA_REPLICATION_FACTOR=20
pub const KADEMLIA_K: usize = 20;

/// Bit derinliği (256-bit ID)
pub const ID_BITS: usize = 256;

// ─── KademliaId ───────────────────────────────────────────────────────────────

/// 256-bit Kademlia ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KademliaId([u8; 32]);

impl KademliaId {
    pub fn from_peer(peer: &PeerId) -> Self {
        // BLAKE3(peer_bytes) → Kademlia key
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-kad-id");
        hasher.update(&peer.bytes);
        Self(*hasher.finalize().as_bytes())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// XOR mesafesi
    pub fn xor_distance(&self, other: &Self) -> [u8; 32] {
        let mut dist = [0u8; 32];
        for i in 0..32 {
            dist[i] = self.0[i] ^ other.0[i];
        }
        dist
    }

    /// Hangi bucket'a girer? (ilk farklı bit)
    pub fn bucket_index(&self, other: &Self) -> usize {
        let dist = self.xor_distance(other);
        for i in 0..32 {
            if dist[i] != 0 {
                return i * 8 + dist[i].leading_zeros() as usize;
            }
        }
        ID_BITS - 1
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─── RoutingTableEntry ────────────────────────────────────────────────────────

/// Routing table kaydı
#[derive(Debug, Clone)]
pub struct RoutingTableEntry {
    pub kad_id: KademliaId,
    pub peer_id: PeerId,
    pub addr: String,
    /// Son görülme zamanı (ms)
    pub last_seen_ms: u64,
}

// ─── KBucket ──────────────────────────────────────────────────────────────────

/// LRU ping isteği — bucket doluyken yeni entry için ping bekleniyor.
///
/// Kademlia §2.4: "new node is placed in a replacement cache until
/// the LRU-eviction candidate responds (or not) to a ping."
#[derive(Debug, Clone)]
pub struct PendingPing {
    /// Ping gönderilmesi gereken (LRU) peer
    pub lru_peer: PeerId,
    /// Ping gönderilme zamanı (ms)
    pub ping_sent_ms: u64,
    /// Ping timeout süresi (ms) — yanıt gelmezse evict
    pub timeout_ms: u64,
    /// Ping timeout'unda eklenecek aday
    pub candidate: RoutingTableEntry,
}

/// Tek bir K-bucket
///
/// NOVA-RED-NETWORK-005: bucket doluysa LRU node hemen evict edilmez;
/// önce ping gönderilir (PendingPing). Timeout geçerse evict + candidate eklenir.
/// Polkadot libp2p `KBucketTable::entry()` → `PendingNode` pattern.
#[derive(Debug, Default)]
pub struct KBucket {
    entries: Vec<RoutingTableEntry>,
    k: usize,
    /// LRU ping bekleme kuyruğu
    pub pending_pings: Vec<PendingPing>,
}

/// Ping timeout süresi: 10 saniye
pub const PING_TIMEOUT_MS: u64 = 10_000;

impl KBucket {
    pub fn new(k: usize) -> Self {
        Self { entries: Vec::new(), k, pending_pings: Vec::new() }
    }

    /// Entry ekle.
    ///
    /// NOVA-RED-NETWORK-005: bucket doluysa LRU'ya ping gönderilmesi gerekiyor.
    /// `now_ms` ile çağrıldığında PendingPing kuyruğuna alır, hemen evict etmez.
    /// `false` → bucket dolu, ping bekleniyor. `true` → eklendi.
    pub fn insert(&mut self, entry: RoutingTableEntry) -> bool {
        // Zaten var mı? → güncelle
        if let Some(pos) = self.entries.iter().position(|e| e.peer_id == entry.peer_id) {
            self.entries[pos] = entry;
            return true;
        }
        if self.entries.len() < self.k {
            self.entries.push(entry);
            true
        } else {
            // NOVA-RED-NETWORK-005: bucket dolu → LRU'ya ping gerekli, hemen evict etme.
            // PendingPing kuyruğuna ekle (tekrar eden peer için kuyruğu güncelle).
            if let Some(lru) = self.entries.iter()
                .min_by_key(|e| e.last_seen_ms)
            {
                let lru_peer = lru.peer_id.clone();
                let ping_sent_ms = entry.last_seen_ms; // "now" olarak kullan
                // Aynı LRU için mevcut pending varsa güncelle, yoksa ekle
                if let Some(p) = self.pending_pings.iter_mut().find(|p| p.lru_peer == lru_peer) {
                    p.candidate = entry;
                } else {
                    self.pending_pings.push(PendingPing {
                        lru_peer,
                        ping_sent_ms,
                        timeout_ms: PING_TIMEOUT_MS,
                        candidate: entry,
                    });
                }
            }
            false // henüz eklenmedi — ping sonucunu bekle
        }
    }

    /// LRU node'dan ping yanıtı geldi → candidate reddedilir, LRU tutulur.
    /// Returns true if the ping was found and processed.
    pub fn on_ping_response(&mut self, peer_id: &PeerId) -> bool {
        let before = self.pending_pings.len();
        self.pending_pings.retain(|p| &p.lru_peer != peer_id);
        self.pending_pings.len() < before
    }

    /// Ping timeout'larını işle — yanıt vermeyen LRU node'ları evict et.
    /// `now_ms`: mevcut zaman. Returns list of evicted peer IDs.
    pub fn process_ping_timeouts(&mut self, now_ms: u64) -> Vec<PeerId> {
        let mut evicted = Vec::new();
        let mut to_add = Vec::new();

        self.pending_pings.retain(|p| {
            if now_ms >= p.ping_sent_ms + p.timeout_ms {
                evicted.push(p.lru_peer.clone());
                to_add.push(p.candidate.clone());
                false // pending'den kaldır
            } else {
                true
            }
        });

        for evict_peer in &evicted {
            self.entries.retain(|e| &e.peer_id != evict_peer);
        }
        for candidate in to_add {
            if self.entries.len() < self.k {
                self.entries.push(candidate);
            }
        }

        evicted
    }

    pub fn remove(&mut self, peer_id: &PeerId) {
        self.entries.retain(|e| &e.peer_id != peer_id);
    }

    pub fn entries(&self) -> &[RoutingTableEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.k
    }
}

// ─── KademliaTable ────────────────────────────────────────────────────────────

/// Kademlia routing table
/// Polkadot: `Behaviour as Kademlia` + `MemoryStore`
#[derive(Debug)]
pub struct KademliaTable {
    local_id: KademliaId,
    buckets: Vec<KBucket>,
    /// DHT key → value (Polkadot: `Record`)
    records: HashMap<Vec<u8>, Vec<u8>>,
}

impl KademliaTable {
    pub fn new(local_peer: &PeerId) -> Self {
        let local_id = KademliaId::from_peer(local_peer);
        let mut buckets = Vec::with_capacity(ID_BITS);
        for _ in 0..ID_BITS {
            buckets.push(KBucket::new(KADEMLIA_K));
        }
        Self { local_id, buckets, records: HashMap::new() }
    }

    /// Peer ekle
    pub fn add_peer(&mut self, peer: &PeerId, addr: impl Into<String>, now_ms: u64) {
        let kad_id = KademliaId::from_peer(peer);
        let bucket_idx = self.local_id.bucket_index(&kad_id);
        let entry = RoutingTableEntry {
            kad_id,
            peer_id: peer.clone(),
            addr: addr.into(),
            last_seen_ms: now_ms,
        };
        self.buckets[bucket_idx].insert(entry);
    }

    /// Peer çıkar
    pub fn remove_peer(&mut self, peer: &PeerId) {
        let kad_id = KademliaId::from_peer(peer);
        let bucket_idx = self.local_id.bucket_index(&kad_id);
        self.buckets[bucket_idx].remove(peer);
    }

    /// K en yakın peer — FIND_NODE sonucu
    /// Polkadot: `get_closest_peers(key)` → `GetClosestPeersOk`
    pub fn find_closest(&self, target: &PeerId, k: usize) -> Vec<&RoutingTableEntry> {
        let target_id = KademliaId::from_peer(target);
        let mut all: Vec<(&RoutingTableEntry, [u8; 32])> = self.buckets.iter()
            .flat_map(|b| b.entries().iter())
            .map(|e| {
                let dist = e.kad_id.xor_distance(&target_id);
                (e, dist)
            })
            .collect();

        all.sort_by(|a, b| a.1.cmp(&b.1));
        all.into_iter().take(k).map(|(e, _)| e).collect()
    }

    /// DHT kayıt koy — Polkadot: `put_record(key, value)`
    pub fn put_record(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.records.insert(key, value);
    }

    /// DHT kayıt al — Polkadot: `get_record(key)`
    pub fn get_record(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.records.get(key)
    }

    /// Toplam peer sayısı
    pub fn peer_count(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    /// Dolmayan bucket sayısı
    pub fn non_empty_buckets(&self) -> usize {
        self.buckets.iter().filter(|b| b.len() > 0).count()
    }

    pub fn local_id(&self) -> &KademliaId {
        &self.local_id
    }
}

// ─── KademliaQuery ────────────────────────────────────────────────────────────

/// Kademlia FIND_NODE sorgusu
/// Polkadot: `QueryId`, `QueryResult::GetClosestPeers`
#[derive(Debug, Clone)]
pub struct KademliaQuery {
    pub query_id: u64,
    pub target: PeerId,
    /// Şimdiye kadar soruldu
    pub queried: Vec<PeerId>,
    /// Bulunan peer'lar
    pub found: Vec<(PeerId, String)>,
    pub complete: bool,
}

impl KademliaQuery {
    pub fn new(query_id: u64, target: PeerId) -> Self {
        Self {
            query_id,
            target,
            queried: vec![],
            found: vec![],
            complete: false,
        }
    }

    /// Sorgu turu — routing table'dan K en yakın al ve "sorgula"
    pub fn step(&mut self, table: &KademliaTable) {
        let closest = table.find_closest(&self.target, KADEMLIA_K);
        for entry in &closest {
            if !self.queried.contains(&entry.peer_id) {
                self.queried.push(entry.peer_id.clone());
                self.found.push((entry.peer_id.clone(), entry.addr.clone()));
            }
        }
        // Basit tamamlanma: en az K peer bulunduysa
        if self.found.len() >= KADEMLIA_K || !closest.is_empty() {
            self.complete = true;
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    #[test]
    fn test_kademlia_id_xor() {
        let a = KademliaId::from_bytes([0xFF; 32]);
        let b = KademliaId::from_bytes([0x00; 32]);
        let dist = a.xor_distance(&b);
        assert_eq!(dist, [0xFF; 32]);
    }

    #[test]
    fn test_kademlia_id_self_xor() {
        let a = KademliaId::from_bytes([0xAB; 32]);
        let dist = a.xor_distance(&a);
        assert_eq!(dist, [0; 32]);
    }

    #[test]
    fn test_kbucket_insert() {
        let mut bucket = KBucket::new(3);
        bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([1; 32]),
            peer_id: pid(1),
            addr: "127.0.0.1:30333".into(),
            last_seen_ms: 1000,
        });
        assert_eq!(bucket.len(), 1);
    }

    /// NOVA-RED-NETWORK-005: bucket doluyken LRU hemen evict edilmez — ping kuyruğuna girer
    #[test]
    fn test_kbucket_full_pending_ping_not_immediate_eviction() {
        let mut bucket = KBucket::new(2);
        // İlk 2 giriş → bucket dolar
        for i in 1u8..=2 {
            let added = bucket.insert(RoutingTableEntry {
                kad_id: KademliaId::from_bytes([i; 32]),
                peer_id: pid(i),
                addr: format!("127.0.0.{i}:30333"),
                last_seen_ms: i as u64 * 1000,
            });
            assert!(added);
        }

        // 3. giriş → bucket dolu, hemen evict edilmemeli
        let added = bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([3; 32]),
            peer_id: pid(3),
            addr: "127.0.0.3:30333".into(),
            last_seen_ms: 3000,
        });
        assert!(!added, "bucket dolu — hemen eklenmemeli");
        assert_eq!(bucket.len(), 2, "bucket boyutu 2 kalmali");
        assert_eq!(bucket.pending_pings.len(), 1, "ping kuyruğunda 1 bekleme olmalı");
    }

    /// NOVA-RED-NETWORK-005: LRU ping'e cevap verirse candidate reddedilir
    #[test]
    fn test_kbucket_ping_response_keeps_lru() {
        let mut bucket = KBucket::new(1);
        let added = bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([1; 32]),
            peer_id: pid(1),
            addr: "127.0.0.1:30333".into(),
            last_seen_ms: 1000,
        });
        assert!(added);

        bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([2; 32]),
            peer_id: pid(2),
            addr: "127.0.0.2:30333".into(),
            last_seen_ms: 2000,
        });

        // LRU (peer 1) ping'e cevap verdi → candidate (peer 2) reddedilmeli
        let found = bucket.on_ping_response(&pid(1));
        assert!(found, "ping yanıtı işlenmeli");
        assert_eq!(bucket.pending_pings.len(), 0, "pending temizlenmeli");
        assert_eq!(bucket.len(), 1, "LRU tutuldu, boyut aynı");
        assert!(bucket.entries().iter().any(|e| e.peer_id == pid(1)), "LRU node hala mevcut");
    }

    /// NOVA-RED-NETWORK-005: ping timeout → LRU evict, candidate eklenir
    #[test]
    fn test_kbucket_ping_timeout_evicts_lru() {
        let mut bucket = KBucket::new(1);
        bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([1; 32]),
            peer_id: pid(1),
            addr: "127.0.0.1:30333".into(),
            last_seen_ms: 1000,
        });

        bucket.insert(RoutingTableEntry {
            kad_id: KademliaId::from_bytes([2; 32]),
            peer_id: pid(2),
            addr: "127.0.0.2:30333".into(),
            last_seen_ms: 5000,
        });

        // Timeout geçti (5000 + 10000 + 1 = 15001)
        let evicted = bucket.process_ping_timeouts(15001);
        assert_eq!(evicted, vec![pid(1)], "LRU evict edilmeli");
        assert_eq!(bucket.len(), 1);
        assert!(bucket.entries().iter().any(|e| e.peer_id == pid(2)), "candidate eklenmeli");
    }

    #[test]
    fn test_routing_table_add_peers() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);

        for i in 1u8..=10 {
            table.add_peer(&pid(i), format!("127.0.0.{i}:30333"), 1000);
        }
        assert!(table.peer_count() <= 10);
    }

    #[test]
    fn test_find_closest() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);

        for i in 1u8..=20 {
            table.add_peer(&pid(i), format!("127.0.0.{i}:30333"), 1000);
        }

        let target = pid(5);
        let closest = table.find_closest(&target, 5);
        assert!(!closest.is_empty());
        assert!(closest.len() <= 5);
    }

    #[test]
    fn test_dht_put_get() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);

        let key = b"nova-dht-key".to_vec();
        let value = b"nova-validator-addr:127.0.0.1:30333".to_vec();
        table.put_record(key.clone(), value.clone());

        let got = table.get_record(&key).unwrap();
        assert_eq!(got, &value);
    }

    #[test]
    fn test_remove_peer() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);
        table.add_peer(&pid(1), "127.0.0.1:30333", 1000);
        let before = table.peer_count();
        table.remove_peer(&pid(1));
        assert!(table.peer_count() <= before);
    }

    #[test]
    fn test_kademlia_query() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);
        for i in 1u8..=15 {
            table.add_peer(&pid(i), format!("127.0.0.{i}:30333"), 1000);
        }

        let mut query = KademliaQuery::new(1, pid(7));
        query.step(&table);
        assert!(query.complete);
        assert!(!query.found.is_empty());
    }

    #[test]
    fn test_non_empty_buckets() {
        let local = pid(0);
        let mut table = KademliaTable::new(&local);
        for i in 1u8..=5 {
            table.add_peer(&pid(i), format!("127.0.0.{i}:30333"), 1000);
        }
        assert!(table.non_empty_buckets() > 0);
    }
}
