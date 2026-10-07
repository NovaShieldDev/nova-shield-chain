//! Bloom filtresi — Solana CRDS Pull gossip anti-entropy
//!
//! Solana `crds_gossip_pull.rs`:
//! - FALSE_RATE = 0.1
//! - KEYS = 8.0 (hash fonksiyon sayısı)
//! - `CrdsFilter { filter: Bloom, mask: u64, mask_bits: u32 }`
//! - Pull: yerel Bloom filtresi gönder → karşı taraf eksikleri döndürür

use serde::{Deserialize, Serialize};

/// Bloom filtresi sabitleri — Solana
pub const BLOOM_FALSE_RATE: f64 = 0.1;
pub const BLOOM_NUM_KEYS: usize = 8;

// ─── BloomFilter ──────────────────────────────────────────────────────────────

/// Basit Bloom filtresi
/// Solana: `Bloom { keys: Vec<u64>, bits: BitVec, num_bits_set: u64 }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloomFilter {
    /// Bit dizisi (u64 dizisi olarak tutulur)
    bits: Vec<u64>,
    /// Bit sayısı
    num_bits: usize,
    /// Hash tuz değerleri (BLOOM_NUM_KEYS kadar)
    seeds: [u64; 8],
    /// Set edilen bit sayısı
    num_bits_set: u64,
}

impl BloomFilter {
    /// Yeni Bloom filtresi — kapasite ve hata oranına göre boyut hesapla
    pub fn new(capacity: usize) -> Self {
        // n=capacity, p=FALSE_RATE → m = -n*ln(p) / (ln(2))^2
        let num_bits = Self::optimal_bits(capacity, BLOOM_FALSE_RATE).max(64);
        let num_words = (num_bits + 63) / 64;
        Self {
            bits: vec![0u64; num_words],
            num_bits,
            seeds: [1, 2, 3, 5, 7, 11, 13, 17],
            num_bits_set: 0,
        }
    }

    /// Sabit boyutlu (test için)
    pub fn with_bits(num_bits: usize) -> Self {
        let num_words = (num_bits + 63) / 64;
        Self {
            bits: vec![0u64; num_words],
            num_bits: num_bits.max(1),
            seeds: [1, 2, 3, 5, 7, 11, 13, 17],
            num_bits_set: 0,
        }
    }

    /// Eleman ekle
    pub fn add(&mut self, item: &[u8; 32]) {
        for k in 0..BLOOM_NUM_KEYS {
            let idx = self.hash_index(item, self.seeds[k]);
            let word = idx / 64;
            let bit = idx % 64;
            if self.bits[word] & (1u64 << bit) == 0 {
                self.bits[word] |= 1u64 << bit;
                self.num_bits_set += 1;
            }
        }
    }

    /// Eleman var mı? (false positive mümkün)
    pub fn contains(&self, item: &[u8; 32]) -> bool {
        for k in 0..BLOOM_NUM_KEYS {
            let idx = self.hash_index(item, self.seeds[k]);
            let word = idx / 64;
            let bit = idx % 64;
            if self.bits[word] & (1u64 << bit) == 0 {
                return false;
            }
        }
        true
    }

    /// Set edilen bit sayısı
    pub fn num_bits_set(&self) -> u64 {
        self.num_bits_set
    }

    /// Toplam bit sayısı
    pub fn num_bits(&self) -> usize {
        self.num_bits
    }

    /// Doluluk oranı
    pub fn fill_rate(&self) -> f64 {
        self.num_bits_set as f64 / self.num_bits as f64
    }

    /// Tahmini hata oranı — (1 - e^(-k*n/m))^k
    pub fn estimated_false_positive_rate(&self, num_items: usize) -> f64 {
        let k = BLOOM_NUM_KEYS as f64;
        let m = self.num_bits as f64;
        let n = num_items as f64;
        let inner = (-k * n / m).exp();
        (1.0 - inner).powf(k)
    }

    fn hash_index(&self, item: &[u8; 32], seed: u64) -> usize {
        // FNV-1a benzeri hash
        let mut h = seed;
        for &b in item.iter() {
            h = h.wrapping_mul(0x517CC1B727220A95);
            h ^= b as u64;
        }
        (h as usize) % self.num_bits
    }

    fn optimal_bits(n: usize, p: f64) -> usize {
        // m = ceil(-n * ln(p) / (ln(2))^2)
        let ln2_sq = std::f64::consts::LN_2 * std::f64::consts::LN_2;
        let m = -(n as f64) * p.ln() / ln2_sq;
        m.ceil() as usize
    }
}

// ─── CrdsFilter — Solana Pull Anti-Entropy ────────────────────────────────────

/// CRDS pull filtresi — Solana `CrdsFilter`
/// mask ve mask_bits ile büyük CRDS tablolarını bölümlere ayır
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdsFilter {
    pub filter: BloomFilter,
    /// Hangi hash alanını işlediğimiz
    pub mask: u64,
    pub mask_bits: u32,
}

impl CrdsFilter {
    pub fn new(num_items: usize) -> Self {
        Self {
            filter: BloomFilter::new(num_items.max(1)),
            mask: 0,
            mask_bits: 0,
        }
    }

    /// Bu filtre verilen hash'i kapsar mı?
    pub fn covers(&self, hash: &[u8; 32]) -> bool {
        if self.mask_bits == 0 {
            return true;
        }
        let h = u64::from_le_bytes(hash[..8].try_into().unwrap_or([0u8; 8]));
        (h >> (64 - self.mask_bits)) == self.mask
    }

    /// Hash'i ekle
    pub fn add(&mut self, hash: &[u8; 32]) {
        if self.covers(hash) {
            self.filter.add(hash);
        }
    }

    /// Hash filtreden geçiyor mu?
    pub fn filter_contains(&self, hash: &[u8; 32]) -> bool {
        self.covers(hash) && self.filter.contains(hash)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(n: u8) -> [u8; 32] {
        // BLAKE3 simülasyonu
        let mut h = [0u8; 32];
        let mut val = (n as u64).wrapping_mul(0x9e3779b97f4a7c15);
        for i in 0..4 {
            h[i * 8..(i + 1) * 8].copy_from_slice(&val.to_le_bytes());
            val = val.wrapping_mul(0x517CC1B727220A95).wrapping_add(0x6C62272E07BB0142);
        }
        h
    }

    #[test]
    fn test_bloom_add_contains() {
        let mut bf = BloomFilter::new(100);
        let h = hash(1);
        bf.add(&h);
        assert!(bf.contains(&h));
    }

    #[test]
    fn test_bloom_not_contains() {
        let mut bf = BloomFilter::new(100);
        bf.add(&hash(1));
        // hash(2) eklenmedi — false negative olmamalı
        // false positive olabilir ama bunu test etmiyoruz
        let _ = bf.contains(&hash(2)); // false positive mümkün, sadece crash etmemeli
    }

    #[test]
    fn test_bloom_multiple_items() {
        let mut bf = BloomFilter::new(1000);
        let hashes: Vec<[u8; 32]> = (1u8..=50).map(hash).collect();
        for h in &hashes {
            bf.add(h);
        }
        for h in &hashes {
            assert!(bf.contains(h), "false negative: {:?}", h);
        }
    }

    #[test]
    fn test_bloom_fill_rate() {
        let mut bf = BloomFilter::new(100);
        for i in 1u8..=10 {
            bf.add(&hash(i));
        }
        let rate = bf.fill_rate();
        assert!(rate > 0.0);
        assert!(rate <= 1.0);
    }

    #[test]
    fn test_bloom_bits_set() {
        let mut bf = BloomFilter::with_bits(256);
        assert_eq!(bf.num_bits_set(), 0);
        bf.add(&hash(1));
        assert!(bf.num_bits_set() > 0);
    }

    #[test]
    fn test_crds_filter_no_mask() {
        let mut cf = CrdsFilter::new(100);
        let h = hash(5);
        cf.add(&h);
        assert!(cf.filter_contains(&h));
    }

    #[test]
    fn test_bloom_false_positive_rate() {
        let mut bf = BloomFilter::new(100);
        for i in 0u8..50 {
            bf.add(&hash(i));
        }
        let fpr = bf.estimated_false_positive_rate(50);
        // Hata oranı makul olmalı
        assert!(fpr < 0.5);
    }
}
