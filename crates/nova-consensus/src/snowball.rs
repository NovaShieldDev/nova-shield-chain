//! GÖREV 108: Snowball Parametreleri (Avalanche Konsensüs)
//!
//! ## Mimari
//!
//! ### Avalanche Snowball Parametreleri
//! - K=20: örnekleme büyüklüğü (her sorguda 20 validator'a sor)
//! - Alpha=15: alpha eşiği (15/20 = %75 çoğunluk gerekli)
//! - Beta=20: confidence threshold (ardışık 20 başarılı round → finalize)
//! - BetaVirtuous=15, BetaRogue=20 (Avalanche'da iki eşik var)
//!
//! ### Snowflake → Snowball → Avalanche
//! - Snowflake: tek bit karar (0/1 → tercih et veya etme)
//! - Snowball: birden fazla tercih birikimi (en çok vote alan kazanır)
//! - Avalanche: DAG üzerinde Snowball (işlem çakışma tespiti)
//!
//! ### Nova Uyarlaması
//! - `SnowballConfig`: parametreler
//! - `SnowballDecision`: tercih durumu (Prefer, Against, Undecided)
//! - `SnowballInstance`: tek bir mesele için Snowball örneği
//! - `SnowballEngine`: çoklu mesele yöneticisi

use nova_primitives::ValidatorId;
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Örnekleme büyüklüğü (her round'da kaç validator'a sorulur)
///
/// Avalanchego: `params.K = 20`
pub const SNOWBALL_K: usize = 20;

/// Alpha eşiği (kabul için gerekli minimum oy)
///
/// Avalanchego: `params.Alpha = 15` (K=20 üzerinden %75)
pub const SNOWBALL_ALPHA: usize = 15;

/// Güven eşiği — sanal işlemler (kendi transaction'ları)
///
/// Avalanchego: `params.BetaVirtuous = 15`
pub const SNOWBALL_BETA_VIRTUOUS: u32 = 15;

/// Güven eşiği — çelişkili işlemler (double-spend gibi)
///
/// Avalanchego: `params.BetaRogue = 20`
pub const SNOWBALL_BETA_ROGUE: u32 = 20;

/// Concurrency sınırı (aynı anda kaç paralel Snowball instance)
///
/// Avalanchego: `params.Concurrent = 4`
pub const SNOWBALL_CONCURRENT: usize = 4;

// ─── SnowballConfig ───────────────────────────────────────────────────────────

/// Snowball algoritması parametreleri
#[derive(Debug, Clone)]
pub struct SnowballConfig {
    pub k: usize,
    pub alpha: usize,
    pub beta_virtuous: u32,
    pub beta_rogue: u32,
}

impl SnowballConfig {
    pub fn default_nova() -> Self {
        Self {
            k: SNOWBALL_K,
            alpha: SNOWBALL_ALPHA,
            beta_virtuous: SNOWBALL_BETA_VIRTUOUS,
            beta_rogue: SNOWBALL_BETA_ROGUE,
        }
    }

    /// Alpha eşiği geçildi mi?
    pub fn has_alpha(&self, votes: usize) -> bool {
        votes >= self.alpha
    }

    /// Alpha oranı (yüzde olarak, bps)
    pub fn alpha_ratio_bps(&self) -> u64 {
        (self.alpha * 10_000 / self.k) as u64
    }
}

impl Default for SnowballConfig {
    fn default() -> Self {
        Self::default_nova()
    }
}

// ─── Tercih (Preference) ─────────────────────────────────────────────────────

/// Snowball kararı: tercih edilen değer
///
/// Avalanchego: `binary.Snowball.Preference() bool`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SnowballPreference {
    /// Değer 0 tercih edildi (e.g. "red")
    Zero,
    /// Değer 1 tercih edildi (e.g. "blue")
    One,
}

impl SnowballPreference {
    pub fn flip(&self) -> Self {
        match self {
            Self::Zero => Self::One,
            Self::One => Self::Zero,
        }
    }
}

// ─── SnowballInstance ─────────────────────────────────────────────────────────

/// Tek bir mesele için Snowball örneği
///
/// Avalanchego: `binary.Snowball { snowflake, preference, numSuccessfulPolls }`
#[derive(Debug, Clone)]
pub struct SnowballInstance {
    /// Mevcut tercih
    pub preference: SnowballPreference,
    /// Ardışık başarılı round sayısı (confidence counter)
    pub confidence: u32,
    /// Her seçenek için toplam oy sayısı
    pub vote_counts: HashMap<SnowballPreference, u32>,
    /// Finalize edildi mi?
    pub finalized: bool,
    /// Bu çelişkili mi? (rogue)
    pub is_rogue: bool,
    /// Toplam round sayısı
    pub rounds: u32,
}

impl SnowballInstance {
    pub fn new(initial: SnowballPreference, is_rogue: bool) -> Self {
        let mut vote_counts = HashMap::new();
        vote_counts.insert(SnowballPreference::Zero, 0);
        vote_counts.insert(SnowballPreference::One, 0);

        Self {
            preference: initial,
            confidence: 0,
            vote_counts,
            finalized: false,
            is_rogue,
            rounds: 0,
        }
    }

    /// Bir round'un sonucunu işle
    ///
    /// votes_for_one: One seçeneğine verilen oy sayısı
    /// votes_for_zero: Zero seçeneğine verilen oy sayısı
    ///
    /// Returns: tercih değişti mi?
    pub fn process_poll(&mut self, config: &SnowballConfig, votes_for_one: usize, votes_for_zero: usize) -> bool {
        if self.finalized { return false; }
        self.rounds += 1;

        let winner = if votes_for_one >= votes_for_zero && config.has_alpha(votes_for_one) {
            Some(SnowballPreference::One)
        } else if votes_for_zero > votes_for_one && config.has_alpha(votes_for_zero) {
            Some(SnowballPreference::Zero)
        } else {
            None // Alpha eşiği geçilemedi → confidence sıfırla
        };

        if let Some(pref) = winner {
            // Toplam oy sayısını güncelle
            *self.vote_counts.entry(pref.clone()).or_insert(0) += 1;

            let preference_changed = pref != self.preference;

            if pref == self.preference {
                // Tercih aynı → confidence artar
                self.confidence += 1;
            } else {
                // Tercih değişti → confidence sıfırla, yeni tercihe geç
                let new_count = *self.vote_counts.get(&pref).unwrap_or(&0);
                let old_count = *self.vote_counts.get(&self.preference).unwrap_or(&0);

                if new_count > old_count {
                    self.preference = pref;
                    self.confidence = 1;
                } else {
                    self.confidence = 1;
                }
            }

            // Beta eşiği kontrolü
            let beta = if self.is_rogue {
                config.beta_rogue
            } else {
                config.beta_virtuous
            };

            if self.confidence >= beta {
                self.finalized = true;
            }

            preference_changed
        } else {
            // Alpha geçilemedi → confidence sıfırla
            self.confidence = 0;
            false
        }
    }

    /// Mevcut beta eşiği
    pub fn beta(&self, config: &SnowballConfig) -> u32 {
        if self.is_rogue { config.beta_rogue } else { config.beta_virtuous }
    }

    /// İlerleme oranı (confidence/beta, bps)
    pub fn progress_bps(&self, config: &SnowballConfig) -> u64 {
        let beta = self.beta(config);
        if beta == 0 { return 10_000; }
        (self.confidence as u64 * 10_000 / beta as u64).min(10_000)
    }
}

// ─── SnowballEngine ───────────────────────────────────────────────────────────

/// Çoklu mesele Snowball yöneticisi
///
/// Avalanchego: `Consensus.RecordPoll(votes Bag)` (avalanche/consensus.go)
pub struct SnowballEngine {
    pub config: SnowballConfig,
    /// Aktif Snowball instance'ları: mesele_id → instance
    instances: HashMap<u64, SnowballInstance>,
    /// Finalize edilmiş kararlar: mesele_id → kazanan tercih
    finalized: HashMap<u64, SnowballPreference>,
    /// Toplam finalize sayısı
    total_finalized: u64,
}

impl SnowballEngine {
    pub fn new(config: SnowballConfig) -> Self {
        Self {
            config,
            instances: HashMap::new(),
            finalized: HashMap::new(),
            total_finalized: 0,
        }
    }

    /// Yeni bir mesele başlat
    pub fn add_issue(&mut self, issue_id: u64, initial: SnowballPreference, is_rogue: bool) -> bool {
        if self.instances.contains_key(&issue_id) || self.finalized.contains_key(&issue_id) {
            return false;
        }
        if self.instances.len() >= SNOWBALL_CONCURRENT {
            return false; // Kapasite dolu
        }
        self.instances.insert(issue_id, SnowballInstance::new(initial, is_rogue));
        true
    }

    /// Bir meseleye poll sonucunu kaydet
    ///
    /// Returns: finalize olduysa Some(kazanan tercih), olmadıysa None
    pub fn record_poll(
        &mut self,
        issue_id: u64,
        votes_for_one: usize,
        votes_for_zero: usize,
    ) -> Option<SnowballPreference> {
        let instance = self.instances.get_mut(&issue_id)?;
        instance.process_poll(&self.config, votes_for_one, votes_for_zero);

        if instance.finalized {
            let winner = instance.preference.clone();
            self.instances.remove(&issue_id);
            self.finalized.insert(issue_id, winner.clone());
            self.total_finalized += 1;
            Some(winner)
        } else {
            None
        }
    }

    /// Simulated poll: validator görüşlerine göre oy hesapla
    ///
    /// validators: (validator_id, preference) çiftleri
    /// K kadarı örneklenir (ya da hepsi K'dan azsa hepsi)
    pub fn sample_and_poll(
        &mut self,
        issue_id: u64,
        validators: &[(ValidatorId, SnowballPreference)],
    ) -> Option<SnowballPreference> {
        let sample_size = validators.len().min(self.config.k);
        let sampled = &validators[..sample_size];

        let votes_one = sampled.iter().filter(|(_, p)| *p == SnowballPreference::One).count();
        let votes_zero = sampled.iter().filter(|(_, p)| *p == SnowballPreference::Zero).count();

        self.record_poll(issue_id, votes_one, votes_zero)
    }

    /// Aktif mesele sayısı
    pub fn active_count(&self) -> usize {
        self.instances.len()
    }

    /// Finalize edilmiş mesele sayısı
    pub fn finalized_count(&self) -> usize {
        self.finalized.len()
    }

    /// Bir mesele için mevcut tercih
    pub fn preference(&self, issue_id: u64) -> Option<&SnowballPreference> {
        self.instances.get(&issue_id).map(|i| &i.preference)
    }

    /// Finalize edilmiş karar
    pub fn finalized_preference(&self, issue_id: u64) -> Option<&SnowballPreference> {
        self.finalized.get(&issue_id)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SnowballConfig {
        SnowballConfig::default_nova()
    }

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }

    // 1. Default parametreler doğru
    #[test]
    fn test_default_params() {
        let cfg = default_config();
        assert_eq!(cfg.k, 20);
        assert_eq!(cfg.alpha, 15);
        assert_eq!(cfg.beta_virtuous, 15);
        assert_eq!(cfg.beta_rogue, 20);
    }

    // 2. Alpha oranı %75
    #[test]
    fn test_alpha_ratio() {
        let cfg = default_config();
        assert_eq!(cfg.alpha_ratio_bps(), 7_500); // 15/20 = %75
        assert!(cfg.has_alpha(15));
        assert!(cfg.has_alpha(20));
        assert!(!cfg.has_alpha(14));
    }

    // 3. Virtuous consensus: beta=15 round sonunda finalize
    #[test]
    fn test_virtuous_finalize() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::One, false);

        // 15 ardışık round, her birinde 15+ oy One'a
        for _ in 0..15 {
            inst.process_poll(&config, 15, 5);
        }

        assert!(inst.finalized);
        assert_eq!(inst.preference, SnowballPreference::One);
        assert_eq!(inst.confidence, 15);
    }

    // 4. Rogue consensus: beta=20 gerekir
    #[test]
    fn test_rogue_needs_more_confidence() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::One, true);

        // 15 round — virtuous için yeterli ama rogue için değil
        for _ in 0..15 {
            inst.process_poll(&config, 15, 5);
        }
        assert!(!inst.finalized);

        // 5 round daha
        for _ in 0..5 {
            inst.process_poll(&config, 15, 5);
        }
        assert!(inst.finalized);
    }

    // 5. Alpha eşiği geçilmezse confidence sıfırlanır
    #[test]
    fn test_alpha_failure_resets_confidence() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::One, false);

        // 10 başarılı round
        for _ in 0..10 {
            inst.process_poll(&config, 15, 5);
        }
        assert_eq!(inst.confidence, 10);

        // Alpha geçilemiyor (14 < 15)
        inst.process_poll(&config, 10, 10);
        assert_eq!(inst.confidence, 0);
    }

    // 6. Tercih değişimi
    #[test]
    fn test_preference_flip() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::Zero, false);

        // Zero için 3 round
        for _ in 0..3 {
            inst.process_poll(&config, 5, 15); // Zero kazanıyor
        }
        assert_eq!(inst.preference, SnowballPreference::Zero);

        // One için art arda daha fazla → tercih değişebilir
        for _ in 0..5 {
            inst.process_poll(&config, 15, 5); // One kazanıyor
        }
        // One lehine 5 kez, Zero lehine 3 kez → One daha fazla vote
        // Tercih One olmalı
        assert_eq!(inst.preference, SnowballPreference::One);
    }

    // 7. Finalize sonrası değişim olmaz
    #[test]
    fn test_no_change_after_finalize() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::One, false);

        for _ in 0..15 {
            inst.process_poll(&config, 15, 5);
        }
        assert!(inst.finalized);

        // Şimdi Zero lehine poll
        let changed = inst.process_poll(&config, 5, 15);
        assert!(!changed);
        assert_eq!(inst.preference, SnowballPreference::One); // değişmedi
    }

    // 8. SnowballEngine yeni mesele ekle
    #[test]
    fn test_engine_add_issue() {
        let mut eng = SnowballEngine::new(default_config());
        assert!(eng.add_issue(1, SnowballPreference::One, false));
        assert!(!eng.add_issue(1, SnowballPreference::Zero, false)); // tekrar ekleme başarısız
        assert_eq!(eng.active_count(), 1);
    }

    // 9. Engine poll ve finalize
    #[test]
    fn test_engine_poll_finalize() {
        let mut eng = SnowballEngine::new(default_config());
        eng.add_issue(42, SnowballPreference::One, false);

        let mut winner = None;
        for _ in 0..20 {
            winner = eng.record_poll(42, 15, 5);
            if winner.is_some() { break; }
        }

        assert!(winner.is_some());
        assert_eq!(winner.unwrap(), SnowballPreference::One);
        assert_eq!(eng.finalized_count(), 1);
        assert_eq!(eng.active_count(), 0);
    }

    // 10. Engine concurrent kapasite sınırı
    #[test]
    fn test_engine_concurrent_limit() {
        let mut eng = SnowballEngine::new(default_config());
        for i in 0..SNOWBALL_CONCURRENT {
            assert!(eng.add_issue(i as u64, SnowballPreference::One, false));
        }
        // Kapasite dolu
        assert!(!eng.add_issue(99, SnowballPreference::One, false));
    }

    // 11. sample_and_poll validator listesiyle
    #[test]
    fn test_sample_and_poll() {
        let mut eng = SnowballEngine::new(default_config());
        eng.add_issue(1, SnowballPreference::One, false);

        // 16 One + 4 Zero validator
        let validators: Vec<(ValidatorId, SnowballPreference)> = (0..20)
            .map(|i| {
                let pref = if i < 16 { SnowballPreference::One } else { SnowballPreference::Zero };
                (vid(i), pref)
            })
            .collect();

        let mut winner = None;
        for _ in 0..20 {
            winner = eng.sample_and_poll(1, &validators);
            if winner.is_some() { break; }
        }

        assert!(winner.is_some());
    }

    // 12. Progress bps hesabı
    #[test]
    fn test_progress_bps() {
        let config = default_config();
        let mut inst = SnowballInstance::new(SnowballPreference::One, false); // beta=15

        assert_eq!(inst.progress_bps(&config), 0);

        for _ in 0..5 {
            inst.process_poll(&config, 15, 5);
        }
        // 5/15 ≈ 3333 bps
        let progress = inst.progress_bps(&config);
        assert!((progress as i64 - 3333).abs() <= 5);
    }

    // 13. Flip preference utility
    #[test]
    fn test_preference_flip_utility() {
        assert_eq!(SnowballPreference::Zero.flip(), SnowballPreference::One);
        assert_eq!(SnowballPreference::One.flip(), SnowballPreference::Zero);
    }

    // 14. Beta virtuous < beta rogue
    #[test]
    fn test_beta_ordering() {
        let cfg = default_config();
        assert!(cfg.beta_virtuous < cfg.beta_rogue);
    }
}
