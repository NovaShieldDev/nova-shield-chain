//! Peer bilgi türleri
//!
//! Polkadot: `sc-network::service::NetworkState::Peer`
//! Avalanchego: `peer.Info` struct (NodeID, IP, Version, TrackedSubnets)

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── PeerId ───────────────────────────────────────────────────────────────────

/// Peer kimliği — 32 byte public key (Noise statik key'den türetilir)
/// Polkadot: `libp2p::PeerId` (multihash)
/// Avalanchego: `ids.NodeID` (20 byte certificate hash)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId {
    pub bytes: [u8; 32],
}

impl PeerId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn from_key(key: &[u8; 32]) -> Self {
        // Üretimde: multihash(public_key) — burada direkt
        Self { bytes: *key }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    pub fn short_display(&self) -> String {
        hex::encode(&self.bytes[..4])
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex()[..12])
    }
}

// ─── PeerRole ─────────────────────────────────────────────────────────────────

/// Peer rolü — Polkadot `ObservedRole`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerRole {
    /// Tam validator node
    Validator,
    /// Light client
    Light,
    /// Full node (validator değil)
    Full,
    /// Bilinmeyen
    Unknown,
}

// ─── PeerState ────────────────────────────────────────────────────────────────

/// Peer bağlantı durumu
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    /// Bağlantı deneniyor
    Connecting,
    /// El sıkışma yapılıyor
    Handshaking,
    /// Bağlı ve aktif
    Connected,
    /// Bağlantı kesildi
    Disconnected,
    /// Yasaklandı
    Banned,
}

// ─── PeerInfo ─────────────────────────────────────────────────────────────────

/// Peer bilgisi — Avalanchego `peer.Info` benzeri
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer kimliği
    pub peer_id: PeerId,
    /// IP:port
    pub addr: String,
    /// Peer rolü
    pub role: PeerRole,
    /// Mevcut durum
    pub state: PeerState,
    /// Çalışan versiyon — Avalanchego `version.Application`
    pub version: String,
    /// İtibar puanı — Polkadot `ReputationChange`
    pub reputation: i32,
    /// Bağlantı zamanı (Unix ms)
    pub connected_at_ms: u64,
    /// Son mesaj zamanı (Unix ms)
    pub last_message_at_ms: u64,
    /// Gönderilen bayt
    pub bytes_sent: u64,
    /// Alınan bayt
    pub bytes_received: u64,
    /// Ping süresi ms
    pub ping_ms: u64,
    /// Takip edilen subnet sayısı — Avalanchego: maxNumTrackedSubnets=16
    pub tracked_subnets: Vec<String>,
}

impl PeerInfo {
    pub fn new(peer_id: PeerId, addr: impl Into<String>, role: PeerRole) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        Self {
            peer_id,
            addr: addr.into(),
            role,
            state: PeerState::Connecting,
            version: "nova-shield/0.1.0".into(),
            reputation: 0,
            connected_at_ms: now,
            last_message_at_ms: now,
            bytes_sent: 0,
            bytes_received: 0,
            ping_ms: 0,
            tracked_subnets: vec![],
        }
    }

    /// Bağlı mı?
    pub fn is_connected(&self) -> bool {
        self.state == PeerState::Connected
    }

    /// Yasaklı mı?
    pub fn is_banned(&self) -> bool {
        self.state == PeerState::Banned
    }

    /// Gönderilen + alınan toplam
    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent + self.bytes_received
    }

    /// Bağlantı süresi ms
    pub fn uptime_ms(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        now.saturating_sub(self.connected_at_ms)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer_id(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    #[test]
    fn test_peer_id_hex() {
        let id = make_peer_id(0xAB);
        let hex = id.to_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.starts_with("ab"));
    }

    #[test]
    fn test_peer_id_short_display() {
        let id = make_peer_id(0x12);
        let short = id.short_display();
        assert_eq!(short.len(), 8);
    }

    #[test]
    fn test_peer_info_new() {
        let id = make_peer_id(1);
        let info = PeerInfo::new(id.clone(), "127.0.0.1:30333", PeerRole::Validator);
        assert_eq!(info.peer_id, id);
        assert_eq!(info.state, PeerState::Connecting);
        assert_eq!(info.reputation, 0);
        assert!(!info.is_connected());
        assert!(!info.is_banned());
    }

    #[test]
    fn test_peer_info_total_bytes() {
        let id = make_peer_id(1);
        let mut info = PeerInfo::new(id, "127.0.0.1:30333", PeerRole::Full);
        info.bytes_sent = 1000;
        info.bytes_received = 500;
        assert_eq!(info.total_bytes(), 1500);
    }

    #[test]
    fn test_peer_state_transitions() {
        let id = make_peer_id(2);
        let mut info = PeerInfo::new(id, "10.0.0.1:30333", PeerRole::Validator);
        info.state = PeerState::Connected;
        assert!(info.is_connected());
        info.state = PeerState::Banned;
        assert!(info.is_banned());
        assert!(!info.is_connected());
    }

    #[test]
    fn test_peer_id_equality() {
        let a = make_peer_id(5);
        let b = make_peer_id(5);
        let c = make_peer_id(6);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
