//! RPC katmanı rate limiter — IP başına token bucket
//!
//! ## Güvenlik Modeli
//!
//! Her istemci IP'si için sabit-pencere (fixed-window) sayacı tutulur.
//! `BURST_CAPACITY` aşılınca `check_and_record()` `false` döner;
//! server katmanı HTTP 429 Too Many Requests yanıtı verir.
//!
//! ## Parametreler
//!
//! | Parametre          | Değer | Açıklama                        |
//! |--------------------|-------|---------------------------------|
//! | `WINDOW_SECS`      | 1     | Pencere süresi (saniye)         |
//! | `MAX_REQ_PER_WIN`  | 100   | Normal op eşiği (req/s)         |
//! | `BURST_CAPACITY`   | 500   | Ani yük toleransı               |
//!
//! ## Tehdit Modeli
//!
//! - **DoS / DDoS**: Tek IP'den binlerce istek → rate limit devreye girer.
//! - **Brute-force RPC**: `nova_sendTransaction` spam → zincir korunur.
//! - **Sybil RPC**: Her IP bağımsız sayılır; aynı arka plan IP paylaşımı
//!   için `X-Real-IP` / `X-Forwarded-For` desteği vardır.
//!
//! ## Thread Safety
//!
//! `DashMap` (64 shard) ile lock-free concurrent erişim sağlanır.
//! `Clone` derive edilmiştir: axum `State<ServerState>` uyumu için.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Zaman penceresi (saniye)
const WINDOW_SECS: u64 = 1;

/// Saniyede izin verilen normal istek sayısı
pub const MAX_REQUESTS_PER_WINDOW: u64 = 100;

/// Burst kapasitesi — tek pencerede mutlak maksimum
pub const BURST_CAPACITY: u64 = 500;

/// Stale girişleri temizleme eşiği (saniye)
const CLEANUP_TTL_SECS: u64 = 300;

// ─── IpState ──────────────────────────────────────────────────────────────────

/// IP başına pencere durumu
struct IpState {
    /// Bu penceredeki toplam istek sayısı
    count: u64,
    /// Mevcut pencerenin başlangıç anı
    window_start: Instant,
}

// ─── RateLimiter ──────────────────────────────────────────────────────────────

/// Token bucket rate limiter
///
/// `Arc<DashMap>` ile sarılıdır — `.clone()` ucuz ve state paylaşımlıdır.
#[derive(Clone)]
pub struct RateLimiter {
    states: Arc<DashMap<String, IpState>>,
}

impl RateLimiter {
    /// Yeni limiter oluştur
    pub fn new() -> Self {
        Self { states: Arc::new(DashMap::with_capacity_and_shard_amount(256, 64)) }
    }

    /// İsteği kaydet ve izin ver / reddet
    ///
    /// # Dönüş
    /// - `true`  → istek kabul edildi
    /// - `false` → burst kapasitesi aşıldı → HTTP 429 dön
    pub fn check_and_record(&self, ip: &str) -> bool {
        let now = Instant::now();

        let mut entry = self.states.entry(ip.to_string()).or_insert_with(|| IpState {
            count: 0,
            window_start: now,
        });

        let elapsed = now.duration_since(entry.window_start).as_secs();

        if elapsed >= WINDOW_SECS {
            // Yeni pencere — sayacı sıfırla
            entry.count = 1;
            entry.window_start = now;
            return true;
        }

        entry.count += 1;
        entry.count <= BURST_CAPACITY
    }

    /// Belirli IP'nin mevcut istek sayısı (monitoring / test)
    pub fn request_count(&self, ip: &str) -> u64 {
        self.states.get(ip).map(|s| s.count).unwrap_or(0)
    }

    /// TTL süresi dolmuş eski girişleri temizle
    ///
    /// Periyodik arka plan görevinde çağrılır.
    pub fn cleanup_stale(&self) {
        let now = Instant::now();
        self.states.retain(|_, v| {
            now.duration_since(v.window_start).as_secs() < CLEANUP_TTL_SECS
        });
    }

    /// Tüm durumu sıfırla (test ortamı için)
    pub fn reset(&self) {
        self.states.clear();
    }

    /// Şu an aktif takip edilen IP sayısı
    pub fn tracked_ip_count(&self) -> usize {
        self.states.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self { Self::new() }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Normal trafik ─────────────────────────────────────────────────────────

    #[test]
    fn test_allows_normal_traffic_under_limit() {
        let rl = RateLimiter::new();
        for i in 0..MAX_REQUESTS_PER_WINDOW {
            assert!(rl.check_and_record("192.168.1.1"), "istek {} reddedilmemeli", i);
        }
    }

    #[test]
    fn test_allows_up_to_burst_capacity() {
        let rl = RateLimiter::new();
        for i in 0..BURST_CAPACITY {
            assert!(
                rl.check_and_record("10.0.0.1"),
                "burst kapasitesine kadar istek {} izin verilmeli", i
            );
        }
    }

    // ── Rate limit tetikleme ──────────────────────────────────────────────────

    #[test]
    fn test_blocks_after_burst_capacity_exceeded() {
        let rl = RateLimiter::new();
        for _ in 0..BURST_CAPACITY {
            rl.check_and_record("attacker");
        }
        // 1 fazla istek reddedilmeli
        assert!(
            !rl.check_and_record("attacker"),
            "burst sonrası istek reddedilmeli — DoS koruması aktif olmalı"
        );
    }

    #[test]
    fn test_blocks_sustained_flood() {
        let rl = RateLimiter::new();
        let flood_count = BURST_CAPACITY * 10;
        let mut blocked = 0u64;

        for _ in 0..flood_count {
            if !rl.check_and_record("flood_attacker") {
                blocked += 1;
            }
        }

        // Flood'un büyük kısmı bloklanmalı
        assert!(
            blocked > flood_count / 2,
            "flood saldırısında en az %50 bloklanmalı, blok: {blocked}/{flood_count}"
        );
    }

    // ── IP izolasyonu ─────────────────────────────────────────────────────────

    #[test]
    fn test_different_ips_are_independent() {
        let rl = RateLimiter::new();

        // IP A'yı flood et
        for _ in 0..=BURST_CAPACITY {
            rl.check_and_record("1.1.1.1");
        }
        assert!(!rl.check_and_record("1.1.1.1"), "IP A bloklanmalı");

        // IP B bağımsız olmalı
        assert!(rl.check_and_record("2.2.2.2"), "IP B'nin IP A ile ilişkisi yok");
        assert!(rl.check_and_record("3.3.3.3"), "IP C'nin IP A ile ilişkisi yok");
    }

    #[test]
    fn test_localhost_treated_as_separate_ip() {
        let rl = RateLimiter::new();
        // localhost (testnet validator, RPC UI) normal istek yapabilmeli
        assert!(rl.check_and_record("127.0.0.1"));
        assert!(rl.check_and_record("::1")); // IPv6 localhost
    }

    // ── Sayaç takibi ──────────────────────────────────────────────────────────

    #[test]
    fn test_request_count_tracking() {
        let rl = RateLimiter::new();
        rl.check_and_record("5.5.5.5");
        rl.check_and_record("5.5.5.5");
        rl.check_and_record("5.5.5.5");
        assert_eq!(rl.request_count("5.5.5.5"), 3);
    }

    #[test]
    fn test_unknown_ip_count_zero() {
        let rl = RateLimiter::new();
        assert_eq!(rl.request_count("9.9.9.9"), 0);
    }

    // ── Yönetim ───────────────────────────────────────────────────────────────

    #[test]
    fn test_reset_clears_all_state() {
        let rl = RateLimiter::new();
        for _ in 0..=BURST_CAPACITY {
            rl.check_and_record("victim");
        }
        assert!(!rl.check_and_record("victim"), "reset öncesi bloklanmalı");

        rl.reset();

        assert!(rl.check_and_record("victim"), "reset sonrası tekrar izin verilmeli");
        assert_eq!(rl.tracked_ip_count(), 1);
    }

    #[test]
    fn test_tracked_ip_count() {
        let rl = RateLimiter::new();
        rl.check_and_record("ip1");
        rl.check_and_record("ip2");
        rl.check_and_record("ip3");
        assert_eq!(rl.tracked_ip_count(), 3);
    }

    // ── Clone paylaşımlı state ────────────────────────────────────────────────

    #[test]
    fn test_clone_shares_underlying_state() {
        let rl1 = RateLimiter::new();
        let rl2 = rl1.clone();

        rl1.check_and_record("shared_ip");
        rl1.check_and_record("shared_ip");

        // rl2 aynı state'i görmeli (Arc paylaşımı)
        assert_eq!(rl2.request_count("shared_ip"), 2);
    }

    // ── Saldırı simülasyonu ───────────────────────────────────────────────────

    #[test]
    fn test_poc_rpc_dos_attack_simulation() {
        // PoC: Bir saldırganın saniyede 10_000 RPC isteği gönderdiği senaryo
        // Beklenti: BURST_CAPACITY sonrası istekler reddedilir
        let rl = RateLimiter::new();
        let attacker_ip = "evil_node";
        let attack_requests = 10_000u64;

        let mut allowed = 0u64;
        let mut blocked = 0u64;

        for _ in 0..attack_requests {
            if rl.check_and_record(attacker_ip) {
                allowed += 1;
            } else {
                blocked += 1;
            }
        }

        // Sadece BURST_CAPACITY kadar istek izin verilmeli
        assert_eq!(
            allowed, BURST_CAPACITY,
            "DoS koruması: sadece {} istek geçmeli, {} geçti", BURST_CAPACITY, allowed
        );
        assert_eq!(
            blocked,
            attack_requests - BURST_CAPACITY,
            "{} istek bloklanmalıydı", attack_requests - BURST_CAPACITY
        );
    }

    #[test]
    fn test_poc_legitimate_user_not_affected_by_attacker() {
        // PoC: Saldırgan rate-limit'i tetiklese bile diğer kullanıcılar etkilenmemeli
        let rl = RateLimiter::new();

        // Saldırgan burst kapasitesini doldurur
        for _ in 0..=BURST_CAPACITY {
            rl.check_and_record("attacker_ip");
        }

        // Meşru kullanıcı etkilenmemeli
        assert!(
            rl.check_and_record("legitimate_user"),
            "meşru kullanıcı saldırgandan bağımsız olmalı"
        );
        assert!(
            rl.check_and_record("another_user"),
            "diğer kullanıcılar da etkilenmemeli"
        );
    }
}
