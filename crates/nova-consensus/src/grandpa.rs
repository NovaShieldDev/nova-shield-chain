//! GÖREV 103: GRANDPA 2-Round Finality (Polkadot pattern)
//!
//! ## Mimari
//!
//! ### Polkadot GRANDPA (GHOST-based Recursive ANcestor Deriving Prefix Agreement)
//! - 2 tur: Prevote + Precommit
//! - Prevote: en uzun zinciri (GHOST seçimi) oylar
//! - Precommit: prevote sürecinde 2/3+ quorum olan bloğu commit eder
//! - Asenkron güvenlik: ağ gecikmesine karşın dayanıklı
//! - Kesinlik: bir blok finalize olduktan sonra geri döndürülemez
//!
//! ### Nova Uyarlaması
//! - `GrandpaVote`: Prevote/Precommit + blok hash + round
//! - `GrandpaRound`: tur yönetimi, quorum takibi
//! - `GrandpaEngine`: çok-tur finality süreci

use nova_primitives::{BlockHash, SlotId, ValidatorId};
use std::collections::{HashMap, HashSet};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// GRANDPA quorum: 2/3+ validator onayı gerekli
pub const GRANDPA_QUORUM_NUM: u64 = 2;
pub const GRANDPA_QUORUM_DEN: u64 = 3;

/// Maksimum tur sayısı (sonsuz döngü önleme)
pub const MAX_GRANDPA_ROUNDS: u64 = 1_000;

/// Prevote timeout (slot)
pub const PREVOTE_TIMEOUT_SLOTS: u64 = 5;

/// Precommit timeout (slot)
pub const PRECOMMIT_TIMEOUT_SLOTS: u64 = 5;

// ─── VoteType ─────────────────────────────────────────────────────────────────

/// GRANDPA oy türü
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoteType {
    Prevote,
    Precommit,
}

// ─── GrandpaVote ──────────────────────────────────────────────────────────────

/// GRANDPA oyu
///
/// Polkadot: `grandpa::Vote { target_hash, target_number }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrandpaVote {
    pub vote_type: VoteType,
    pub validator: ValidatorId,
    pub round: u64,
    pub target_hash: BlockHash,
    pub target_height: u64,
    /// Mock imza (gerçek BLS12-381 sonra)
    pub signature: [u8; 32],
}

impl GrandpaVote {
    pub fn new(
        vote_type: VoteType,
        validator: ValidatorId,
        round: u64,
        target_hash: BlockHash,
        target_height: u64,
    ) -> Self {
        // Mock imza: validator_id + round + type hash
        let mut sig = [0u8; 32];
        sig[0..8].copy_from_slice(&validator.0.to_le_bytes());
        sig[8..16].copy_from_slice(&round.to_le_bytes());
        sig[16] = vote_type as u8;
        Self { vote_type, validator, round, target_hash, target_height, signature: sig }
    }
}

// ─── GrandpaRound ─────────────────────────────────────────────────────────────

/// Tek bir GRANDPA turu
///
/// Polkadot: `RoundState { prevote_ghost, precommit_ghost, completable }`
#[derive(Debug)]
pub struct GrandpaRound {
    pub round_number: u64,
    /// Prevote oyları: target_hash → oy verenler
    prevotes: HashMap<BlockHash, HashSet<ValidatorId>>,
    /// Precommit oyları
    precommits: HashMap<BlockHash, HashSet<ValidatorId>>,
    /// Oy veren validator'lar (tekrar oy önleme)
    prevote_voters: HashSet<ValidatorId>,
    precommit_voters: HashSet<ValidatorId>,
    /// Toplam validator sayısı
    total_validators: u64,
    /// Prevote quorum sağlandı mı?
    pub prevote_ghost: Option<BlockHash>,
    /// Finalize edilen blok
    pub finalized: Option<(BlockHash, u64)>,
}

impl GrandpaRound {
    pub fn new(round_number: u64, total_validators: u64) -> Self {
        Self {
            round_number,
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
            prevote_voters: HashSet::new(),
            precommit_voters: HashSet::new(),
            total_validators,
            prevote_ghost: None,
            finalized: None,
        }
    }

    /// Prevote ekle
    pub fn add_prevote(&mut self, vote: GrandpaVote) -> GrandpaResult<bool> {
        if vote.vote_type != VoteType::Prevote {
            return Err(GrandpaError::WrongVoteType);
        }
        if self.prevote_voters.contains(&vote.validator) {
            return Err(GrandpaError::DuplicateVote);
        }
        self.prevote_voters.insert(vote.validator);
        self.prevotes.entry(vote.target_hash.clone()).or_default().insert(vote.validator);

        // GHOST seçimi: en çok oy alan blok
        self.try_set_prevote_ghost();
        Ok(self.prevote_ghost.is_some())
    }

    /// Precommit ekle
    pub fn add_precommit(&mut self, vote: GrandpaVote) -> GrandpaResult<Option<(BlockHash, u64)>> {
        if vote.vote_type != VoteType::Precommit {
            return Err(GrandpaError::WrongVoteType);
        }
        if self.prevote_ghost.is_none() {
            return Err(GrandpaError::PrevoteNotComplete);
        }
        if self.precommit_voters.contains(&vote.validator) {
            return Err(GrandpaError::DuplicateVote);
        }

        self.precommit_voters.insert(vote.validator);
        let height = vote.target_height;
        self.precommits.entry(vote.target_hash.clone()).or_default().insert(vote.validator);

        // Precommit quorum kontrolü
        if let Some(winner) = self.find_precommit_winner() {
            let finalized = (winner.clone(), height);
            self.finalized = Some(finalized.clone());
            return Ok(Some(finalized));
        }
        Ok(None)
    }

    fn quorum_threshold(&self) -> u64 {
        (self.total_validators * GRANDPA_QUORUM_NUM + GRANDPA_QUORUM_DEN - 1) / GRANDPA_QUORUM_DEN
    }

    fn try_set_prevote_ghost(&mut self) {
        let threshold = self.quorum_threshold();
        for (hash, voters) in &self.prevotes {
            if voters.len() as u64 >= threshold {
                self.prevote_ghost = Some(hash.clone());
                return;
            }
        }
    }

    fn find_precommit_winner(&self) -> Option<BlockHash> {
        let threshold = self.quorum_threshold();
        for (hash, voters) in &self.precommits {
            if voters.len() as u64 >= threshold {
                return Some(hash.clone());
            }
        }
        None
    }

    /// Prevote sayısı (belirli blok için)
    pub fn prevote_count(&self, hash: &BlockHash) -> usize {
        self.prevotes.get(hash).map(|s| s.len()).unwrap_or(0)
    }

    /// Precommit sayısı (belirli blok için)
    pub fn precommit_count(&self, hash: &BlockHash) -> usize {
        self.precommits.get(hash).map(|s| s.len()).unwrap_or(0)
    }

    /// Toplam prevote sayısı
    pub fn total_prevotes(&self) -> usize {
        self.prevote_voters.len()
    }

    /// Toplam precommit sayısı
    pub fn total_precommits(&self) -> usize {
        self.precommit_voters.len()
    }

    /// Tur tamamlandı mı?
    pub fn is_complete(&self) -> bool {
        self.finalized.is_some()
    }
}

// ─── GrandpaEngine ────────────────────────────────────────────────────────────

/// GRANDPA finality motoru
///
/// Polkadot: `GrandpaBlockImport<B, BE, Client, SC>` + `Environment`
pub struct GrandpaEngine {
    /// Mevcut tur
    current_round: u64,
    /// Aktif tur
    active_round: Option<GrandpaRound>,
    /// Toplam validator sayısı
    total_validators: u64,
    /// Finalize edilmiş bloklar
    finalized_blocks: Vec<(u64, BlockHash, u64)>, // (round, hash, height)
}

impl GrandpaEngine {
    pub fn new(total_validators: u64) -> Self {
        let mut engine = Self {
            current_round: 0,
            active_round: None,
            total_validators,
            finalized_blocks: Vec::new(),
        };
        engine.start_round(1);
        engine
    }

    /// Yeni tur başlat
    pub fn start_round(&mut self, round: u64) {
        self.current_round = round;
        self.active_round = Some(GrandpaRound::new(round, self.total_validators));
    }

    /// Prevote gönder
    pub fn prevote(&mut self, validator: ValidatorId, hash: BlockHash, height: u64)
        -> GrandpaResult<bool>
    {
        let vote = GrandpaVote::new(VoteType::Prevote, validator, self.current_round, hash, height);
        let round = self.active_round.as_mut()
            .ok_or(GrandpaError::NoActiveRound)?;
        round.add_prevote(vote)
    }

    /// Precommit gönder
    pub fn precommit(&mut self, validator: ValidatorId, hash: BlockHash, height: u64)
        -> GrandpaResult<Option<BlockHash>>
    {
        let vote = GrandpaVote::new(VoteType::Precommit, validator, self.current_round, hash.clone(), height);
        let round = self.active_round.as_mut()
            .ok_or(GrandpaError::NoActiveRound)?;

        let result = round.add_precommit(vote)?;

        if let Some((fin_hash, fin_height)) = result {
            let round_num = self.current_round;
            self.finalized_blocks.push((round_num, fin_hash.clone(), fin_height));
            // Sonraki tur
            if round_num < MAX_GRANDPA_ROUNDS {
                self.start_round(round_num + 1);
            }
            return Ok(Some(fin_hash));
        }
        Ok(None)
    }

    /// Mevcut tur numarası
    pub fn current_round(&self) -> u64 {
        self.current_round
    }

    /// Finalize edilen blok sayısı
    pub fn finalized_count(&self) -> usize {
        self.finalized_blocks.len()
    }

    /// Son finalize edilen blok
    pub fn last_finalized(&self) -> Option<(BlockHash, u64)> {
        self.finalized_blocks.last().map(|(_, h, ht)| (h.clone(), *ht))
    }

    /// Aktif turun prevote quorum durumu
    pub fn has_prevote_ghost(&self) -> bool {
        self.active_round.as_ref()
            .and_then(|r| r.prevote_ghost.as_ref())
            .is_some()
    }

    /// Aktif tur tamamlandı mı?
    pub fn active_round_complete(&self) -> bool {
        self.active_round.as_ref()
            .map(|r| r.is_complete())
            .unwrap_or(false)
    }
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrandpaError {
    WrongVoteType,
    DuplicateVote,
    PrevoteNotComplete,
    NoActiveRound,
    RoundLimitReached,
}

pub type GrandpaResult<T> = Result<T, GrandpaError>;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }
    fn bh(n: u8) -> BlockHash { BlockHash::from_bytes([n; 32]) }

    fn make_engine_quorum(validators: u64) -> GrandpaEngine {
        GrandpaEngine::new(validators)
    }

    fn quorum_of(total: u64) -> u64 {
        (total * GRANDPA_QUORUM_NUM + GRANDPA_QUORUM_DEN - 1) / GRANDPA_QUORUM_DEN
    }

    // 1. Prevote quorum tetikleme
    #[test]
    fn test_prevote_quorum_achieved() {
        let mut engine = make_engine_quorum(3);
        let q = quorum_of(3); // 2

        for i in 1..=q {
            let result = engine.prevote(vid(i), bh(0xAA), 100).unwrap();
            if i == q { assert!(result); } // Son oy → quorum
        }
        assert!(engine.has_prevote_ghost());
    }

    // 2. Precommit sadece prevote sonrası
    #[test]
    fn test_precommit_requires_prevote() {
        let mut engine = make_engine_quorum(3);
        // Prevote quorum olmadan precommit → hata
        let err = engine.precommit(vid(1), bh(0xAA), 100).unwrap_err();
        assert_eq!(err, GrandpaError::PrevoteNotComplete);
    }

    // 3. Tam 2-tur finality
    #[test]
    fn test_full_two_round_finality() {
        let total = 4u64;
        let q = quorum_of(total); // 3
        let mut engine = make_engine_quorum(total);
        let target = bh(0xBB);

        // Prevote
        for i in 1..=q {
            engine.prevote(vid(i), target.clone(), 50).unwrap();
        }
        assert!(engine.has_prevote_ghost());

        // Precommit
        let mut finalized = None;
        for i in 1..=q {
            finalized = engine.precommit(vid(i), target.clone(), 50).unwrap();
        }
        assert!(finalized.is_some());
        assert_eq!(finalized.unwrap(), target);
        assert_eq!(engine.finalized_count(), 1);
    }

    // 4. Duplicate vote reddedilir
    #[test]
    fn test_duplicate_prevote_rejected() {
        let mut engine = make_engine_quorum(3);
        engine.prevote(vid(1), bh(0xAA), 100).unwrap();
        let err = engine.prevote(vid(1), bh(0xAA), 100).unwrap_err(); // aynı validator
        assert_eq!(err, GrandpaError::DuplicateVote);
    }

    // 5. Yanlış oy türü
    #[test]
    fn test_wrong_vote_type() {
        let mut engine = make_engine_quorum(3);
        // Precommit vote → prevote round'una ekle → hata
        let round = engine.active_round.as_mut().unwrap();
        let vote = GrandpaVote::new(VoteType::Precommit, vid(1), 1, bh(0xAA), 100);
        let err = round.add_prevote(vote).unwrap_err();
        assert_eq!(err, GrandpaError::WrongVoteType);
    }

    // 6. Sonraki tur otomatik başlar
    #[test]
    fn test_next_round_starts_after_finality() {
        let total = 3u64;
        let q = quorum_of(total);
        let mut engine = make_engine_quorum(total);
        let target = bh(0xCC);

        for i in 1..=q { engine.prevote(vid(i), target.clone(), 10).unwrap(); }
        for i in 1..=q { engine.precommit(vid(i), target.clone(), 10).unwrap(); }

        // Sonraki tur 2 olmalı
        assert_eq!(engine.current_round(), 2);
    }

    // 7. Quorum threshold hesabı
    #[test]
    fn test_quorum_threshold() {
        assert_eq!(quorum_of(3), 2);  // ceil(3*2/3) = 2
        assert_eq!(quorum_of(4), 3);  // ceil(4*2/3) = 3
        assert_eq!(quorum_of(6), 4);  // ceil(6*2/3) = 4
        assert_eq!(quorum_of(9), 6);  // ceil(9*2/3) = 6
    }

    // 8. Farklı bloklara oy
    #[test]
    fn test_split_vote_no_quorum() {
        let mut engine = make_engine_quorum(4);
        // 2 validator farklı bloklara oy → quorum yok
        engine.prevote(vid(1), bh(0xAA), 10).unwrap();
        engine.prevote(vid(2), bh(0xBB), 10).unwrap();
        assert!(!engine.has_prevote_ghost());
    }

    // 9. Prevote count
    #[test]
    fn test_prevote_count() {
        let mut engine = make_engine_quorum(5);
        let target = bh(0xDD);
        engine.prevote(vid(1), target.clone(), 5).unwrap();
        engine.prevote(vid(2), target.clone(), 5).unwrap();

        let round = engine.active_round.as_ref().unwrap();
        assert_eq!(round.prevote_count(&target), 2);
        assert_eq!(round.total_prevotes(), 2);
    }

    // 10. Last finalized
    #[test]
    fn test_last_finalized() {
        let total = 3u64;
        let q = quorum_of(total);
        let mut engine = make_engine_quorum(total);
        assert!(engine.last_finalized().is_none());

        let target = bh(0xEE);
        for i in 1..=q { engine.prevote(vid(i), target.clone(), 77).unwrap(); }
        for i in 1..=q { engine.precommit(vid(i), target.clone(), 77).unwrap(); }

        let (fin_hash, fin_height) = engine.last_finalized().unwrap();
        assert_eq!(fin_hash, target);
        assert_eq!(fin_height, 77);
    }
}
