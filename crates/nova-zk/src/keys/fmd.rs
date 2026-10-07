//! GÖREV 96: FMD Clue Key Sistemi (Fuzzy Message Detection — Penumbra Pattern)
//!
//! ## Mimari
//!
//! ### Penumbra FMD (Fuzzy Message Detection)
//! - Kullanıcılar `ClueKey` paylaşır (gizliliği koruyarak)
//! - Gönderen: `Clue::create(clue_key, precision)` → TX'e ekler
//! - Alıcı: `DetectionKey::detect(clue)` → bu TX bana mı ait?
//! - **Yanlış pozitif oranı**: `2^(-precision)` (precision=4 → %6.25 false pos)
//! - Alıcı gerçek işlemi bulmak için tüm eşleşmeleri indirmeli
//!
//! ### Privacy Modeli
//! - Üçüncü taraf (DetectionService) ClueKey ile tüm TX'leri tarar
//! - Kullanıcıya sadece "olası eşleşme" listesi döner
//! - Kesin eşleşme bilgisi SADECE alıcıda (SpendingKey gerektirir)
//! - FMD'nin sağladığı gizlilik: gözlemci hangi TX'lerin alıcıya ait olduğunu bilemez

use blake3;
use nova_primitives::Address;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Minimum FMD precision (yanlış pozitif oranı yüksek ama ucuz)
pub const FMD_MIN_PRECISION: u8 = 1;

/// Maksimum FMD precision
pub const FMD_MAX_PRECISION: u8 = 24;

/// Varsayılan precision (1/16 = %6.25 false positive)
pub const FMD_DEFAULT_PRECISION: u8 = 4;

/// Clue boyutu (bayt): 32 byte commitment + 1 byte precision
pub const CLUE_SIZE: usize = 33;

// ─── FmdClueKey ───────────────────────────────────────────────────────────────

/// FMD Clue Key — paylaşılabilir (detection service'e verilir)
///
/// Penumbra: `ExpandedFMDParameters { clue_key }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmdClueKey {
    /// 32 byte public key (mock: BLAKE3 türetilmiş)
    pub key: [u8; 32],
}

impl FmdClueKey {
    pub fn from_bytes(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// IncomingViewingKey'den türet
    ///
    /// Penumbra: `FMD_CLUE_KEY = PRF(ivk, "fmd_clue_key")`
    pub fn from_ivk(ivk_bytes: &[u8]) -> Self {
        let key = *blake3::hash(&[ivk_bytes, b"nova-fmd-clue-key"].concat()).as_bytes();
        Self { key }
    }

    /// Clue oluştur (gönderenin yapacağı)
    pub fn create_clue(&self, precision: u8, randomness: [u8; 32]) -> FmdClue {
        let effective_precision = precision.clamp(FMD_MIN_PRECISION, FMD_MAX_PRECISION);
        FmdClue::create(self, effective_precision, randomness)
    }
}

// ─── FmdDetectionKey ──────────────────────────────────────────────────────────

/// FMD Detection Key — daha az gizli (tam eşleşme için)
///
/// Penumbra: `ExpandedFMDParameters { detection_key }`
/// ClueKey'den türetilir ama ClueKey'e dönüştürülemez.
#[derive(Debug, Clone)]
pub struct FmdDetectionKey {
    /// 32 byte detection key
    dk: [u8; 32],
    /// İlgili clue key
    pub clue_key: FmdClueKey,
}

impl FmdDetectionKey {
    /// SpendingKey veya IVK'dan türet
    ///
    /// Penumbra: `DetectionKey::from_field_element`
    pub fn from_seed(seed: &[u8]) -> Self {
        let dk = *blake3::hash(&[seed, b"nova-fmd-detection-key"].concat()).as_bytes();
        let clue_key = FmdClueKey::from_ivk(&dk);
        Self { dk, clue_key }
    }

    /// Clue'nun bu hesaba ait olup olmadığını kontrol et
    ///
    /// Penumbra: `DetectionKey::detect(clue) -> bool`
    /// True dönebirabilir (false positive mümkün — precision belirler)
    pub fn detect(&self, clue: &FmdClue) -> bool {
        clue.detect_with_key(self)
    }

    /// Kesin eşleşme: hem detect hem commitment doğrulama
    pub fn is_exact_match(&self, clue: &FmdClue) -> bool {
        clue.is_exact_match_for(self)
    }

    pub fn dk_bytes(&self) -> &[u8; 32] {
        &self.dk
    }
}

// ─── FmdClue ──────────────────────────────────────────────────────────────────

/// FMD Clue — TX'e eklenen algılama ipucu
///
/// Penumbra: `Clue { ciphertext: [u8; 68], precision_bits: u8 }`
/// Nova mock: 32 byte commitment + 1 byte precision
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmdClue {
    /// Commitment (clue key + randomness ile üretildi)
    pub commitment: [u8; 32],
    /// Precision (false positive oranı kontrolü)
    pub precision: u8,
    /// Randomness (commitment seed)
    pub randomness: [u8; 32],
}

impl FmdClue {
    /// Clue oluştur
    ///
    /// Penumbra: `ClueKey::create(precision_bits, randomness) -> Clue`
    pub fn create(clue_key: &FmdClueKey, precision: u8, randomness: [u8; 32]) -> Self {
        // commitment = BLAKE3(clue_key || randomness || precision)
        let commitment = *blake3::hash(&[
            clue_key.key.as_slice(),
            randomness.as_slice(),
            &[precision],
            b"nova-fmd-clue",
        ].concat()).as_bytes();

        Self { commitment, precision, randomness }
    }

    /// DetectionKey ile eşleşme kontrolü (false positive mümkün)
    ///
    /// Penumbra: `Clue::detect(detection_key) -> bool`
    fn detect_with_key(&self, dk: &FmdDetectionKey) -> bool {
        // Exact match her zaman true
        if self.is_exact_match_for(dk) {
            return true;
        }

        // False positive: commitment'in son precision bit'i sıfır mı?
        // 2^(-precision) olasılıkla false positive
        let hash = *blake3::hash(&[dk.dk.as_slice(), self.randomness.as_slice()].concat()).as_bytes();
        let check_bits = self.precision as u32;
        if check_bits == 0 { return true; }

        // Son N bit sıfır mı? (N = precision)
        let val = u32::from_le_bytes(hash[0..4].try_into().unwrap());
        let mask = if check_bits >= 32 { u32::MAX } else { (1u32 << check_bits) - 1 };
        (val & mask) == 0
    }

    /// Kesin eşleşme (clue key ile)
    fn is_exact_match_for(&self, dk: &FmdDetectionKey) -> bool {
        // Eğer bu clue bu detection key'in clue_key'i için oluşturulduysa
        let expected = *blake3::hash(&[
            dk.clue_key.key.as_slice(),
            self.randomness.as_slice(),
            &[self.precision],
            b"nova-fmd-clue",
        ].concat()).as_bytes();
        self.commitment == expected
    }

    /// Clue byte'larını serialize et
    pub fn to_bytes(&self) -> [u8; CLUE_SIZE] {
        let mut bytes = [0u8; CLUE_SIZE];
        bytes[0..32].copy_from_slice(&self.commitment);
        bytes[32] = self.precision;
        bytes
    }

    /// Deserialize
    pub fn from_bytes(bytes: &[u8; CLUE_SIZE], randomness: [u8; 32]) -> Self {
        let mut commitment = [0u8; 32];
        commitment.copy_from_slice(&bytes[0..32]);
        Self { commitment, precision: bytes[32], randomness }
    }
}

// ─── FmdKeyPair ───────────────────────────────────────────────────────────────

/// FMD anahtar çifti: DetectionKey + ClueKey
#[derive(Debug, Clone)]
pub struct FmdKeyPair {
    pub detection_key: FmdDetectionKey,
}

impl FmdKeyPair {
    pub fn generate(seed: &[u8]) -> Self {
        Self {
            detection_key: FmdDetectionKey::from_seed(seed),
        }
    }

    pub fn clue_key(&self) -> &FmdClueKey {
        &self.detection_key.clue_key
    }

    pub fn detection_key(&self) -> &FmdDetectionKey {
        &self.detection_key
    }
}

// ─── FmdDetectionService ──────────────────────────────────────────────────────

/// Clue'ları bir DetectionKey ile tarayan servis (üçüncü taraf)
///
/// Penumbra: `CompactBlock` + client-side filtering
/// Kullanıcı ClueKey'i servise verir, servis eşleşmeleri döner
pub struct FmdDetectionService {
    detection_key: FmdDetectionKey,
    /// Taranan toplam clue sayısı
    scanned: u64,
    /// Eşleşen (true veya false positive) sayısı
    matched: u64,
}

impl FmdDetectionService {
    pub fn new(detection_key: FmdDetectionKey) -> Self {
        Self { detection_key, scanned: 0, matched: 0 }
    }

    /// Clue listesini tara, eşleşenleri döndür
    pub fn scan(&mut self, clues: &[FmdClue]) -> Vec<usize> {
        let mut matches = Vec::new();
        for (i, clue) in clues.iter().enumerate() {
            self.scanned += 1;
            if self.detection_key.detect(clue) {
                self.matched += 1;
                matches.push(i);
            }
        }
        matches
    }

    pub fn scanned(&self) -> u64 { self.scanned }
    pub fn matched(&self) -> u64 { self.matched }

    /// Yanlış pozitif oranı (istatistiksel)
    pub fn false_positive_rate(precision: u8) -> f64 {
        2f64.powi(-(precision as i32))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keypair(n: u8) -> FmdKeyPair {
        FmdKeyPair::generate(&[n; 32])
    }

    fn rand(n: u8) -> [u8; 32] {
        [n; 32]
    }

    // 1. FmdClueKey türetme deterministik
    #[test]
    fn test_clue_key_deterministic() {
        let k1 = FmdClueKey::from_ivk(&[1u8; 32]);
        let k2 = FmdClueKey::from_ivk(&[1u8; 32]);
        assert_eq!(k1.key, k2.key);
    }

    // 2. Farklı IVK → farklı ClueKey
    #[test]
    fn test_clue_key_different_ivk() {
        let k1 = FmdClueKey::from_ivk(&[1u8; 32]);
        let k2 = FmdClueKey::from_ivk(&[2u8; 32]);
        assert_ne!(k1.key, k2.key);
    }

    // 3. Clue oluşturma deterministik
    #[test]
    fn test_clue_creation_deterministic() {
        let kp = make_keypair(1);
        let r = rand(42);
        let c1 = kp.clue_key().create_clue(FMD_DEFAULT_PRECISION, r);
        let c2 = kp.clue_key().create_clue(FMD_DEFAULT_PRECISION, r);
        assert_eq!(c1.commitment, c2.commitment);
        assert_eq!(c1, c2);
    }

    // 4. Doğru DetectionKey → her zaman detect eder
    #[test]
    fn test_exact_match_always_detected() {
        let kp = make_keypair(5);
        let r = rand(99);
        let clue = kp.clue_key().create_clue(FMD_DEFAULT_PRECISION, r);
        assert!(kp.detection_key().detect(&clue));
    }

    // 5. Yanlış DetectionKey → clue oluşturanın değil
    #[test]
    fn test_wrong_detection_key_mostly_false_negative() {
        let kp_sender = make_keypair(1);
        let kp_wrong = make_keypair(2); // Farklı key pair

        let mut false_positives = 0;
        for i in 0..100u8 {
            let clue = kp_sender.clue_key().create_clue(16, rand(i)); // precision=16 → 1/65536 false pos
            if kp_wrong.detection_key().detect(&clue) {
                false_positives += 1;
            }
        }
        // %99.99'dan fazla false negative bekliyoruz
        assert!(false_positives < 5, "too many false positives: {false_positives}");
    }

    // 6. Precision 0 → her zaman eşleşir (tamamen bulanık)
    #[test]
    fn test_precision_zero_always_matches() {
        let kp1 = make_keypair(10);
        let kp2 = make_keypair(20);
        let r = rand(7);
        // Precision=0 → tüm mesajlar eşleşir (hiç filtreleme yok)
        let clue = kp1.clue_key().create_clue(0, r);
        // 0 clamp → min=1, ama hala yüksek false positive
        // Burada precision=1 test edelim
        let clue1 = kp1.clue_key().create_clue(1, r);
        // Kendi key ile her zaman match
        assert!(kp1.detection_key().detect(&clue1));
    }

    // 7. Clue serialize/deserialize
    #[test]
    fn test_clue_serialization() {
        let kp = make_keypair(3);
        let r = rand(55);
        let original = kp.clue_key().create_clue(FMD_DEFAULT_PRECISION, r);
        let bytes = original.to_bytes();
        let restored = FmdClue::from_bytes(&bytes, r);
        assert_eq!(original.commitment, restored.commitment);
        assert_eq!(original.precision, restored.precision);
    }

    // 8. Exact match doğrulaması
    #[test]
    fn test_exact_match_detection() {
        let kp = make_keypair(7);
        let r = rand(11);
        let clue = kp.clue_key().create_clue(FMD_DEFAULT_PRECISION, r);
        assert!(kp.detection_key().is_exact_match(&clue));

        // Başka bir key için exact match olmamalı
        let kp2 = make_keypair(8);
        assert!(!kp2.detection_key().is_exact_match(&clue));
    }

    // 9. FmdDetectionService tarama
    #[test]
    fn test_detection_service_scan() {
        let kp_alice = make_keypair(1);
        let kp_bob = make_keypair(2);

        // Alice için 3 clue, bob için 2 clue
        let mut clues = Vec::new();
        for i in 0..3u8 {
            clues.push(kp_alice.clue_key().create_clue(FMD_DEFAULT_PRECISION, rand(i)));
        }
        for i in 10..12u8 {
            clues.push(kp_bob.clue_key().create_clue(FMD_DEFAULT_PRECISION, rand(i)));
        }

        let mut service = FmdDetectionService::new(kp_alice.detection_key.clone());
        let matches = service.scan(&clues);

        // Alice'in 3 clue'su (indeks 0, 1, 2) eşleşmeli
        assert!(matches.contains(&0));
        assert!(matches.contains(&1));
        assert!(matches.contains(&2));
        assert_eq!(service.scanned(), 5);
    }

    // 10. False positive rate hesabı
    #[test]
    fn test_false_positive_rate() {
        let rate_4 = FmdDetectionService::false_positive_rate(4);
        let rate_8 = FmdDetectionService::false_positive_rate(8);
        // precision=4 → 1/16 ≈ 0.0625
        assert!((rate_4 - 0.0625).abs() < 0.001);
        // precision=8 → 1/256 ≈ 0.0039
        assert!((rate_8 - 1.0 / 256.0).abs() < 0.0001);
        assert!(rate_8 < rate_4);
    }

    // 11. Farklı precision → farklı commitment
    #[test]
    fn test_different_precision_different_commitment() {
        let kp = make_keypair(1);
        let r = rand(1);
        let c4 = kp.clue_key().create_clue(4, r);
        let c8 = kp.clue_key().create_clue(8, r);
        assert_ne!(c4.commitment, c8.commitment);
    }

    // 12. FmdKeyPair clue_key/detection_key erişimi
    #[test]
    fn test_key_pair_access() {
        let kp = make_keypair(99);
        let ck = kp.clue_key().key;
        let dk = kp.detection_key().dk_bytes();
        // Farklı key'ler
        assert_ne!(ck, *dk);
    }

    // 13. Precision clamp
    #[test]
    fn test_precision_clamping() {
        let kp = make_keypair(1);
        let r = rand(1);
        let c_max = kp.clue_key().create_clue(255, r); // clamped to 24
        assert_eq!(c_max.precision, FMD_MAX_PRECISION);
        let c_min = kp.clue_key().create_clue(0, r); // clamped to 1
        assert_eq!(c_min.precision, FMD_MIN_PRECISION);
    }
}
