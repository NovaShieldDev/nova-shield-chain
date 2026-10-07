//! Peer itibat yönetimi (PeerStore)
//!
//! Polkadot pattern:
//! - BANNED_THRESHOLD = 71 * (i32::MIN / 100)
//! - DISCONNECT_REPUTATION_CHANGE = -256
//! - INVERSE_DECREMENT = 200 (her saniye itibarı 1/200 oranında azalt)
//! - FORGET_AFTER = 3600s (itibar 0'a düştükten sonra unut)

use crate::peer::info::{PeerId, PeerInfo, PeerRole, PeerState};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

// ─── Sabitler — Polkadot pattern ──────────────────────────────────────────────

/// Yasaklama eşiği — Polkadot: 71 * (i32::MIN / 100)
pub const BANNED_THRESHOLD: i32 = 71 * (i32::MIN / 100);

/// Bağlantı kesilince itibar düşüşü — Polkadot: -256
pub const DISCONNECT_REPUTATION_CHANGE: i32 = -256;

/// İtibar azalma faktörü (ters) — Polkadot: 200
/// Her periyotta |itibar| / INVERSE_DECREMENT kadar azalt
pub const INVERSE_DECREMENT: i32 = 200;

/// Yasaklama çıkış süresi (slot) — 69 saniye / 0.4s slot = ~172 slot
pub const BAN_ESCAPE_SLOTS: u64 = 172;

// ─── ReputationChange ─────────────────────────────────────────────────────────

/// İtibar değişim olayı — Polkadot `ReputationChange`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationChange {
    pub value: i32,
    pub reason: &'static str,
}

impl ReputationChange {
    pub fn new(value: i32, reason: &'static str) -> Self {
        Self { value, reason }
    }
}

// Önceden tanımlı değişimler
pub const GOOD_MESSAGE: ReputationChange = ReputationChange { value: 1 << 7, reason: "iyi mesaj" };
pub const BAD_MESSAGE: ReputationChange = ReputationChange { value: -(1 << 12), reason: "kötü mesaj" };
pub const TIMEOUT: ReputationChange = ReputationChange { value: -(1 << 10), reason: "zaman aşımı" };
pub const DUPLICATE_MESSAGE: ReputationChange = ReputationChange { value: -(1 << 8), reason: "tekrar mesaj" };
pub const INVALID_GOSSIP: ReputationChange = ReputationChange { value: -(1 << 14), reason: "geçersiz gossip" };

// ─── PeerEntry ────────────────────────────────────────────────────────────────

/// PeerStore'daki peer kaydı
#[derive(Debug, Clone)]
struct PeerEntry {
    info: PeerInfo,
    /// Yasaklanma slot'u (eğer yasaklıysa)
    banned_at_slot: Option<u64>,
}

// ─── PeerStore ────────────────────────────────────────────────────────────────

/// Peer itibat deposu
/// Polkadot: `PeerStore` (parking_lot::Mutex ile korunur)
/// Nova: DashMap (lock-free concurrent)
#[derive(Debug, Default)]
pub struct PeerStore {
    peers: Arc<DashMap<PeerId, PeerEntry>>,
}

impl PeerStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Peer ekle (bootstrap ya da keşif)
    pub fn add_peer(&self, peer_id: PeerId, addr: impl Into<String>, role: PeerRole) {
        self.peers.entry(peer_id.clone()).or_insert_with(|| PeerEntry {
            info: PeerInfo::new(peer_id, addr, role),
            banned_at_slot: None,
        });
    }

    /// İtibar güncelle — Polkadot `report_peer()`
    pub fn report_peer(&self, peer_id: &PeerId, change: &ReputationChange) {
        if let Some(mut entry) = self.peers.get_mut(peer_id) {
            entry.info.reputation = entry
                .info
                .reputation
                .saturating_add(change.value);

            // Eşiğin altına düştüyse yasakla
            if entry.info.reputation <= BANNED_THRESHOLD {
                entry.info.state = PeerState::Banned;
            }
        }
    }

    /// Bağlantı kesilince itibar düşür — Polkadot `report_disconnect()`
    pub fn report_disconnect(&self, peer_id: &PeerId) {
        self.report_peer(peer_id, &ReputationChange {
            value: DISCONNECT_REPUTATION_CHANGE,
            reason: "bağlantı kesildi",
        });
        if let Some(mut e) = self.peers.get_mut(peer_id) {
            if e.info.state == PeerState::Connected {
                e.info.state = PeerState::Disconnected;
            }
        }
    }

    /// Peer yasaklı mı? — Polkadot `is_banned()`
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|e| e.info.reputation <= BANNED_THRESHOLD)
            .unwrap_or(false)
    }

    /// Peer itibatı
    pub fn reputation(&self, peer_id: &PeerId) -> i32 {
        self.peers.get(peer_id).map(|e| e.info.reputation).unwrap_or(0)
    }

    /// Giden bağlantı için en iyi adaylar — Polkadot `outgoing_candidates()`
    /// En yüksek itibarlı (yasaklı olmayanlar), `ignored` setindekiler hariç
    pub fn outgoing_candidates(
        &self,
        count: usize,
        ignored: &HashSet<PeerId>,
    ) -> Vec<PeerId> {
        let mut candidates: Vec<(i32, PeerId)> = self
            .peers
            .iter()
            .filter(|e| {
                !e.info.is_banned()
                    && !ignored.contains(&e.info.peer_id)
                    && e.info.state != PeerState::Connected
            })
            .map(|e| (e.info.reputation, e.info.peer_id.clone()))
            .collect();

        // Yüksek itibar → önce
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        candidates.truncate(count);
        candidates.into_iter().map(|(_, id)| id).collect()
    }

    /// Peer durumunu güncelle
    pub fn set_state(&self, peer_id: &PeerId, state: PeerState) {
        if let Some(mut e) = self.peers.get_mut(peer_id) {
            e.info.state = state;
        }
    }

    /// Peer bilgisi al
    pub fn peer_info(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.get(peer_id).map(|e| e.info.clone())
    }

    /// Tüm bağlı peerler
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|e| e.info.state == PeerState::Connected)
            .map(|e| e.info.peer_id.clone())
            .collect()
    }

    /// Toplam peer sayısı
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// İtibar decay — Polkadot'taki gibi periyodik çağrılır
    /// |itibar| / INVERSE_DECREMENT kadar azalt
    pub fn decay_reputations(&self) {
        for mut entry in self.peers.iter_mut() {
            let rep = entry.info.reputation;
            if rep != 0 {
                let decay = rep / INVERSE_DECREMENT;
                entry.info.reputation = rep.saturating_sub(decay);
            }
        }
    }

    /// Yasaklı peerler listesi
    pub fn banned_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|e| e.info.is_banned())
            .map(|e| e.info.peer_id.clone())
            .collect()
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
    fn test_peer_store_add_and_get() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Validator);
        assert_eq!(store.peer_count(), 1);
        let info = store.peer_info(&pid(1)).unwrap();
        assert_eq!(info.reputation, 0);
    }

    #[test]
    fn test_reputation_increase() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.report_peer(&pid(1), &GOOD_MESSAGE);
        assert!(store.reputation(&pid(1)) > 0);
    }

    #[test]
    fn test_reputation_decrease() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.report_peer(&pid(1), &BAD_MESSAGE);
        assert!(store.reputation(&pid(1)) < 0);
    }

    #[test]
    fn test_ban_on_threshold() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        // Eşiğin altına düşür
        store.report_peer(&pid(1), &ReputationChange::new(i32::MIN, "test"));
        assert!(store.is_banned(&pid(1)));
    }

    #[test]
    fn test_disconnect_decreases_reputation() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.set_state(&pid(1), PeerState::Connected);
        store.report_disconnect(&pid(1));
        assert!(store.reputation(&pid(1)) < 0);
    }

    #[test]
    fn test_outgoing_candidates_excludes_banned() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.add_peer(pid(2), "127.0.0.1:30334", PeerRole::Full);
        // pid(2)'yi yasakla
        store.report_peer(&pid(2), &ReputationChange::new(i32::MIN, "test"));

        let candidates = store.outgoing_candidates(5, &HashSet::new());
        assert!(candidates.contains(&pid(1)));
        assert!(!candidates.contains(&pid(2)));
    }

    #[test]
    fn test_outgoing_candidates_excludes_ignored() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.add_peer(pid(2), "127.0.0.1:30334", PeerRole::Full);

        let mut ignored = HashSet::new();
        ignored.insert(pid(1));

        let candidates = store.outgoing_candidates(5, &ignored);
        assert!(!candidates.contains(&pid(1)));
        assert!(candidates.contains(&pid(2)));
    }

    #[test]
    fn test_reputation_decay() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        // Yüksek itibar ver
        for _ in 0..5 {
            store.report_peer(&pid(1), &GOOD_MESSAGE);
        }
        let before = store.reputation(&pid(1));
        store.decay_reputations();
        let after = store.reputation(&pid(1));
        // Decay uygulandı mı?
        assert!(after <= before);
    }

    #[test]
    fn test_connected_peers() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.add_peer(pid(2), "127.0.0.1:30334", PeerRole::Full);
        store.set_state(&pid(1), PeerState::Connected);

        let connected = store.connected_peers();
        assert_eq!(connected.len(), 1);
        assert!(connected.contains(&pid(1)));
    }

    #[test]
    fn test_banned_peers_list() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "127.0.0.1:30333", PeerRole::Full);
        store.add_peer(pid(2), "127.0.0.1:30334", PeerRole::Full);
        store.report_peer(&pid(1), &ReputationChange::new(i32::MIN, "test"));

        let banned = store.banned_peers();
        assert!(banned.contains(&pid(1)));
        assert!(!banned.contains(&pid(2)));
    }

    #[test]
    fn test_banned_threshold_value() {
        // Polkadot eşiğini kontrol et
        assert!(BANNED_THRESHOLD < 0);
        // i32::MIN / 100 * 71 — bu negatif olmalı
        let threshold = 71i32.wrapping_mul(i32::MIN / 100);
        assert_eq!(BANNED_THRESHOLD, threshold);
    }
}
