//! Deterministik in-memory ağ simülatörü.
//!
//! Desteklenen özellikler:
//! - Mesaj düşürme (drop rate bps)
//! - Ağ bölme (partition: grup A ↔ grup B iletişim kesilebilir)
//! - Gecikme (delayed delivery at future tick)
//! - Mesaj istatistikleri
//!
//! Kaos mühendisliği testlerinin temel altyapısı.

use std::collections::HashMap;

// ─── SimMessage ───────────────────────────────────────────────────────────────

/// Simüle edilmiş ağ mesajı.
#[derive(Debug, Clone)]
pub struct SimMessage {
    /// Gönderen node indeksi.
    pub from: usize,
    /// Alıcı node indeksi.
    pub to: usize,
    /// Mesaj verisi.
    pub data: Vec<u8>,
    /// Kaçıncı tick'te teslim edilecek.
    pub deliver_at_tick: u64,
}

// ─── PartitionConfig ─────────────────────────────────────────────────────────

/// Ağ bölme konfigürasyonu.
///
/// grup_a ↔ grup_b arasındaki mesajlar düşürülür.
#[derive(Debug, Clone)]
pub struct PartitionConfig {
    /// Bölme A'daki node indeksleri.
    pub group_a: Vec<usize>,
    /// Bölme B'daki node indeksleri.
    pub group_b: Vec<usize>,
}

impl PartitionConfig {
    /// İki eşit gruba böl (n node → n/2 + n/2).
    pub fn split_half(n: usize) -> Self {
        let half = n / 2;
        Self {
            group_a: (0..half).collect(),
            group_b: (half..n).collect(),
        }
    }

    /// Tek node'u izole et (1 vs n-1).
    pub fn isolate(isolated_node: usize, n: usize) -> Self {
        Self {
            group_a: vec![isolated_node],
            group_b: (0..n).filter(|&i| i != isolated_node).collect(),
        }
    }

    /// Mesajın bölme tarafından engellenmesi gerekiyor mu?
    pub fn is_blocked(&self, from: usize, to: usize) -> bool {
        let in_a = |idx: usize| self.group_a.contains(&idx);
        let in_b = |idx: usize| self.group_b.contains(&idx);
        (in_a(from) && in_b(to)) || (in_b(from) && in_a(to))
    }
}

// ─── SimNetworkStats ──────────────────────────────────────────────────────────

/// Ağ simülasyon istatistikleri.
#[derive(Debug, Clone, Default)]
pub struct SimNetworkStats {
    pub total_sent: u64,
    pub total_delivered: u64,
    pub total_dropped_rate: u64,
    pub total_dropped_partition: u64,
    pub total_delayed: u64,
}

impl SimNetworkStats {
    pub fn total_dropped(&self) -> u64 {
        self.total_dropped_rate + self.total_dropped_partition
    }

    pub fn delivery_rate_bps(&self) -> u64 {
        if self.total_sent == 0 {
            return 10_000;
        }
        self.total_delivered.saturating_mul(10_000) / self.total_sent
    }
}

// ─── SimNetwork ───────────────────────────────────────────────────────────────

/// Deterministik in-memory ağ simülatörü.
///
/// Node sayısı oluşturma sırasında belirlenir, sonradan değişmez.
/// Her node kendi inbox'ına sahiptir.
pub struct SimNetwork {
    /// Node sayısı.
    pub node_count: usize,
    /// Bekleyen mesajlar (teslim anı gelmeyen).
    pending: Vec<SimMessage>,
    /// Her node için teslim edilmiş mesajlar.
    pub inboxes: Vec<Vec<SimMessage>>,
    /// Aktif ağ bölmesi (varsa).
    pub partition: Option<PartitionConfig>,
    /// Mesaj düşürme oranı (0..10000 bps, %0..%100).
    pub drop_rate_bps: u32,
    /// Mevcut tick.
    pub current_tick: u64,
    /// Deterministik rastgele sayı üreteci durumu.
    rng_state: u64,
    /// İstatistikler.
    pub stats: SimNetworkStats,
}

impl SimNetwork {
    /// Yeni ağ simülatörü.
    pub fn new(node_count: usize) -> Self {
        Self {
            node_count,
            pending: Vec::new(),
            inboxes: vec![Vec::new(); node_count],
            partition: None,
            drop_rate_bps: 0,
            current_tick: 0,
            rng_state: 0xDEAD_BEEF_CAFE_1234,
            stats: SimNetworkStats::default(),
        }
    }

    /// Deterministik seed ile başlat.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng_state = seed;
        self
    }

    /// Mesaj düşürme oranı ayarla (0..10000 bps).
    pub fn with_drop_rate(mut self, bps: u32) -> Self {
        self.drop_rate_bps = bps.min(10_000);
        self
    }

    /// Ağ bölmesi uygula.
    pub fn apply_partition(&mut self, config: PartitionConfig) {
        self.partition = Some(config);
    }

    /// Ağ bölmesini kaldır (heal).
    pub fn heal_partition(&mut self) {
        self.partition = None;
    }

    /// Mesaj gönder (tek alıcı).
    pub fn send(&mut self, from: usize, to: usize, data: Vec<u8>) {
        self.send_delayed(from, to, data, 0);
    }

    /// Gecikmiş mesaj gönder.
    pub fn send_delayed(&mut self, from: usize, to: usize, data: Vec<u8>, delay_ticks: u64) {
        assert!(from < self.node_count, "geçersiz from: {}", from);
        assert!(to < self.node_count, "geçersiz to: {}", to);

        self.stats.total_sent += 1;

        // Bölme kontrolü
        if let Some(ref p) = self.partition {
            if p.is_blocked(from, to) {
                self.stats.total_dropped_partition += 1;
                return;
            }
        }

        // Drop rate kontrolü (deterministik LCG)
        if self.drop_rate_bps > 0 {
            let r = self.next_rng() % 10_000;
            if r < self.drop_rate_bps as u64 {
                self.stats.total_dropped_rate += 1;
                return;
            }
        }

        let deliver_at = self.current_tick + delay_ticks;
        if deliver_at == 0 || delay_ticks == 0 {
            // Anlık teslim
            self.inboxes[to].push(SimMessage { from, to, data, deliver_at_tick: self.current_tick });
            self.stats.total_delivered += 1;
        } else {
            // Gecikmeli kuyruğa ekle
            self.pending.push(SimMessage { from, to, data, deliver_at_tick: deliver_at });
            self.stats.total_delayed += 1;
        }
    }

    /// Broadcast — from dışındaki tüm node'lara gönder.
    pub fn broadcast(&mut self, from: usize, data: Vec<u8>) {
        for to in 0..self.node_count {
            if to != from {
                self.send(from, to, data.clone());
            }
        }
    }

    /// Tick ilerlet — gecikmiş mesajları teslim et.
    pub fn advance_tick(&mut self) {
        self.current_tick += 1;
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].deliver_at_tick <= self.current_tick {
                let msg = self.pending.remove(i);
                self.inboxes[msg.to].push(msg);
                self.stats.total_delivered += 1;
            } else {
                i += 1;
            }
        }
    }

    /// Bir node'un inbox'ını al ve temizle.
    pub fn drain_inbox(&mut self, node_idx: usize) -> Vec<SimMessage> {
        std::mem::take(&mut self.inboxes[node_idx])
    }

    /// Tüm inbox'lardaki toplam mesaj sayısı.
    pub fn total_inbox_len(&self) -> usize {
        self.inboxes.iter().map(|b| b.len()).sum()
    }

    /// Bekleyen (teslim edilmemiş) mesaj sayısı.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    // LCG deterministik rastgele sayı üreteci
    fn next_rng(&mut self) -> u64 {
        self.rng_state = self.rng_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.rng_state >> 33
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_send_receive() {
        let mut net = SimNetwork::new(3);
        net.send(0, 1, b"hello".to_vec());
        assert_eq!(net.inboxes[1].len(), 1);
        assert_eq!(net.inboxes[1][0].data, b"hello");
    }

    #[test]
    fn test_broadcast_not_self() {
        let mut net = SimNetwork::new(4);
        net.broadcast(0, b"block".to_vec());
        assert_eq!(net.inboxes[0].len(), 0); // kendine gönderilmez
        assert_eq!(net.inboxes[1].len(), 1);
        assert_eq!(net.inboxes[2].len(), 1);
        assert_eq!(net.inboxes[3].len(), 1);
    }

    #[test]
    fn test_partition_blocks_messages() {
        let mut net = SimNetwork::new(4);
        net.apply_partition(PartitionConfig::split_half(4));
        // 0→1 aynı grupta — geçer
        net.send(0, 1, b"ok".to_vec());
        // 0→2 farklı grupta — bloke
        net.send(0, 2, b"blocked".to_vec());
        net.send(0, 3, b"blocked".to_vec());

        assert_eq!(net.inboxes[1].len(), 1);
        assert_eq!(net.inboxes[2].len(), 0);
        assert_eq!(net.inboxes[3].len(), 0);
        assert_eq!(net.stats.total_dropped_partition, 2);
    }

    #[test]
    fn test_partition_heal() {
        let mut net = SimNetwork::new(4);
        net.apply_partition(PartitionConfig::split_half(4));
        net.send(0, 2, b"blocked".to_vec());
        assert_eq!(net.inboxes[2].len(), 0);

        net.heal_partition();
        net.send(0, 2, b"healed".to_vec());
        assert_eq!(net.inboxes[2].len(), 1);
    }

    #[test]
    fn test_delayed_message_delivery() {
        let mut net = SimNetwork::new(2);
        net.send_delayed(0, 1, b"delayed".to_vec(), 3);
        assert_eq!(net.inboxes[1].len(), 0);

        net.advance_tick(); // tick 1
        assert_eq!(net.inboxes[1].len(), 0);
        net.advance_tick(); // tick 2
        assert_eq!(net.inboxes[1].len(), 0);
        net.advance_tick(); // tick 3 — teslim
        assert_eq!(net.inboxes[1].len(), 1);
    }

    #[test]
    fn test_drop_rate_100_percent() {
        let mut net = SimNetwork::new(4).with_drop_rate(10_000);
        for _ in 0..100 {
            net.send(0, 1, b"x".to_vec());
        }
        assert_eq!(net.inboxes[1].len(), 0);
        assert_eq!(net.stats.total_dropped_rate, 100);
    }

    #[test]
    fn test_drop_rate_0_percent() {
        let mut net = SimNetwork::new(4).with_drop_rate(0);
        for _ in 0..100 {
            net.send(0, 1, b"x".to_vec());
        }
        assert_eq!(net.inboxes[1].len(), 100);
        assert_eq!(net.stats.total_dropped_rate, 0);
    }

    #[test]
    fn test_isolate_partition() {
        let mut net = SimNetwork::new(5);
        net.apply_partition(PartitionConfig::isolate(2, 5));
        // Node 2 → diğerlerine bloke
        net.send(2, 0, b"x".to_vec());
        net.send(2, 1, b"x".to_vec());
        // Diğerler kendi aralarında serbest
        net.send(0, 1, b"ok".to_vec());
        net.send(3, 4, b"ok".to_vec());

        assert_eq!(net.inboxes[0].len(), 0); // node 2'den bloke
        assert_eq!(net.inboxes[1].len(), 1); // node 0'dan ok
        assert_eq!(net.stats.total_dropped_partition, 2);
    }

    #[test]
    fn test_stats_delivery_rate() {
        let mut net = SimNetwork::new(2);
        for _ in 0..100 {
            net.send(0, 1, b"x".to_vec());
        }
        assert_eq!(net.stats.delivery_rate_bps(), 10_000); // %100
    }

    #[test]
    fn test_drain_inbox() {
        let mut net = SimNetwork::new(3);
        net.send(0, 1, b"a".to_vec());
        net.send(2, 1, b"b".to_vec());

        let inbox = net.drain_inbox(1);
        assert_eq!(inbox.len(), 2);
        assert_eq!(net.inboxes[1].len(), 0); // temizlendi
    }
}
