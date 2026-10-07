//! AŞAMA 12 — Güvenlik Saldırı Senaryoları (Görev 82-84)
//!
//! # Eclipse Attack (Görev 82)
//! Saldırgan, hedef node'un tüm gelen/giden bağlantılarını kendi node'larıyla doldurur.
//! Nova Shield koruması: /24 subnet IP limiti (BUCKET_IP_LIMIT=2, TABLE_IP_LIMIT=10).
//!
//! # Replay Attack (Görev 83)
//! Saldırgan geçerli bir mesajı kaydedip tekrar gönderir.
//! Nova Shield koruması: mesaj nonce + slot bazlı pencere.
//!
//! # P2P DoS Attack (Görev 84)
//! Saldırgan hedef node'u aşırı mesajla boğar.
//! Nova Shield koruması: itibar sistemi (PeerStore), rate limiter.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ─── Eclipse Attack Senaryosu (Görev 82) ─────────────────────────────────────

/// /24 subnet grubundaki node sayısını takip eder.
/// SubnetIpLimiter ile aynı mantık (nova-network::metrics).
#[derive(Debug, Default)]
pub struct SubnetTracker {
    // prefix (/24) → node sayısı
    counts: HashMap<[u8; 3], usize>,
    // toplam node sayısı
    total: usize,
}

impl SubnetTracker {
    pub const TABLE_IP_LIMIT: usize = 10; // Toplam tablo başına /24 max
    pub const BUCKET_IP_LIMIT: usize = 2; // Tek k-bucket başına /24 max

    pub fn new() -> Self {
        Self::default()
    }

    /// Node eklenebilir mi? (eclipse koruması)
    pub fn can_add(&self, ip: [u8; 4]) -> bool {
        let prefix = [ip[0], ip[1], ip[2]];
        let count = self.counts.get(&prefix).copied().unwrap_or(0);
        count < Self::TABLE_IP_LIMIT
    }

    /// Node ekle. Limit aşıldıysa Err döner.
    pub fn add(&mut self, ip: [u8; 4]) -> Result<(), &'static str> {
        let prefix = [ip[0], ip[1], ip[2]];
        let count = self.counts.entry(prefix).or_insert(0);
        if *count >= Self::TABLE_IP_LIMIT {
            return Err("subnet limiti aşıldı");
        }
        *count += 1;
        self.total += 1;
        Ok(())
    }

    pub fn total_nodes(&self) -> usize {
        self.total
    }

    pub fn subnet_count(&self, ip: [u8; 4]) -> usize {
        let prefix = [ip[0], ip[1], ip[2]];
        self.counts.get(&prefix).copied().unwrap_or(0)
    }
}

/// Eclipse saldırı simülasyonu
pub struct EclipseAttackSimulation {
    tracker: SubnetTracker,
    /// Saldırgan node sayısı (aynı /24'ten)
    attacker_count: usize,
    /// Meşru node sayısı (farklı /24'lerden)
    legitimate_count: usize,
}

impl EclipseAttackSimulation {
    pub fn new() -> Self {
        Self {
            tracker: SubnetTracker::new(),
            attacker_count: 0,
            legitimate_count: 0,
        }
    }

    /// Saldırgan node ekle: hepsi 192.168.1.x subneti
    pub fn add_attacker_node(&mut self, index: u8) -> bool {
        let ip = [192, 168, 1, index];
        if self.tracker.add(ip).is_ok() {
            self.attacker_count += 1;
            true
        } else {
            false
        }
    }

    /// Meşru node ekle: farklı subnetlerden
    pub fn add_legitimate_node(&mut self, subnet_prefix: u8, host: u8) -> bool {
        let ip = [10, subnet_prefix, 0, host];
        if self.tracker.add(ip).is_ok() {
            self.legitimate_count += 1;
            true
        } else {
            false
        }
    }

    pub fn attacker_count(&self) -> usize {
        self.attacker_count
    }

    pub fn legitimate_count(&self) -> usize {
        self.legitimate_count
    }

    /// Attacker'ların toplam node içindeki oranı
    pub fn attacker_ratio(&self) -> f64 {
        let total = self.tracker.total_nodes();
        if total == 0 {
            return 0.0;
        }
        self.attacker_count as f64 / total as f64
    }
}

// ─── Replay Attack Senaryosu (Görev 83) ──────────────────────────────────────

/// Bir mesajı tanımlayan anahtar: (gönderen, nonce, slot_penceresi)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageKey {
    pub sender: [u8; 32],
    pub nonce: u64,
    /// Slot penceresi (geçerlilik aralığı)
    pub valid_until_slot: u64,
}

/// Replay koruması: görülen mesajları takip et
pub struct ReplayProtector {
    seen: HashSet<MessageKey>,
    current_slot: u64,
    /// Pencere dışı mesajlar reddedilir (geçmiş güvenlik penceresi)
    window_slots: u64,
}

impl ReplayProtector {
    pub const DEFAULT_WINDOW_SLOTS: u64 = 100;

    pub fn new(current_slot: u64) -> Self {
        Self {
            seen: HashSet::new(),
            current_slot,
            window_slots: Self::DEFAULT_WINDOW_SLOTS,
        }
    }

    pub fn advance_slot(&mut self, slot: u64) {
        self.current_slot = slot;
        // Süresi geçmiş mesajları temizle
        self.seen.retain(|k| k.valid_until_slot >= self.current_slot);
    }

    /// Mesajı kabul et. Daha önce görüldüyse veya süresi geçtiyse Err döner.
    pub fn accept(&mut self, key: MessageKey) -> Result<(), ReplayError> {
        // Slot penceresi kontrolü
        if key.valid_until_slot < self.current_slot {
            return Err(ReplayError::Expired {
                valid_until: key.valid_until_slot,
                current: self.current_slot,
            });
        }

        // Çok ilerideki slot'u reddet (saldırı önleme)
        if key.valid_until_slot > self.current_slot + self.window_slots {
            return Err(ReplayError::TooFarInFuture);
        }

        // Daha önce görüldü mü?
        if self.seen.contains(&key) {
            return Err(ReplayError::Duplicate);
        }

        self.seen.insert(key);
        Ok(())
    }

    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
    Duplicate,
    Expired { valid_until: u64, current: u64 },
    TooFarInFuture,
}

// ─── P2P DoS Saldırı Senaryosu (Görev 84) ────────────────────────────────────

/// Mesaj itibar değişimi
#[derive(Debug, Clone)]
pub struct ReputationDelta(pub i32);

/// Basit itibar tabanlı rate limiter
/// PeerStore'daki itibar yönetimini simüle eder (nova-network::peer::store)
pub struct DosProtector {
    peer_reputation: HashMap<[u8; 32], i32>,
    peer_message_count: HashMap<[u8; 32], u64>,
    /// Tek slot'ta izin verilen maksimum mesaj sayısı
    max_msgs_per_slot: u64,
    /// Yasak eşiği (bu altı → banned)
    ban_threshold: i32,
    banned: HashSet<[u8; 32]>,
    /// Toplam reddedilen mesaj sayısı
    rejected: Arc<AtomicU64>,
    current_slot: u64,
}

impl DosProtector {
    pub const DEFAULT_MAX_MSGS_PER_SLOT: u64 = 100;
    pub const DEFAULT_BAN_THRESHOLD: i32 = -500;

    pub fn new() -> Self {
        Self {
            peer_reputation: HashMap::new(),
            peer_message_count: HashMap::new(),
            max_msgs_per_slot: Self::DEFAULT_MAX_MSGS_PER_SLOT,
            ban_threshold: Self::DEFAULT_BAN_THRESHOLD,
            banned: HashSet::new(),
            rejected: Arc::new(AtomicU64::new(0)),
            current_slot: 0,
        }
    }

    /// Yeni slot'a geç — sayaçları sıfırla
    pub fn advance_slot(&mut self, slot: u64) {
        self.current_slot = slot;
        self.peer_message_count.clear();
    }

    /// Mesaj gönder. Rate limit veya ban durumunda Err döner.
    pub fn accept_message(&mut self, peer_id: [u8; 32]) -> Result<(), DosError> {
        if self.banned.contains(&peer_id) {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(DosError::PeerBanned);
        }

        let count = self.peer_message_count.entry(peer_id).or_insert(0);
        if *count >= self.max_msgs_per_slot {
            // Rate limit aşıldı → itibar düşür
            let rep = self.peer_reputation.entry(peer_id).or_insert(0);
            *rep -= 50; // Hızlı gönderim cezası
            if *rep <= self.ban_threshold {
                self.banned.insert(peer_id);
            }
            self.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(DosError::RateLimitExceeded { current_slot: self.current_slot });
        }

        *count += 1;
        Ok(())
    }

    /// Kötü mesaj → itibar düşür
    pub fn report_bad_message(&mut self, peer_id: [u8; 32], penalty: i32) {
        let rep = self.peer_reputation.entry(peer_id).or_insert(0);
        *rep -= penalty;
        if *rep <= self.ban_threshold {
            self.banned.insert(peer_id);
        }
    }

    pub fn is_banned(&self, peer_id: &[u8; 32]) -> bool {
        self.banned.contains(peer_id)
    }

    pub fn reputation(&self, peer_id: &[u8; 32]) -> i32 {
        self.peer_reputation.get(peer_id).copied().unwrap_or(0)
    }

    pub fn total_rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    pub fn banned_count(&self) -> usize {
        self.banned.len()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DosError {
    PeerBanned,
    RateLimitExceeded { current_slot: u64 },
}
