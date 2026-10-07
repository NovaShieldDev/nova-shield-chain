//! GÖREV 104: Epoch XOR Randomness (VRF Segment Birikimi)
//!
//! ## Mimari
//!
//! ### BABE Epoch Randomness Birikimi
//! - Her epoch, BABE_EPOCH_DURATION slot içerir
//! - İlk 2/3 segment: VRF çıktıları toplanır (randomness accumulation window)
//! - Son 1/3 segment: VRF çıktıları sonraki epoch randomness'ına eklenmez
//!   → "Last-reveal saldırısı" koruması: son 1/3'te validator seçim üstünlüğü yok
//! - XOR kombinasyonu: `new_randomness = BLAKE3(old_XOR ⊕ vrf_output1 ⊕ ... ⊕ vrf_outputN)`
//!
//! ### Nova Uyarlaması
//! - `EpochRandomnessAccumulator`: segment bazlı XOR birikimi
//! - `RandomnessSegment`: her slot'un hangi segmente ait olduğu
//! - `EpochRandomnessHistory`: geçmiş epoch randomness kayıtları (on-chain VDF opsiyonel)

use crate::vrf::BABE_EPOCH_DURATION;
use blake3;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Epoch başına kaç segment?
/// BABE'de: accumulation window = ilk 2/3 slot
pub const RANDOMNESS_ACCUMULATION_SEGMENTS: u64 = 2;
pub const RANDOMNESS_TOTAL_SEGMENTS: u64 = 3;

/// Epoch başına randomness accumulation penceresi (slot sayısı)
/// = BABE_EPOCH_DURATION * 2/3
pub const RANDOMNESS_WINDOW_SLOTS: u64 =
    BABE_EPOCH_DURATION * RANDOMNESS_ACCUMULATION_SEGMENTS / RANDOMNESS_TOTAL_SEGMENTS;

/// Geçmiş epoch randomness kayıt derinliği
pub const RANDOMNESS_HISTORY_DEPTH: usize = 84; // ~1 yıl (BABE'de 84 epoch saklı)

// ─── RandomnessSegment ────────────────────────────────────────────────────────

/// Bir slot'un epoch içindeki segment konumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RandomnessSegment {
    /// İlk 2/3 → VRF çıktısı birikime eklenir
    AccumulationWindow,
    /// Son 1/3 → VRF çıktısı birikime eklenmez (last-reveal koruması)
    LockoutWindow,
}

impl RandomnessSegment {
    /// Bir epoch-içi slot offset'i hangi segmente ait?
    ///
    /// slot_in_epoch: [0, BABE_EPOCH_DURATION)
    pub fn for_slot_in_epoch(slot_in_epoch: u64) -> Self {
        if slot_in_epoch < RANDOMNESS_WINDOW_SLOTS {
            Self::AccumulationWindow
        } else {
            Self::LockoutWindow
        }
    }

    /// Bu segmentte VRF birikimi yapılmalı mı?
    pub fn should_accumulate(&self) -> bool {
        matches!(self, Self::AccumulationWindow)
    }
}

// ─── EpochRandomnessAccumulator ───────────────────────────────────────────────

/// Epoch başına VRF çıktıları XOR ile biriktirir
///
/// Polkadot: `NextRandomness` storage item (pallet_babe)
#[derive(Debug, Clone)]
pub struct EpochRandomnessAccumulator {
    /// Mevcut epoch numarası
    pub epoch_index: u64,
    /// Epoch başlangıç slotu (mutlak)
    pub epoch_start_slot: u64,
    /// XOR birikimi (next epoch randomness ham verisi)
    pub xor_accumulator: [u8; 32],
    /// Kaç VRF çıktısı biriktirildi?
    pub accumulated_count: u64,
    /// Kaç slot lockout window'da işlendi? (atlandı)
    pub lockout_count: u64,
    /// Epoch kilitlendi mi? (finalize edildi, yeni accumulation başlamadı)
    pub finalized: bool,
}

impl EpochRandomnessAccumulator {
    /// Yeni accumulator, verilen epoch için
    pub fn new(epoch_index: u64, epoch_start_slot: u64) -> Self {
        Self {
            epoch_index,
            epoch_start_slot,
            xor_accumulator: [0u8; 32],
            accumulated_count: 0,
            lockout_count: 0,
            finalized: false,
        }
    }

    /// VRF çıktısını, hangi absolute slota ait olduğuna göre birikime ekle
    ///
    /// Returns: birikime eklendi mi?
    pub fn add_vrf_output(&mut self, slot: u64, vrf_output: &[u8; 32]) -> bool {
        if self.finalized {
            return false;
        }

        let slot_in_epoch = slot.saturating_sub(self.epoch_start_slot);
        let segment = RandomnessSegment::for_slot_in_epoch(slot_in_epoch);

        if segment.should_accumulate() {
            // XOR ile ekle
            for (i, byte) in vrf_output.iter().enumerate() {
                self.xor_accumulator[i] ^= byte;
            }
            self.accumulated_count += 1;
            true
        } else {
            self.lockout_count += 1;
            false
        }
    }

    /// Epoch randomness'ını finalize et → sonraki epoch'un seed'i
    ///
    /// BABE: `CurrentRandomness = BLAKE3(NextRandomness || epoch_index)`
    pub fn finalize(&mut self) -> [u8; 32] {
        let result = compute_epoch_randomness(&self.xor_accumulator, self.epoch_index);
        self.finalized = true;
        result
    }

    /// XOR accumulator değeri (ham)
    pub fn raw_xor(&self) -> &[u8; 32] {
        &self.xor_accumulator
    }

    /// Toplam accumulation ilerleme yüzdesi (bps, 0-10000)
    ///
    /// Ne kadar VRF toplanmalıydı vs. ne kadar toplandı
    pub fn progress_bps(&self) -> u64 {
        if RANDOMNESS_WINDOW_SLOTS == 0 {
            return 10_000;
        }
        (self.accumulated_count * 10_000 / RANDOMNESS_WINDOW_SLOTS).min(10_000)
    }
}

/// Epoch randomness hesabı (BLAKE3 finalization)
///
/// Polkadot: `H(NextRandomness || epoch_index) → CurrentRandomness`
pub fn compute_epoch_randomness(xor_acc: &[u8; 32], epoch_index: u64) -> [u8; 32] {
    *blake3::hash(&[
        b"nova-babe-epoch-finalize:".as_slice(),
        xor_acc.as_slice(),
        &epoch_index.to_le_bytes(),
    ].concat()).as_bytes()
}

// ─── EpochRandomnessRecord ────────────────────────────────────────────────────

/// Bir epoch'un finalize edilmiş randomness kaydı
#[derive(Debug, Clone)]
pub struct EpochRandomnessRecord {
    /// Epoch numarası
    pub epoch_index: u64,
    /// Finalize edilmiş randomness (sonraki epoch'ta kullanılır)
    pub randomness: [u8; 32],
    /// Kaç VRF katkısı yapıldı?
    pub contributor_count: u64,
}

// ─── EpochRandomnessHistory ───────────────────────────────────────────────────

/// Geçmiş epoch randomness kayıtları
///
/// Polkadot: `EpochRandomness` storage map (son 84 epoch)
pub struct EpochRandomnessHistory {
    records: std::collections::VecDeque<EpochRandomnessRecord>,
    capacity: usize,
}

impl EpochRandomnessHistory {
    pub fn new() -> Self {
        Self {
            records: std::collections::VecDeque::new(),
            capacity: RANDOMNESS_HISTORY_DEPTH,
        }
    }

    /// Yeni finalize kayıt ekle
    pub fn push(&mut self, record: EpochRandomnessRecord) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    /// Epoch numarasına göre randomness bul
    pub fn get(&self, epoch_index: u64) -> Option<&EpochRandomnessRecord> {
        self.records.iter().find(|r| r.epoch_index == epoch_index)
    }

    /// En son epoch randomness
    pub fn latest(&self) -> Option<&EpochRandomnessRecord> {
        self.records.back()
    }

    /// Kayıt sayısı
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl Default for EpochRandomnessHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ─── EpochRandomnessManager ───────────────────────────────────────────────────

/// Epoch randomness tam yaşam döngüsü yöneticisi
///
/// Polkadot: `pallet_babe` randomness mantığı
pub struct EpochRandomnessManager {
    /// Mevcut epoch accumulator
    current: EpochRandomnessAccumulator,
    /// Bir sonraki epoch accumulator (hazırlık aşamasında)
    next: Option<EpochRandomnessAccumulator>,
    /// Geçmiş kayıtlar
    history: EpochRandomnessHistory,
    /// Mevcut epoch başlangıç randomness (genesis veya önceki epoch finalized)
    current_randomness: [u8; 32],
}

impl EpochRandomnessManager {
    /// Genesis randomness ile başlat
    pub fn new(genesis_randomness: [u8; 32], epoch_start_slot: u64) -> Self {
        Self {
            current: EpochRandomnessAccumulator::new(0, epoch_start_slot),
            next: None,
            history: EpochRandomnessHistory::new(),
            current_randomness: genesis_randomness,
        }
    }

    /// Slot'un VRF çıktısını işle
    ///
    /// Doğru accumulator'a (current vs next) yönlendirir
    pub fn process_slot_vrf(&mut self, slot: u64, vrf_output: &[u8; 32]) -> bool {
        // Slot mevcut epoch'a mı yoksa sonrakine mi ait?
        let current_epoch_end = self.current.epoch_start_slot + BABE_EPOCH_DURATION;

        if slot < current_epoch_end {
            self.current.add_vrf_output(slot, vrf_output)
        } else if let Some(ref mut next_acc) = self.next {
            next_acc.add_vrf_output(slot, vrf_output)
        } else {
            false
        }
    }

    /// Epoch geçişi: current finalize, next → current olur
    ///
    /// Returns: finalize edilen randomness
    pub fn advance_epoch(&mut self) -> [u8; 32] {
        // 1. Mevcut epoch'u finalize et
        let finalized_randomness = self.current.finalize();
        let record = EpochRandomnessRecord {
            epoch_index: self.current.epoch_index,
            randomness: finalized_randomness,
            contributor_count: self.current.accumulated_count,
        };
        self.history.push(record);

        // 2. Bir sonraki epoch'u hazırla
        let next_epoch_index = self.current.epoch_index + 1;
        let next_start = self.current.epoch_start_slot + BABE_EPOCH_DURATION;

        // 3. Aktif randomness güncelle
        self.current_randomness = finalized_randomness;

        // 4. Yeni accumulator oluştur
        self.current = self.next.take().unwrap_or_else(|| {
            EpochRandomnessAccumulator::new(next_epoch_index, next_start)
        });

        // Epoch index ve start slot'u güncelle (eğer hazır next alındıysa)
        if self.current.epoch_index == 0 && next_epoch_index > 0 {
            self.current.epoch_index = next_epoch_index;
            self.current.epoch_start_slot = next_start;
        }

        // 5. Next+1 accumulator oluştur
        let next_next_start = next_start + BABE_EPOCH_DURATION;
        self.next = Some(EpochRandomnessAccumulator::new(
            next_epoch_index + 1,
            next_next_start,
        ));

        finalized_randomness
    }

    /// Mevcut aktif randomness (bu epoch'ta kullanılan seed)
    pub fn current_randomness(&self) -> &[u8; 32] {
        &self.current_randomness
    }

    /// Geçmiş kayıtlar
    pub fn history(&self) -> &EpochRandomnessHistory {
        &self.history
    }

    /// Mevcut accumulator bilgisi
    pub fn current_accumulator(&self) -> &EpochRandomnessAccumulator {
        &self.current
    }

    /// Kaç epoch tamamlandı?
    pub fn completed_epochs(&self) -> usize {
        self.history.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Accumulation window kontrolü
    #[test]
    fn test_accumulation_window() {
        let window = RANDOMNESS_WINDOW_SLOTS;
        assert_eq!(window, BABE_EPOCH_DURATION * 2 / 3);

        let seg_early = RandomnessSegment::for_slot_in_epoch(0);
        assert_eq!(seg_early, RandomnessSegment::AccumulationWindow);

        let seg_mid = RandomnessSegment::for_slot_in_epoch(window - 1);
        assert_eq!(seg_mid, RandomnessSegment::AccumulationWindow);

        let seg_lockout = RandomnessSegment::for_slot_in_epoch(window);
        assert_eq!(seg_lockout, RandomnessSegment::LockoutWindow);

        let seg_late = RandomnessSegment::for_slot_in_epoch(BABE_EPOCH_DURATION - 1);
        assert_eq!(seg_late, RandomnessSegment::LockoutWindow);
    }

    // 2. XOR birikimi doğru çalışıyor
    #[test]
    fn test_xor_accumulation() {
        let mut acc = EpochRandomnessAccumulator::new(0, 0);

        let v1 = [0xAAu8; 32];
        let v2 = [0x55u8; 32];

        acc.add_vrf_output(0, &v1);
        acc.add_vrf_output(1, &v2);

        // 0xAA XOR 0x55 = 0xFF
        assert_eq!(acc.xor_accumulator, [0xFFu8; 32]);
        assert_eq!(acc.accumulated_count, 2);
    }

    // 3. Lockout window'da VRF eklenmez
    #[test]
    fn test_lockout_window_rejected() {
        let mut acc = EpochRandomnessAccumulator::new(0, 0);

        // Lockout window'da bir slot
        let lockout_slot = RANDOMNESS_WINDOW_SLOTS + 10;
        let added = acc.add_vrf_output(lockout_slot, &[0xFFu8; 32]);

        assert!(!added);
        assert_eq!(acc.accumulated_count, 0);
        assert_eq!(acc.lockout_count, 1);
        assert_eq!(acc.xor_accumulator, [0u8; 32]);
    }

    // 4. Accumulation pencere doluluk oranı
    #[test]
    fn test_progress_bps() {
        let mut acc = EpochRandomnessAccumulator::new(0, 0);
        assert_eq!(acc.progress_bps(), 0);

        // Yarısını doldur
        for slot in 0..(RANDOMNESS_WINDOW_SLOTS / 2) {
            acc.add_vrf_output(slot, &[0x01u8; 32]);
        }

        let progress = acc.progress_bps();
        assert!((progress as i64 - 5000).abs() <= 10); // ~%50 ± tolerans
    }

    // 5. Finalize → farklı epoch index → farklı randomness
    #[test]
    fn test_finalize_epoch_index_matters() {
        let xor = [0xAAu8; 32];
        let r0 = compute_epoch_randomness(&xor, 0);
        let r1 = compute_epoch_randomness(&xor, 1);
        assert_ne!(r0, r1);
    }

    // 6. Finalize sonrası ekleme engellenir
    #[test]
    fn test_no_add_after_finalize() {
        let mut acc = EpochRandomnessAccumulator::new(0, 0);
        acc.add_vrf_output(0, &[0x01u8; 32]);
        acc.finalize();

        let added = acc.add_vrf_output(1, &[0x02u8; 32]);
        assert!(!added);
        assert_eq!(acc.accumulated_count, 1); // hâlâ 1
    }

    // 7. EpochRandomnessHistory capacity
    #[test]
    fn test_history_capacity() {
        let mut hist = EpochRandomnessHistory::new();
        for i in 0..(RANDOMNESS_HISTORY_DEPTH + 5) as u64 {
            hist.push(EpochRandomnessRecord {
                epoch_index: i,
                randomness: [i as u8; 32],
                contributor_count: 10,
            });
        }
        assert_eq!(hist.len(), RANDOMNESS_HISTORY_DEPTH);
        // En eskiler silinmiş
        assert!(hist.get(0).is_none());
        assert!(hist.get(4).is_none());
        // En yeniler var
        assert!(hist.latest().is_some());
    }

    // 8. EpochRandomnessManager genesis başlangıcı
    #[test]
    fn test_manager_genesis() {
        let genesis = [0x42u8; 32];
        let mgr = EpochRandomnessManager::new(genesis, 0);
        assert_eq!(*mgr.current_randomness(), genesis);
        assert_eq!(mgr.completed_epochs(), 0);
    }

    // 9. Manager slot işleme ve epoch geçişi
    #[test]
    fn test_manager_epoch_advance() {
        let genesis = [0x01u8; 32];
        let mut mgr = EpochRandomnessManager::new(genesis, 0);

        // Bazı slot VRF çıktıları ekle (accumulation window'da)
        for slot in 0..10u64 {
            mgr.process_slot_vrf(slot, &[slot as u8; 32]);
        }

        let finalized_r = mgr.advance_epoch();
        assert_ne!(finalized_r, [0u8; 32]);
        assert_eq!(mgr.completed_epochs(), 1);

        // Bir sonraki epoch randomness genesis'ten farklı olmalı
        assert_ne!(*mgr.current_randomness(), genesis);
    }

    // 10. İki ardışık epoch birikimi
    #[test]
    fn test_two_epoch_randomness_different() {
        let mut mgr = EpochRandomnessManager::new([0x01u8; 32], 0);

        // Epoch 0: bazı VRF ekle
        for slot in 0..5u64 {
            mgr.process_slot_vrf(slot, &[0xAAu8; 32]);
        }
        let r0 = mgr.advance_epoch();

        // Epoch 1: farklı VRF ekle
        let epoch1_start = BABE_EPOCH_DURATION;
        for slot in epoch1_start..epoch1_start + 5 {
            mgr.process_slot_vrf(slot, &[0x55u8; 32]);
        }
        let r1 = mgr.advance_epoch();

        assert_ne!(r0, r1);
        assert_eq!(mgr.completed_epochs(), 2);
    }

    // 11. Aynı epoch aynı VRF çıktıları → deterministik randomness
    #[test]
    fn test_deterministic_accumulation() {
        let finalize_with = |outputs: &[[u8; 32]]| {
            let mut acc = EpochRandomnessAccumulator::new(5, 0);
            for (i, out) in outputs.iter().enumerate() {
                acc.add_vrf_output(i as u64, out);
            }
            acc.finalize()
        };

        let outputs = [[0x11u8; 32], [0x22u8; 32], [0x33u8; 32]];
        let r1 = finalize_with(&outputs);
        let r2 = finalize_with(&outputs);
        assert_eq!(r1, r2);
    }

    // 12. Sıfır VRF katkısıyla bile finalize çalışır (kötü senaryo)
    #[test]
    fn test_finalize_with_no_contributions() {
        let mut acc = EpochRandomnessAccumulator::new(0, 0);
        let result = acc.finalize();
        // Hepsi sıfır XOR ile de bir randomness üretilir (degrade mode)
        assert_ne!(result, [0u8; 32]); // BLAKE3(sıfır || epoch_index) != sıfır
    }

    // 13. Should_accumulate kontrol
    #[test]
    fn test_segment_should_accumulate() {
        assert!(RandomnessSegment::AccumulationWindow.should_accumulate());
        assert!(!RandomnessSegment::LockoutWindow.should_accumulate());
    }

    // 14. Manager'da lockout slot'lar işlenmez
    #[test]
    fn test_manager_lockout_slots_ignored() {
        let mut mgr = EpochRandomnessManager::new([0x01u8; 32], 0);

        // Accumulation window dışındaki slot
        let lockout_slot = RANDOMNESS_WINDOW_SLOTS + 100;
        let added = mgr.process_slot_vrf(lockout_slot, &[0xFFu8; 32]);
        assert!(!added);
        assert_eq!(mgr.current_accumulator().accumulated_count, 0);
    }
}
