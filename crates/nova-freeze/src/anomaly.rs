//! Anomali tespit motoru — §5.5.2 Tetikleyici 3
//!
//! Araştırma kaynakları:
//! - Polkadot im-online `UnresponsivenessOffence::slash_fraction()`:
//!   "min((3*(k - n/10 + 1)) / n, 1) * 0.07" — kademeli eşik formülü.
//!   Bizde: ağırlıklı katkı toplamı → normalize edilmiş skor.
//! - Cosmos SDK slashing `HandleValidatorSignature()`:
//!   Kayan pencere (sliding window) + bitmap ile geçmiş takibi.
//!   Bizde: VecDeque ile son N işlem geçmişi.
//! - Avalanchego snow/engine `Halter { halted: AtomicU32 }`:
//!   Thread-safe atomic flag pattern → AnomalyDetector'da DashMap thread safety.
//!
//! Anomali skoru hesaplama (0-10_000 bps):
//! 1. Volume sapması: (amount - avg) / avg → normalize
//! 2. Bilinmeyen alıcı oranı: unknown / total
//! 3. Yüksek frekans: tx_per_100_slots / baseline_rate
//! 4. Çoklu alıcı paterni: distinct_recipients > threshold

use crate::errors::{FreezeError, FreezeResult};
use crate::types::AnomalyType;
use dashmap::DashMap;
use nova_primitives::{Address, SlotId, ANOMALY_DETECTION_MIN_TX};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

/// Anomali hesaplama penceresi: son kaç işlem bakılır.
const BEHAVIOR_WINDOW_SIZE: usize = 500;

/// Yüksek frekans eşiği: kısa pencerede (100 slot) bu kadar işlem → anomali.
const HIGH_FREQUENCY_TX_COUNT: usize = 20;

/// Kısa pencere slot sayısı (frekans hesabı için).
const SHORT_WINDOW_SLOTS: u64 = 250; // ~100 saniye

/// Volume anomali çarpanı: ortalamanın kaç katı → anomali.
const VOLUME_ANOMALY_MULTIPLIER: u128 = 5;

/// Çoklu alıcı eşiği: tek batch'te bu kadar farklı alıcı → anomali.
const MULTI_RECIPIENT_THRESHOLD: usize = 10;

// ─── TxRecord ────────────────────────────────────────────────────────────────

/// Bir işlem kaydı — davranış profili oluşturmak için kaydedilir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxRecord {
    /// İşlemin gerçekleştiği slot.
    pub slot: SlotId,
    /// Transfer miktarı (baz birim, u128).
    pub amount: u128,
    /// Alıcı adresi (transfer/swap ise).
    pub recipient: Address,
    /// Bu işlem çıkış mı (true) yoksa giriş mi (false)?
    pub is_outgoing: bool,
}

// ─── BehaviorProfile ─────────────────────────────────────────────────────────

/// Bir hesabın geçmiş davranış profili.
///
/// Pattern: Cosmos SDK slashing `ValidatorSigningInfo` — her validator için
/// geçmiş aktivite bilgisi tutulur. Bizde: her hesap için tx geçmişi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorProfile {
    /// Hesap adresi.
    pub account: Address,
    /// Son N işlem geçmişi (kayan pencere).
    pub tx_history: VecDeque<TxRecord>,
    /// Toplam işlem sayısı (tüm zamanlar — baseline için).
    pub total_tx_count: u64,
    /// Ortalama çıkış miktarı (çalışan ortalama).
    pub avg_outgoing_amount: u128,
    /// Bilinen alıcı adresleri (baseline oluştuktan sonra).
    pub known_recipients: Vec<Address>,
    /// Son güncelleme slot'u.
    pub last_updated_slot: SlotId,
    /// Baseline kuruldu mu (min tx sayısına ulaşıldı mı)?
    pub baseline_established: bool,
}

impl BehaviorProfile {
    /// Yeni boş profil.
    pub fn new(account: Address) -> Self {
        Self {
            account,
            tx_history: VecDeque::with_capacity(BEHAVIOR_WINDOW_SIZE),
            total_tx_count: 0,
            avg_outgoing_amount: 0,
            known_recipients: Vec::new(),
            last_updated_slot: SlotId(0),
            baseline_established: false,
        }
    }

    /// İşlem kaydı ekle ve profili güncelle.
    pub fn record_tx(&mut self, record: TxRecord) {
        self.total_tx_count += 1;
        self.last_updated_slot = record.slot;

        // Bilinen alıcı listesine ekle (varsa)
        if record.is_outgoing && !self.known_recipients.contains(&record.recipient) {
            self.known_recipients.push(record.recipient);
        }

        // Kayan ortalama miktarı güncelle
        if record.is_outgoing && record.amount > 0 {
            if self.avg_outgoing_amount == 0 {
                self.avg_outgoing_amount = record.amount;
            } else {
                // Kayan ortalama: yeni = (eski * 9 + yeni) / 10
                self.avg_outgoing_amount =
                    (self.avg_outgoing_amount.saturating_mul(9) + record.amount) / 10;
            }
        }

        // Pencere doluysa eskiyi çıkar
        if self.tx_history.len() >= BEHAVIOR_WINDOW_SIZE {
            self.tx_history.pop_front();
        }
        self.tx_history.push_back(record);

        // Baseline kontrolü
        if !self.baseline_established
            && self.total_tx_count >= ANOMALY_DETECTION_MIN_TX as u64
        {
            self.baseline_established = true;
        }
    }

    /// Baseline kurulmuş mu?
    pub fn has_baseline(&self) -> bool {
        self.baseline_established
    }

    /// Son kısa penceredeki çıkış işlem sayısı.
    pub fn tx_count_in_window(&self, current_slot: SlotId, window_slots: u64) -> usize {
        let cutoff = SlotId(current_slot.0.saturating_sub(window_slots));
        self.tx_history
            .iter()
            .filter(|tx| tx.slot >= cutoff && tx.is_outgoing)
            .count()
    }

    /// Son batch'teki benzersiz alıcı sayısı.
    pub fn distinct_recipients_in_window(&self, current_slot: SlotId, window_slots: u64) -> HashSet<[u8; 20]> {
        let cutoff = SlotId(current_slot.0.saturating_sub(window_slots));
        self.tx_history
            .iter()
            .filter(|tx| tx.slot >= cutoff && tx.is_outgoing)
            .map(|tx| *tx.recipient.as_bytes())
            .collect()
    }
}

// ─── AnomalyResult ───────────────────────────────────────────────────────────

/// Anomali tespit sonucu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyResult {
    /// Toplam anomali skoru (0-10_000 bps).
    pub score_bps: u32,
    /// Birincil anomali türü (en yüksek katkılı).
    pub anomaly_type: AnomalyType,
    /// Katkıda bulunan faktörler.
    pub factors: Vec<AnomalyFactor>,
}

/// Tek bir anomali faktörü.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyFactor {
    /// Faktör açıklaması.
    pub description: String,
    /// Bu faktörün anomali skoruna katkısı (bps).
    pub contribution_bps: u32,
}

// ─── AnomalyDetector ─────────────────────────────────────────────────────────

/// Hesap davranış anomalisi tespit motoru.
///
/// Pattern:
/// - Polkadot im-online `SlashFraction` rolling window: kayan pencerede
///   kayıp blok sayısı → slash fraction hesabı. Bizde: kayan pencerede
///   tx sapma skoru hesabı.
/// - Avalanchego Halter atomic flag: thread-safe DashMap ile concurrent access.
pub struct AnomalyDetector {
    /// Her hesap için davranış profili.
    profiles: Arc<DashMap<Address, BehaviorProfile>>,
}

impl AnomalyDetector {
    pub fn new() -> Self {
        Self {
            profiles: Arc::new(DashMap::new()),
        }
    }

    /// İşlem kaydını profile ekle.
    pub fn record_transaction(&self, account: Address, record: TxRecord) {
        let mut profile = self.profiles.entry(account).or_insert_with(|| BehaviorProfile::new(account));
        profile.record_tx(record);
    }

    /// Profil var mı?
    pub fn has_profile(&self, account: &Address) -> bool {
        self.profiles.contains_key(account)
    }

    /// Profilin kopyasını al.
    pub fn get_profile(&self, account: &Address) -> Option<BehaviorProfile> {
        self.profiles.get(account).map(|p| p.clone())
    }

    /// Anomali skoru hesapla.
    ///
    /// Baseline yoksa `InsufficientBehaviorHistory` hatası döner.
    /// Pattern: Polkadot im-online `minHeight` check: "if height > minHeight".
    ///
    /// Skor bileşenleri (ağırlıklı):
    /// 1. Volume sapması (%35 ağırlık)
    /// 2. Bilinmeyen alıcı oranı (%25 ağırlık)
    /// 3. Yüksek frekans (%25 ağırlık)
    /// 4. Çoklu alıcı paterni (%15 ağırlık)
    pub fn compute_anomaly_score(
        &self,
        account: &Address,
        proposed_txs: &[TxRecord],
        current_slot: SlotId,
    ) -> FreezeResult<AnomalyResult> {
        let profile = self.profiles.get(account).ok_or(FreezeError::InsufficientBehaviorHistory)?;

        if !profile.has_baseline() {
            return Err(FreezeError::InsufficientBehaviorHistory);
        }

        let mut factors: Vec<AnomalyFactor> = Vec::new();
        let mut dominant_type = AnomalyType::UnusualTransferVolume;
        let mut max_contribution: u32 = 0;

        // ── 1. Volume sapması (ağırlık %35 = 3_500 bps max) ─────────────────
        let volume_score = Self::compute_volume_score(&profile, proposed_txs);
        if volume_score > 0 {
            let contribution = (volume_score * 3_500 / 10_000).min(3_500);
            factors.push(AnomalyFactor {
                description: "volume sapması (ortalama * 5 üzeri)".to_string(),
                contribution_bps: contribution,
            });
            if contribution > max_contribution {
                max_contribution = contribution;
                dominant_type = AnomalyType::UnusualTransferVolume;
            }
        }

        // ── 2. Bilinmeyen alıcı oranı (ağırlık %25 = 2_500 bps max) ─────────
        let unknown_score = Self::compute_unknown_recipient_score(&profile, proposed_txs);
        if unknown_score > 0 {
            let contribution = (unknown_score * 2_500 / 10_000).min(2_500);
            factors.push(AnomalyFactor {
                description: "bilinmeyen alıcı oranı yüksek".to_string(),
                contribution_bps: contribution,
            });
            if contribution > max_contribution {
                max_contribution = contribution;
                dominant_type = AnomalyType::TransferToUnknownAddress;
            }
        }

        // ── 3. Yüksek frekans (ağırlık %25 = 2_500 bps max) ─────────────────
        let freq_score = Self::compute_frequency_score(&profile, proposed_txs, current_slot);
        if freq_score > 0 {
            let contribution = (freq_score * 2_500 / 10_000).min(2_500);
            factors.push(AnomalyFactor {
                description: "kısa pencerede yüksek işlem frekansı".to_string(),
                contribution_bps: contribution,
            });
            if contribution > max_contribution {
                max_contribution = contribution;
                dominant_type = AnomalyType::RapidMultipleWithdrawals;
            }
        }

        // ── 4. Çoklu alıcı paterni (ağırlık %15 = 1_500 bps max) ─────────────
        let multi_score = Self::compute_multi_recipient_score(proposed_txs);
        if multi_score > 0 {
            let contribution = (multi_score * 1_500 / 10_000).min(1_500);
            factors.push(AnomalyFactor {
                description: "çok sayıda farklı alıcıya eş zamanlı transfer".to_string(),
                contribution_bps: contribution,
            });
            if contribution > max_contribution {
                dominant_type = AnomalyType::RapidMultipleWithdrawals;
            }
        }

        // Toplam skor
        let total: u32 = factors.iter().map(|f| f.contribution_bps).sum();
        let score_bps = total.min(10_000);

        Ok(AnomalyResult {
            score_bps,
            anomaly_type: dominant_type,
            factors,
        })
    }

    // ─── Özel skor hesaplama fonksiyonları ────────────────────────────────────

    /// Volume sapması skoru (0-10_000 bps).
    fn compute_volume_score(profile: &BehaviorProfile, proposed_txs: &[TxRecord]) -> u32 {
        if profile.avg_outgoing_amount == 0 {
            return 0;
        }
        let proposed_total: u128 = proposed_txs
            .iter()
            .filter(|tx| tx.is_outgoing)
            .map(|tx| tx.amount)
            .sum();
        if proposed_total == 0 {
            return 0;
        }
        let threshold = profile.avg_outgoing_amount.saturating_mul(VOLUME_ANOMALY_MULTIPLIER);
        if proposed_total <= threshold {
            return 0;
        }
        // proposed_total / avg ne kadar büyük? → skor
        let ratio = (proposed_total / profile.avg_outgoing_amount.max(1)) as u32;
        // 5x → 3_000 bps, 10x → 6_000 bps, 20x+ → 10_000 bps
        ((ratio.saturating_sub(5)) * 1_000).min(10_000)
    }

    /// Bilinmeyen alıcı oranı skoru (0-10_000 bps).
    fn compute_unknown_recipient_score(profile: &BehaviorProfile, proposed_txs: &[TxRecord]) -> u32 {
        let outgoing: Vec<_> = proposed_txs.iter().filter(|tx| tx.is_outgoing).collect();
        if outgoing.is_empty() {
            return 0;
        }
        let unknown = outgoing.iter().filter(|tx| {
            !profile.known_recipients.contains(&tx.recipient)
        }).count();
        // (unknown / total) * 10_000
        ((unknown as u32) * 10_000) / (outgoing.len() as u32)
    }

    /// Yüksek frekans skoru (0-10_000 bps).
    fn compute_frequency_score(
        profile: &BehaviorProfile,
        proposed_txs: &[TxRecord],
        current_slot: SlotId,
    ) -> u32 {
        // Son kısa penceredeki mevcut tx sayısı + proposed
        let existing = profile.tx_count_in_window(current_slot, SHORT_WINDOW_SLOTS);
        let new_txs = proposed_txs.iter().filter(|tx| tx.is_outgoing).count();
        let total = existing + new_txs;

        if total <= HIGH_FREQUENCY_TX_COUNT {
            return 0;
        }
        // HIGH_FREQUENCY_TX_COUNT'u ne kadar aştık?
        let excess = (total - HIGH_FREQUENCY_TX_COUNT) as u32;
        (excess * 1_000).min(10_000)
    }

    /// Çoklu alıcı skoru (0-10_000 bps).
    fn compute_multi_recipient_score(proposed_txs: &[TxRecord]) -> u32 {
        let distinct: HashSet<[u8; 20]> = proposed_txs
            .iter()
            .filter(|tx| tx.is_outgoing)
            .map(|tx| *tx.recipient.as_bytes())
            .collect();
        if distinct.len() <= MULTI_RECIPIENT_THRESHOLD {
            return 0;
        }
        let excess = (distinct.len() - MULTI_RECIPIENT_THRESHOLD) as u32;
        (excess * 500).min(10_000)
    }
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::ANOMALY_DETECTION_MIN_TX;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_tx(slot: u64, amount: u128, recipient: u8, outgoing: bool) -> TxRecord {
        TxRecord {
            slot: SlotId(slot),
            amount,
            recipient: addr(recipient),
            is_outgoing: outgoing,
        }
    }

    fn build_baseline_profile(detector: &AnomalyDetector, account: Address) {
        // ANOMALY_DETECTION_MIN_TX = 10 kadar normal işlem kaydediyoruz
        for i in 0..ANOMALY_DETECTION_MIN_TX {
            detector.record_transaction(
                account,
                make_tx(i as u64, 1_000_000, (i as u8 % 5) + 10, true),
            );
        }
    }

    // ─── BehaviorProfile ──────────────────────────────────────────────────────

    #[test]
    fn test_profile_baseline_not_established_before_min_tx() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        assert!(!profile.has_baseline());

        for i in 0..(ANOMALY_DETECTION_MIN_TX - 1) {
            profile.record_tx(make_tx(i as u64, 1000, 10, true));
        }
        assert!(!profile.has_baseline(), "henüz min tx sayısına ulaşılmadı");
    }

    #[test]
    fn test_profile_baseline_established_at_min_tx() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        for i in 0..ANOMALY_DETECTION_MIN_TX {
            profile.record_tx(make_tx(i as u64, 1000, 10, true));
        }
        assert!(profile.has_baseline(), "min tx sayısında baseline kurulmalı");
    }

    #[test]
    fn test_profile_known_recipients_tracked() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        profile.record_tx(make_tx(1, 1000, 10, true));
        profile.record_tx(make_tx(2, 1000, 11, true));
        profile.record_tx(make_tx(3, 1000, 10, true)); // tekrar addr(10)

        assert_eq!(profile.known_recipients.len(), 2, "tekrar eden alıcı eklenmemeli");
        assert!(profile.known_recipients.contains(&addr(10)));
        assert!(profile.known_recipients.contains(&addr(11)));
    }

    #[test]
    fn test_profile_incoming_tx_not_added_to_recipients() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        profile.record_tx(make_tx(1, 1000, 10, false)); // giriş işlemi
        assert_eq!(profile.known_recipients.len(), 0, "giriş tx alıcısı known_recipients'e eklenmemeli");
    }

    #[test]
    fn test_profile_running_average_amount() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        profile.record_tx(make_tx(1, 1_000_000, 10, true));
        let first_avg = profile.avg_outgoing_amount;
        assert_eq!(first_avg, 1_000_000);

        profile.record_tx(make_tx(2, 100_000, 10, true));
        // Kayan ortalama: (1_000_000 * 9 + 100_000) / 10 = 910_000
        assert_eq!(profile.avg_outgoing_amount, 910_000);
    }

    #[test]
    fn test_profile_window_size_capped() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        for i in 0..(BEHAVIOR_WINDOW_SIZE + 50) {
            profile.record_tx(make_tx(i as u64, 1000, 10, true));
        }
        assert_eq!(profile.tx_history.len(), BEHAVIOR_WINDOW_SIZE);
    }

    #[test]
    fn test_profile_tx_count_in_window() {
        let account = addr(1);
        let mut profile = BehaviorProfile::new(account);
        // slot 0-99: 10 tx
        for i in 0..10u64 {
            profile.record_tx(make_tx(i, 1000, 10, true));
        }
        // slot 1000-1009: 5 tx
        for i in 1000..1005u64 {
            profile.record_tx(make_tx(i, 1000, 11, true));
        }
        // current_slot = 1100, window = 250 → sadece 1000-1005 görünmeli
        let count = profile.tx_count_in_window(SlotId(1100), 250);
        assert_eq!(count, 5);
    }

    // ─── AnomalyDetector ──────────────────────────────────────────────────────

    #[test]
    fn test_detector_no_profile_returns_error() {
        let detector = AnomalyDetector::new();
        let account = addr(99);
        let err = detector
            .compute_anomaly_score(&account, &[], SlotId(100))
            .unwrap_err();
        assert!(matches!(err, FreezeError::InsufficientBehaviorHistory));
    }

    #[test]
    fn test_detector_profile_without_baseline_returns_error() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        // 5 tx ekle (min 10 gerekli)
        for i in 0..5u64 {
            detector.record_transaction(account, make_tx(i, 1000, 10, true));
        }
        let err = detector
            .compute_anomaly_score(&account, &[], SlotId(100))
            .unwrap_err();
        assert!(matches!(err, FreezeError::InsufficientBehaviorHistory));
    }

    #[test]
    fn test_detector_normal_tx_low_score() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // Normal işlem (avg = 1_000_000, proposed = 1_000_000)
        let proposed = vec![make_tx(100, 1_000_000, 10, true)]; // bilinen alıcı
        let result = detector.compute_anomaly_score(&account, &proposed, SlotId(100)).unwrap();

        // Bilinen alıcıya normal miktar → düşük skor
        assert!(result.score_bps < 3_000, "normal işlemde skor düşük olmalı: {}", result.score_bps);
    }

    #[test]
    fn test_detector_high_volume_raises_score() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // Ortalama 1_000_000, proposed = 50_000_000 (50x)
        let proposed = vec![make_tx(100, 50_000_000, 50, true)];
        let result = detector.compute_anomaly_score(&account, &proposed, SlotId(100)).unwrap();

        // Yüksek volume → yüksek skor
        assert!(result.score_bps > 2_000, "yüksek volume anomali skoru yükseltmeli: {}", result.score_bps);
    }

    #[test]
    fn test_detector_all_unknown_recipients_raises_score() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // Tüm alıcılar tamamen yabancı (200-209 arası)
        let proposed: Vec<TxRecord> = (200u8..205u8)
            .map(|r| make_tx(100, 1_000_000, r, true))
            .collect();
        let result = detector.compute_anomaly_score(&account, &proposed, SlotId(100)).unwrap();

        // Tümü bilinmeyen → yüksek skor
        assert!(result.score_bps > 2_000, "tümü bilinmeyen alıcı skoru yükseltmeli: {}", result.score_bps);
    }

    #[test]
    fn test_detector_multi_recipient_raises_score() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // 15 farklı alıcı (eşik 10)
        let proposed: Vec<TxRecord> = (100u8..115u8)
            .map(|r| make_tx(100, 500_000, r, true))
            .collect();
        let result = detector.compute_anomaly_score(&account, &proposed, SlotId(100)).unwrap();

        assert!(result.score_bps > 0, "çoklu alıcı anomali skoru sıfırdan büyük olmalı");
    }

    #[test]
    fn test_detector_high_frequency_raises_score() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // Son 250 slot içinde 30 tx (eşik 20)
        for i in 0..30u64 {
            detector.record_transaction(account, make_tx(1000 + i, 100_000, 10, true));
        }
        // Şimdi 5 daha öner
        let proposed: Vec<TxRecord> = (0..5u64)
            .map(|i| make_tx(1030 + i, 100_000, 11, true))
            .collect();
        let result = detector
            .compute_anomaly_score(&account, &proposed, SlotId(1035))
            .unwrap();

        assert!(result.score_bps > 0, "yüksek frekans anomali skoru sıfırdan büyük olmalı");
    }

    #[test]
    fn test_detector_incoming_tx_not_counted_as_anomaly() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        build_baseline_profile(&detector, account);

        // Sadece giriş işlemi → volume/frequency/recipient anomalisi olmamalı
        let proposed = vec![make_tx(100, 100_000_000, 50, false)]; // is_outgoing = false
        let result = detector.compute_anomaly_score(&account, &proposed, SlotId(100)).unwrap();

        assert_eq!(result.score_bps, 0, "sadece giriş işlemi anomali sayılmamalı");
    }

    #[test]
    fn test_detector_has_profile() {
        let detector = AnomalyDetector::new();
        let account = addr(1);
        assert!(!detector.has_profile(&account));
        detector.record_transaction(account, make_tx(1, 1000, 10, true));
        assert!(detector.has_profile(&account));
    }
}
