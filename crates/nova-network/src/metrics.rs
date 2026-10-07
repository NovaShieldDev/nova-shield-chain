//! P2P ağ metrikleri — AŞAMA 10 Görev 70-75
//!
//! ## Görev 70: P2P mesaj metrikleri (validation_time, handling_time)
//! ## Görev 71: HandlerRegistry (topic başına çoklu handler zinciri)
//! ## Görev 72: ValidatorRegistry (mesaj doğrulama zinciri)
//! ## Görev 73: Graceful shutdown (context.Done() pattern)
//! ## Görev 74: Sybil koruması (/24 subnet IP limiti)
//! ## Görev 75: 3-katmanlı health (readiness/health/liveness)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

// ─── GÖREV 70: P2P MESAJ METRİKLERİ ─────────────────────────────────────────

/// Tek bir mesaj tipi için metrik kaydı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetrics {
    /// Toplam alınan mesaj sayısı.
    pub received: u64,
    /// Doğrulama başarısız mesaj sayısı.
    pub validation_failed: u64,
    /// İşleme başarısız mesaj sayısı.
    pub handling_failed: u64,
    /// Toplam doğrulama süresi (mikrosaniye).
    pub total_validation_us: u64,
    /// Toplam işleme süresi (mikrosaniye).
    pub total_handling_us: u64,
}

impl MessageMetrics {
    pub fn new() -> Self { Self::default() }

    /// Ortalama doğrulama süresi (mikrosaniye).
    pub fn avg_validation_us(&self) -> u64 {
        if self.received == 0 { 0 } else { self.total_validation_us / self.received }
    }

    /// Ortalama işleme süresi (mikrosaniye).
    pub fn avg_handling_us(&self) -> u64 {
        if self.received == 0 { 0 } else { self.total_handling_us / self.received }
    }
}

impl Default for MessageMetrics {
    fn default() -> Self {
        Self {
            received: 0,
            validation_failed: 0,
            handling_failed: 0,
            total_validation_us: 0,
            total_handling_us: 0,
        }
    }
}

/// P2P mesaj metrik toplayıcı.
///
/// Prometheus'a export edilmek üzere tasarlanmış — topic başına metrik.
#[derive(Debug, Default)]
pub struct P2pMetricsCollector {
    metrics: Mutex<HashMap<String, MessageMetrics>>,
    /// Toplam bant genişliği (byte).
    total_bytes_received: AtomicU64,
    total_bytes_sent: AtomicU64,
}

impl P2pMetricsCollector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Mesaj alımını kaydet.
    pub fn record_received(&self, topic: &str, bytes: u64) {
        self.total_bytes_received.fetch_add(bytes, Ordering::Relaxed);
        let mut m = self.metrics.lock().unwrap();
        m.entry(topic.to_string()).or_default().received += 1;
    }

    /// Doğrulama süresini kaydet.
    pub fn record_validation(&self, topic: &str, duration: Duration, success: bool) {
        let us = duration.as_micros() as u64;
        let mut m = self.metrics.lock().unwrap();
        let entry = m.entry(topic.to_string()).or_default();
        entry.total_validation_us += us;
        if !success { entry.validation_failed += 1; }
    }

    /// İşleme süresini kaydet.
    pub fn record_handling(&self, topic: &str, duration: Duration, success: bool) {
        let us = duration.as_micros() as u64;
        let mut m = self.metrics.lock().unwrap();
        let entry = m.entry(topic.to_string()).or_default();
        entry.total_handling_us += us;
        if !success { entry.handling_failed += 1; }
    }

    pub fn record_sent(&self, bytes: u64) {
        self.total_bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn get(&self, topic: &str) -> Option<MessageMetrics> {
        self.metrics.lock().unwrap().get(topic).cloned()
    }

    pub fn total_bytes_received(&self) -> u64 { self.total_bytes_received.load(Ordering::Relaxed) }
    pub fn total_bytes_sent(&self) -> u64 { self.total_bytes_sent.load(Ordering::Relaxed) }

    /// Prometheus text format export.
    pub fn prometheus_export(&self) -> String {
        let mut out = String::new();
        let m = self.metrics.lock().unwrap();
        for (topic, metrics) in m.iter() {
            out.push_str(&format!(
                "nova_p2p_messages_received{{topic=\"{}\"}} {}\n",
                topic, metrics.received
            ));
            out.push_str(&format!(
                "nova_p2p_validation_failed{{topic=\"{}\"}} {}\n",
                topic, metrics.validation_failed
            ));
            out.push_str(&format!(
                "nova_p2p_avg_validation_us{{topic=\"{}\"}} {}\n",
                topic, metrics.avg_validation_us()
            ));
            out.push_str(&format!(
                "nova_p2p_avg_handling_us{{topic=\"{}\"}} {}\n",
                topic, metrics.avg_handling_us()
            ));
        }
        out.push_str(&format!(
            "nova_p2p_bytes_received_total {}\n",
            self.total_bytes_received()
        ));
        out.push_str(&format!(
            "nova_p2p_bytes_sent_total {}\n",
            self.total_bytes_sent()
        ));
        out
    }
}

// ─── GÖREV 71: HANDLER REGISTRY ──────────────────────────────────────────────

/// Mesaj işleyici trait'i.
pub trait MessageHandler: Send + Sync {
    /// Mesajı işle. `true` = başarılı, `false` = işleme başarısız.
    fn handle(&self, topic: &str, data: &[u8]) -> bool;
    fn name(&self) -> &'static str;
}

/// Topic başına handler zinciri.
///
/// Avalanchego `HandlerDescriptor` pattern:
/// Her topic için birden fazla handler kayıtlı olabilir.
/// Hepsi sırayla çağrılır (short-circuit yok).
#[derive(Default)]
pub struct HandlerRegistry {
    handlers: Mutex<HashMap<String, Vec<Arc<dyn MessageHandler>>>>,
}

impl HandlerRegistry {
    pub fn new() -> Self { Self::default() }

    /// Topic için handler kaydet.
    pub fn register(&self, topic: &str, handler: Arc<dyn MessageHandler>) {
        self.handlers.lock().unwrap()
            .entry(topic.to_string())
            .or_default()
            .push(handler);
    }

    /// Topic'in tüm handler'larını çağır.
    ///
    /// Returns: (başarılı sayısı, toplam sayısı)
    pub fn dispatch(&self, topic: &str, data: &[u8]) -> (usize, usize) {
        let m = self.handlers.lock().unwrap();
        let handlers = match m.get(topic) {
            Some(h) => h.clone(),
            None => return (0, 0),
        };
        drop(m);

        let total = handlers.len();
        let success = handlers.iter().filter(|h| h.handle(topic, data)).count();
        (success, total)
    }

    /// Kayıtlı topic sayısı.
    pub fn topic_count(&self) -> usize {
        self.handlers.lock().unwrap().len()
    }

    /// Topic için kayıtlı handler sayısı.
    pub fn handler_count(&self, topic: &str) -> usize {
        self.handlers.lock().unwrap().get(topic).map(|v| v.len()).unwrap_or(0)
    }
}

// ─── GÖREV 72: VALIDATOR REGISTRY (MESAJ DOĞRULAMA ZİNCİRİ) ─────────────────

/// Mesaj doğrulayıcı trait'i.
pub trait MessageValidator: Send + Sync {
    /// Mesajı doğrula. `Ok(())` = geçerli, `Err(String)` = red sebebi.
    fn validate(&self, topic: &str, data: &[u8]) -> Result<(), String>;
    fn name(&self) -> &'static str;
}

/// Mesaj doğrulama zinciri.
///
/// Herhangi bir validator reddederse mesaj işlenmez.
#[derive(Default)]
pub struct ValidatorRegistry {
    validators: Mutex<Vec<Arc<dyn MessageValidator>>>,
}

impl ValidatorRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&self, validator: Arc<dyn MessageValidator>) {
        self.validators.lock().unwrap().push(validator);
    }

    /// Tüm validatorları uygula.
    pub fn validate_all(&self, topic: &str, data: &[u8]) -> Result<(), String> {
        let validators = self.validators.lock().unwrap().clone();
        for v in &validators {
            v.validate(topic, data)?;
        }
        Ok(())
    }

    pub fn validator_count(&self) -> usize {
        self.validators.lock().unwrap().len()
    }
}

// ─── GÖREV 73: GRACEFUL SHUTDOWN ─────────────────────────────────────────────

/// Graceful shutdown context — Go'nun `context.Done()` pattern'ı.
///
/// Avalanchego tüm goroutine'ler bu context'i dinler.
/// Nova'da Arc<ShutdownContext> paylaşılır.
#[derive(Debug)]
pub struct ShutdownContext {
    cancelled: AtomicBool,
}

impl ShutdownContext {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { cancelled: AtomicBool::new(false) })
    }

    /// Shutdown sinyali gönder.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Shutdown sinyali geldi mi?
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Shutdown bekle (busy-wait — gerçekte tokio::select! kullanılır).
    pub fn wait_for_cancel(&self) {
        while !self.is_cancelled() {
            std::hint::spin_loop();
        }
    }
}

impl Default for ShutdownContext {
    fn default() -> Self {
        Self { cancelled: AtomicBool::new(false) }
    }
}

// ─── GÖREV 74: SYBIL KORUMASI — /24 SUBNET IP LİMİTİ ────────────────────────

/// /24 subnet IP limiti — Sybil saldırı koruması.
///
/// Kademlia (go-libp2p-kad-dht, avalanchego):
/// - `bucketIPLimit = 2`: Aynı /24 subnet'ten bir bucket'ta max 2 peer
/// - `tableIPLimit = 10`: Tüm tabloda aynı /24'ten max 10 peer
///
/// Nova'da daha agresif: `bucketIPLimit=2`, `tableIPLimit=10`
pub const BUCKET_IP_LIMIT: usize = 2;
pub const TABLE_IP_LIMIT: usize = 10;

/// IP tabanlı Sybil koruma yöneticisi.
#[derive(Debug, Default)]
pub struct SubnetIpLimiter {
    /// /24 prefix → peer sayısı
    subnet_counts: Mutex<HashMap<[u8; 3], usize>>,
}

impl SubnetIpLimiter {
    pub fn new() -> Self { Self::default() }

    /// IPv4 adresinden /24 prefix çıkar.
    fn subnet_prefix(ip: &str) -> Option<[u8; 3]> {
        let parts: Vec<&str> = ip.split(':').next()?.split('.').collect();
        if parts.len() != 4 { return None; }
        let a: u8 = parts[0].parse().ok()?;
        let b: u8 = parts[1].parse().ok()?;
        let c: u8 = parts[2].parse().ok()?;
        Some([a, b, c])
    }

    /// Bu IP'yi tabloya ekleyebilir miyiz?
    pub fn can_add(&self, ip_addr: &str) -> bool {
        let Some(prefix) = Self::subnet_prefix(ip_addr) else { return true };
        let counts = self.subnet_counts.lock().unwrap();
        let count = counts.get(&prefix).copied().unwrap_or(0);
        count < TABLE_IP_LIMIT
    }

    /// IP'yi tabloya ekle.
    pub fn add(&self, ip_addr: &str) -> bool {
        let Some(prefix) = Self::subnet_prefix(ip_addr) else { return true };
        let mut counts = self.subnet_counts.lock().unwrap();
        let count = counts.entry(prefix).or_insert(0);
        if *count >= TABLE_IP_LIMIT {
            return false;
        }
        *count += 1;
        true
    }

    /// IP'yi tablodan çıkar.
    pub fn remove(&self, ip_addr: &str) {
        let Some(prefix) = Self::subnet_prefix(ip_addr) else { return };
        let mut counts = self.subnet_counts.lock().unwrap();
        if let Some(c) = counts.get_mut(&prefix) {
            *c = c.saturating_sub(1);
            if *c == 0 { counts.remove(&prefix); }
        }
    }

    /// Bir subnet'teki mevcut peer sayısı.
    pub fn subnet_count(&self, ip_addr: &str) -> usize {
        let Some(prefix) = Self::subnet_prefix(ip_addr) else { return 0 };
        self.subnet_counts.lock().unwrap().get(&prefix).copied().unwrap_or(0)
    }
}

// ─── GÖREV 75: 3-KATMANLI HEALTH ─────────────────────────────────────────────

/// Health durumu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Hazır — trafik kabul eder.
    Healthy,
    /// Degraded — çalışıyor ama sorun var.
    Degraded { reason: String },
    /// Sağlıksız — servis dışı.
    Unhealthy { reason: String },
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool { matches!(self, HealthStatus::Healthy) }

    pub fn http_status_code(&self) -> u16 {
        match self {
            HealthStatus::Healthy => 200,
            HealthStatus::Degraded { .. } => 200, // Trafik alır ama uyarı
            HealthStatus::Unhealthy { .. } => 503,
        }
    }
}

/// 3-katmanlı health kontrol sistemi.
///
/// Kubernetes / Avalanchego pattern:
/// - **Readiness**: Trafik almaya hazır mı? (startup tamamlandı mı?)
/// - **Health**: Genel sağlık (peer bağlantısı, senkronizasyon)
/// - **Liveness**: Canlı mı? (deadlock/crash tespiti)
#[derive(Debug)]
pub struct HealthChecker {
    /// Readiness: startup tamamlandı mı?
    is_ready: AtomicBool,
    /// Liveness: son heartbeat zamanı
    last_heartbeat: Mutex<Option<Instant>>,
    /// Liveness timeout (bu süre geçerse dead sayılır)
    liveness_timeout: Duration,
    /// Minimum peer sayısı (health için)
    min_peers: usize,
}

impl HealthChecker {
    pub fn new(min_peers: usize, liveness_timeout: Duration) -> Self {
        Self {
            is_ready: AtomicBool::new(false),
            last_heartbeat: Mutex::new(None),
            liveness_timeout,
            min_peers,
        }
    }

    pub fn set_ready(&self) {
        self.is_ready.store(true, Ordering::SeqCst);
    }

    pub fn heartbeat(&self) {
        *self.last_heartbeat.lock().unwrap() = Some(Instant::now());
    }

    /// Readiness kontrol — trafik almaya hazır mı?
    pub fn check_readiness(&self) -> HealthStatus {
        if self.is_ready.load(Ordering::SeqCst) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy { reason: "startup tamamlanmadı".into() }
        }
    }

    /// Health kontrol — genel sağlık.
    pub fn check_health(&self, connected_peers: usize) -> HealthStatus {
        if !self.is_ready.load(Ordering::SeqCst) {
            return HealthStatus::Unhealthy { reason: "hazır değil".into() };
        }
        if connected_peers < self.min_peers {
            return HealthStatus::Degraded {
                reason: format!(
                    "yetersiz peer: {}/{}", connected_peers, self.min_peers
                ),
            };
        }
        HealthStatus::Healthy
    }

    /// Liveness kontrol — deadlock/crash tespiti.
    pub fn check_liveness(&self) -> HealthStatus {
        let hb = self.last_heartbeat.lock().unwrap();
        match *hb {
            None => HealthStatus::Unhealthy { reason: "hiç heartbeat yok".into() },
            Some(t) if t.elapsed() > self.liveness_timeout => {
                HealthStatus::Unhealthy {
                    reason: format!(
                        "heartbeat timeout: {:?} > {:?}",
                        t.elapsed(), self.liveness_timeout
                    ),
                }
            }
            _ => HealthStatus::Healthy,
        }
    }

    /// Tam health raporu.
    pub fn full_report(&self, connected_peers: usize) -> HealthReport {
        HealthReport {
            readiness: self.check_readiness(),
            health: self.check_health(connected_peers),
            liveness: self.check_liveness(),
        }
    }
}

/// Tam health raporu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub readiness: HealthStatus,
    pub health: HealthStatus,
    pub liveness: HealthStatus,
}

impl HealthReport {
    pub fn all_healthy(&self) -> bool {
        self.readiness.is_healthy() && self.health.is_healthy() && self.liveness.is_healthy()
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    // ─── Görev 70: Metrik testleri ────────────────────────────────────────────

    #[test]
    fn test_metrics_record_and_avg() {
        let col = P2pMetricsCollector::new();
        col.record_received("gossip", 1024);
        col.record_received("gossip", 512);
        col.record_validation("gossip", Duration::from_micros(100), true);
        col.record_validation("gossip", Duration::from_micros(200), false);
        col.record_handling("gossip", Duration::from_micros(50), true);

        let m = col.get("gossip").unwrap();
        assert_eq!(m.received, 2);
        assert_eq!(m.validation_failed, 1);
        assert_eq!(m.avg_validation_us(), 150); // (100+200)/2
        assert_eq!(col.total_bytes_received(), 1536);
    }

    #[test]
    fn test_prometheus_export_format() {
        let col = P2pMetricsCollector::new();
        col.record_received("block", 2048);
        let export = col.prometheus_export();
        assert!(export.contains("nova_p2p_messages_received{topic=\"block\"} 1"));
        assert!(export.contains("nova_p2p_bytes_received_total"));
    }

    // ─── Görev 71: HandlerRegistry testleri ──────────────────────────────────

    struct CountHandler(Arc<AtomicUsize>);
    impl MessageHandler for CountHandler {
        fn handle(&self, _: &str, _: &[u8]) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst);
            true
        }
        fn name(&self) -> &'static str { "CountHandler" }
    }

    #[test]
    fn test_handler_registry_multiple_handlers() {
        let reg = HandlerRegistry::new();
        let count = Arc::new(AtomicUsize::new(0));
        reg.register("tx", Arc::new(CountHandler(count.clone())));
        reg.register("tx", Arc::new(CountHandler(count.clone())));
        reg.register("block", Arc::new(CountHandler(count.clone())));

        assert_eq!(reg.handler_count("tx"), 2);
        let (success, total) = reg.dispatch("tx", b"data");
        assert_eq!(total, 2);
        assert_eq!(success, 2);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_handler_registry_unknown_topic() {
        let reg = HandlerRegistry::new();
        let (success, total) = reg.dispatch("unknown", b"data");
        assert_eq!(total, 0);
        assert_eq!(success, 0);
    }

    // ─── Görev 72: ValidatorRegistry testleri ────────────────────────────────

    struct SizeValidator(usize);
    impl MessageValidator for SizeValidator {
        fn validate(&self, _: &str, data: &[u8]) -> Result<(), String> {
            if data.len() > self.0 {
                Err(format!("çok büyük: {} > {}", data.len(), self.0))
            } else { Ok(()) }
        }
        fn name(&self) -> &'static str { "SizeValidator" }
    }

    #[test]
    fn test_validator_registry_all_pass() {
        let reg = ValidatorRegistry::new();
        reg.register(Arc::new(SizeValidator(100)));
        assert!(reg.validate_all("tx", b"small data").is_ok());
    }

    #[test]
    fn test_validator_registry_one_fails() {
        let reg = ValidatorRegistry::new();
        reg.register(Arc::new(SizeValidator(5)));
        reg.register(Arc::new(SizeValidator(100)));
        let err = reg.validate_all("tx", b"too large data here");
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("çok büyük"));
    }

    // ─── Görev 73: ShutdownContext testleri ──────────────────────────────────

    #[test]
    fn test_shutdown_context() {
        let ctx = ShutdownContext::new();
        assert!(!ctx.is_cancelled());
        ctx.cancel();
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn test_shutdown_context_shared() {
        let ctx = ShutdownContext::new();
        let ctx2 = ctx.clone();
        ctx.cancel();
        assert!(ctx2.is_cancelled());
    }

    // ─── Görev 74: SubnetIpLimiter testleri ──────────────────────────────────

    #[test]
    fn test_subnet_limiter_allows_up_to_limit() {
        let limiter = SubnetIpLimiter::new();
        // Aynı /24 subnet'ten TABLE_IP_LIMIT=10 peer
        for i in 1..=TABLE_IP_LIMIT {
            let ip = format!("192.168.1.{}:30333", i);
            assert!(limiter.add(&ip), "Peer {} eklenebilmeli", i);
        }
        // 11. peer reddedilmeli
        assert!(!limiter.add("192.168.1.100:30333"));
    }

    #[test]
    fn test_subnet_limiter_different_subnets() {
        let limiter = SubnetIpLimiter::new();
        // Farklı subnet'ler bağımsız
        for i in 1..=TABLE_IP_LIMIT {
            limiter.add(&format!("10.0.1.{}:30333", i));
            limiter.add(&format!("10.0.2.{}:30333", i));
        }
        // Her subnet kendi limitinde
        assert!(!limiter.add("10.0.1.200:30333")); // dolu
        assert!(!limiter.add("10.0.2.200:30333")); // dolu
        assert!(limiter.add("10.0.3.1:30333"));    // yeni subnet → ok
    }

    #[test]
    fn test_subnet_limiter_remove() {
        let limiter = SubnetIpLimiter::new();
        for i in 1..=TABLE_IP_LIMIT {
            limiter.add(&format!("192.168.1.{}:30333", i));
        }
        assert!(!limiter.can_add("192.168.1.200:30333")); // dolu
        limiter.remove("192.168.1.1:30333"); // 1 çıkar
        assert!(limiter.can_add("192.168.1.200:30333")); // şimdi eklenebilir
    }

    // ─── Görev 75: HealthChecker testleri ────────────────────────────────────

    #[test]
    fn test_health_not_ready() {
        let hc = HealthChecker::new(3, Duration::from_secs(30));
        assert!(!hc.check_readiness().is_healthy());
    }

    #[test]
    fn test_health_ready_enough_peers() {
        let hc = HealthChecker::new(3, Duration::from_secs(30));
        hc.set_ready();
        hc.heartbeat();
        assert!(hc.check_health(5).is_healthy());
    }

    #[test]
    fn test_health_degraded_low_peers() {
        let hc = HealthChecker::new(3, Duration::from_secs(30));
        hc.set_ready();
        hc.heartbeat();
        let status = hc.check_health(1);
        assert!(matches!(status, HealthStatus::Degraded { .. }));
    }

    #[test]
    fn test_liveness_no_heartbeat() {
        let hc = HealthChecker::new(3, Duration::from_secs(30));
        assert!(!hc.check_liveness().is_healthy());
    }

    #[test]
    fn test_liveness_with_heartbeat() {
        let hc = HealthChecker::new(3, Duration::from_secs(30));
        hc.heartbeat();
        assert!(hc.check_liveness().is_healthy());
    }

    #[test]
    fn test_full_health_report() {
        let hc = HealthChecker::new(2, Duration::from_secs(30));
        hc.set_ready();
        hc.heartbeat();
        let report = hc.full_report(5);
        assert!(report.all_healthy());
    }
}
