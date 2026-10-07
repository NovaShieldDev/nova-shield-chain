//! GÖREV 105: Phragmén Guardian Seçim Algoritması
//!
//! ## Mimari
//!
//! ### Polkadot Sequential Phragmén
//! - Adaylar (candidateler): seçilebilir guardian'lar
//! - Oylar (votes): kullanıcılar guardian'lara stake ile oy verir
//! - Hedef: N guardian seç, minimum backing'i maksimize et
//! - Her seçimde: "yük" (load) en az artacak aday seçilir
//! - Aday yükü = (backing / seçilmiş kez) → daha az seçilmiş → daha düşük yük
//!
//! ### Nova Uyarlaması
//! - GuardianCandidate: stake, trust_score, capacity
//! - PhragmenVote: bir kullanıcının N guardian'a verdiği oy payı
//! - PhragmenResult: seçilen guardian seti + her birinin backing miktarı
//! - Kapasite kısıtı: bir guardian max_accounts kadar hesaba hizmet edebilir

use nova_primitives::Address;
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Varsayılan max seçilecek guardian sayısı (Polkadot'ta N=297)
pub const DEFAULT_GUARDIAN_SET_SIZE: usize = 21;

/// Minimum backing eşiği (bps — toplam stakeın yüzdesi)
pub const MIN_BACKING_BPS: u64 = 100; // %1 minimum backing

// ─── GuardianCandidate ────────────────────────────────────────────────────────

/// Phragmén seçiminde bir guardian adayı
///
/// Polkadot: `CandidateState { who, deposit, votes, .. }`
#[derive(Debug, Clone)]
pub struct GuardianCandidate {
    /// Guardian adresi
    pub address: Address,
    /// Guardian'ın kendi stake miktarı (self-bond)
    pub self_stake: u128,
    /// Trust skoru (0-10000 bps)
    pub trust_score_bps: u64,
    /// Maksimum kaç kullanıcıya hizmet verebilir
    pub max_accounts: u32,
    /// Mevcut aktif hesap sayısı
    pub current_accounts: u32,
    /// KYC doğrulandı mı?
    pub kyc_verified: bool,
}

impl GuardianCandidate {
    pub fn new(
        address: Address,
        self_stake: u128,
        trust_score_bps: u64,
        max_accounts: u32,
    ) -> Self {
        Self {
            address,
            self_stake,
            trust_score_bps,
            max_accounts,
            current_accounts: 0,
            kyc_verified: false,
        }
    }

    /// Kapasite doluluk oranı (bps)
    pub fn capacity_used_bps(&self) -> u64 {
        if self.max_accounts == 0 { return 10_000; }
        (self.current_accounts as u64 * 10_000 / self.max_accounts as u64).min(10_000)
    }

    /// Bu guardian seçilebilir mi?
    pub fn is_eligible(&self) -> bool {
        self.current_accounts < self.max_accounts && self.self_stake > 0
    }

    /// Efektif stake = self_stake * trust_score düzeltme faktörü
    ///
    /// Trust düşükse efektif ağırlık da düşer
    pub fn effective_weight(&self) -> u128 {
        self.self_stake * self.trust_score_bps as u128 / 10_000
    }
}

// ─── PhragmenVote ─────────────────────────────────────────────────────────────

/// Bir hesabın birden fazla guardian adayına dağıttığı oy
///
/// Polkadot: `VoterOf { who, stake, targets: Vec<AccountId> }`
#[derive(Debug, Clone)]
pub struct PhragmenVote {
    /// Oy kullanan hesap adresi
    pub voter: Address,
    /// Toplam stake miktarı
    pub stake: u128,
    /// Hangi guardian adaylarına oy verildi (eşit dağılım varsayılır)
    pub targets: Vec<Address>,
}

impl PhragmenVote {
    pub fn new(voter: Address, stake: u128, targets: Vec<Address>) -> Self {
        Self { voter, stake, targets }
    }

    /// Her adaya düşen stake payı (eşit dağılım)
    pub fn stake_per_target(&self) -> u128 {
        if self.targets.is_empty() { return 0; }
        self.stake / self.targets.len() as u128
    }
}

// ─── PhragmenResult ───────────────────────────────────────────────────────────

/// Phragmén seçim sonucu
#[derive(Debug, Clone)]
pub struct PhragmenResult {
    /// Seçilen guardian'lar ve aldıkları toplam backing
    pub winners: Vec<(Address, u128)>,
    /// Her guardian'ın backing detayı: guardian_addr → backing
    pub backing: HashMap<Address, u128>,
    /// Toplam dağıtılan stake
    pub total_stake: u128,
    /// Minimum backing (en az desteklenen guardian)
    pub min_backing: u128,
    /// Maksimum backing (en çok desteklenen guardian)
    pub max_backing: u128,
    /// Seçim skoru: min_backing'i maksimize et
    pub score: u128,
}

impl PhragmenResult {
    /// Seçilen guardian sayısı
    pub fn winner_count(&self) -> usize {
        self.winners.len()
    }

    /// Bu guardian seçildi mi?
    pub fn is_winner(&self, addr: &Address) -> bool {
        self.backing.contains_key(addr)
    }
}

// ─── PhragmenElection ─────────────────────────────────────────────────────────

/// Sequential Phragmén seçim motoru
///
/// Polkadot: `pallet_election_provider_multi_phase::unsigned::PhragmenElection`
pub struct PhragmenElection {
    /// Seçilecek guardian sayısı
    pub target_count: usize,
    /// Minimum self-stake zorunluluğu
    pub min_self_stake: u128,
    /// Trust score minimum zorunluluğu (bps)
    pub min_trust_bps: u64,
}

impl PhragmenElection {
    pub fn new(target_count: usize, min_self_stake: u128, min_trust_bps: u64) -> Self {
        Self {
            target_count,
            min_self_stake,
            min_trust_bps,
        }
    }

    /// Sequential Phragmén seçimi uygula
    ///
    /// Adım adım:
    /// 1. Uygun adayları filtrele
    /// 2. Her adayın başlangıç yükü = 0
    /// 3. N kez: en iyi adayı seç (backing en yüksek → yük en düşük)
    /// 4. Sonucu döndür
    ///
    /// Polkadot: `seq_phragmen(candidates, voters, to_elect, ..)`
    pub fn elect(
        &self,
        candidates: &[GuardianCandidate],
        votes: &[PhragmenVote],
    ) -> PhragmenResult {
        // 1. Uygun adaylar (filtrele)
        let eligible: Vec<&GuardianCandidate> = candidates
            .iter()
            .filter(|c| {
                c.is_eligible()
                    && c.self_stake >= self.min_self_stake
                    && c.trust_score_bps >= self.min_trust_bps
            })
            .collect();

        if eligible.is_empty() {
            return PhragmenResult {
                winners: vec![],
                backing: HashMap::new(),
                total_stake: 0,
                min_backing: 0,
                max_backing: 0,
                score: 0,
            };
        }

        // 2. Her adayın başlangıç backing'i = self_stake * trust_factor
        let mut backing: HashMap<Address, u128> = eligible
            .iter()
            .map(|c| (c.address, c.effective_weight()))
            .collect();

        // 3. Oy paylarını ekle
        let stake_per_target: HashMap<Address, u128> = {
            let mut map: HashMap<Address, u128> = HashMap::new();
            for vote in votes {
                let per = vote.stake_per_target();
                for target in &vote.targets {
                    if backing.contains_key(target) {
                        *map.entry(*target).or_insert(0) += per;
                    }
                }
            }
            map
        };

        for (addr, extra) in &stake_per_target {
            if let Some(b) = backing.get_mut(addr) {
                *b += extra;
            }
        }

        // 4. Phragmén yük hesabı
        // Polkadot'ta: her round'da en az yüklü adayı seç
        // Basitleştirilmiş: backing / (seçilme sırası) → daha az seçilmiş → daha yüksek öncelik
        // Nova: backing miktarına göre sırala, kapasite kısıtını uygula
        let mut sorted: Vec<(&GuardianCandidate, u128)> = eligible
            .iter()
            .map(|c| (*c, *backing.get(&c.address).unwrap_or(&0)))
            .collect();

        // Phragmén sıralama: en yüksek efektif backing önce
        // (gerçek Phragmén'de yük birden fazla tur gerektirir; bu basitleştirilmiş versiyon)
        sorted.sort_by(|a, b| {
            // Önce backing, sonra trust_score tiebreak
            b.1.cmp(&a.1)
                .then(b.0.trust_score_bps.cmp(&a.0.trust_score_bps))
        });

        // 5. İlk target_count elemanı seç
        let count = self.target_count.min(sorted.len());
        let mut winners: Vec<(Address, u128)> = Vec::with_capacity(count);
        let mut result_backing: HashMap<Address, u128> = HashMap::new();

        for (candidate, back) in sorted.iter().take(count) {
            winners.push((candidate.address, *back));
            result_backing.insert(candidate.address, *back);
        }

        // 6. İstatistikler
        let total_stake: u128 = votes.iter().map(|v| v.stake).sum::<u128>()
            + eligible.iter().map(|c| c.self_stake).sum::<u128>();

        let min_backing = winners.iter().map(|(_, b)| *b).min().unwrap_or(0);
        let max_backing = winners.iter().map(|(_, b)| *b).max().unwrap_or(0);

        PhragmenResult {
            winners,
            backing: result_backing,
            total_stake,
            min_backing,
            max_backing,
            score: min_backing, // Phragmén skoru = minimum backing
        }
    }

    /// Bir hesap için en iyi N guardian seç (bireysel koruma)
    ///
    /// Kullanıcı kaç guardian istiyorsa, en yüksek trust + kapasite olan seçilir
    pub fn select_for_account(
        &self,
        candidates: &[GuardianCandidate],
        required_count: usize,
        min_kyc: bool,
    ) -> Vec<Address> {
        let mut eligible: Vec<&GuardianCandidate> = candidates
            .iter()
            .filter(|c| {
                c.is_eligible()
                    && c.self_stake >= self.min_self_stake
                    && c.trust_score_bps >= self.min_trust_bps
                    && (!min_kyc || c.kyc_verified)
            })
            .collect();

        // Trust score + kapasite müsaitliğine göre sırala
        eligible.sort_by(|a, b| {
            // Dolu kapasiteliler sona
            let a_cap_pct = a.capacity_used_bps();
            let b_cap_pct = b.capacity_used_bps();

            b.trust_score_bps
                .cmp(&a.trust_score_bps)
                .then(a_cap_pct.cmp(&b_cap_pct)) // daha az dolu olanlar önce
                .then(b.self_stake.cmp(&a.self_stake))
        });

        eligible
            .iter()
            .take(required_count)
            .map(|c| c.address)
            .collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_candidate(n: u8, stake: u128, trust: u64, max_accts: u32) -> GuardianCandidate {
        GuardianCandidate::new(addr(n), stake, trust, max_accts)
    }

    fn make_election() -> PhragmenElection {
        PhragmenElection::new(5, 1_000, 5_000) // 5 guardian, min 1000 stake, min %50 trust
    }

    // 1. Temel seçim — en yüksek backing kazanır
    #[test]
    fn test_basic_election() {
        let candidates = vec![
            make_candidate(1, 10_000, 8_000, 100),
            make_candidate(2, 5_000, 7_000, 100),
            make_candidate(3, 1_000, 6_000, 100),
        ];

        let election = PhragmenElection::new(3, 100, 5_000);
        let result = election.elect(&candidates, &[]);

        assert_eq!(result.winner_count(), 3);
        // v1 en çok stake → en yüksek backing
        let top = &result.winners[0];
        assert_eq!(top.0, addr(1));
    }

    // 2. Hedef sayısı aday sayısından az
    #[test]
    fn test_target_count_less_than_candidates() {
        let candidates: Vec<GuardianCandidate> = (1..=10)
            .map(|n| make_candidate(n, 1_000 * n as u128, 8_000, 100))
            .collect();

        let election = PhragmenElection::new(3, 100, 0);
        let result = election.elect(&candidates, &[]);

        assert_eq!(result.winner_count(), 3);
    }

    // 3. Oy dağılımı backing'i artırır
    #[test]
    fn test_votes_increase_backing() {
        let candidates = vec![
            make_candidate(1, 1_000, 8_000, 100),
            make_candidate(2, 1_000, 8_000, 100),
        ];

        // Tüm oylar v1'e
        let votes = vec![
            PhragmenVote::new(addr(99), 50_000, vec![addr(1)]),
        ];

        let election = PhragmenElection::new(2, 100, 0);
        let result = election.elect(&candidates, &votes);

        let b1 = result.backing[&addr(1)];
        let b2 = result.backing[&addr(2)];
        assert!(b1 > b2, "b1={b1} should be > b2={b2}");
    }

    // 4. Minimum stake filtresi
    #[test]
    fn test_min_stake_filter() {
        let candidates = vec![
            make_candidate(1, 10_000, 8_000, 100), // geçer
            make_candidate(2, 500, 8_000, 100),    // min_stake=1000, elenir
        ];

        let election = PhragmenElection::new(5, 1_000, 0);
        let result = election.elect(&candidates, &[]);

        assert_eq!(result.winner_count(), 1);
        assert!(result.is_winner(&addr(1)));
        assert!(!result.is_winner(&addr(2)));
    }

    // 5. Minimum trust score filtresi
    #[test]
    fn test_min_trust_filter() {
        let candidates = vec![
            make_candidate(1, 5_000, 8_000, 100), // geçer
            make_candidate(2, 5_000, 3_000, 100), // min_trust=5000, elenir
        ];

        let election = PhragmenElection::new(5, 100, 5_000);
        let result = election.elect(&candidates, &[]);

        assert_eq!(result.winner_count(), 1);
        assert!(result.is_winner(&addr(1)));
    }

    // 6. Kapasite dolu guardian seçilmez
    #[test]
    fn test_full_capacity_excluded() {
        let mut c1 = make_candidate(1, 5_000, 8_000, 5);
        c1.current_accounts = 5; // dolu!

        let c2 = make_candidate(2, 1_000, 6_000, 5);

        let election = PhragmenElection::new(2, 100, 0);
        let result = election.elect(&[c1, c2], &[]);

        assert!(!result.is_winner(&addr(1))); // dolu → elendi
        assert!(result.is_winner(&addr(2)));
    }

    // 7. Boş aday → boş sonuç
    #[test]
    fn test_empty_candidates() {
        let election = make_election();
        let result = election.elect(&[], &[]);
        assert_eq!(result.winner_count(), 0);
        assert_eq!(result.score, 0);
    }

    // 8. Min/max backing hesabı
    #[test]
    fn test_min_max_backing() {
        let candidates = vec![
            make_candidate(1, 10_000, 8_000, 100),
            make_candidate(2, 2_000, 7_000, 100),
        ];

        let election = PhragmenElection::new(2, 100, 0);
        let result = election.elect(&candidates, &[]);

        assert!(result.max_backing >= result.min_backing);
        assert!(result.min_backing > 0);
    }

    // 9. Trust score efektif ağırlık hesabı
    #[test]
    fn test_effective_weight() {
        let c = make_candidate(1, 10_000, 5_000, 100); // %50 trust
        assert_eq!(c.effective_weight(), 5_000); // 10000 * 5000/10000

        let c2 = make_candidate(2, 10_000, 10_000, 100); // %100 trust
        assert_eq!(c2.effective_weight(), 10_000);
    }

    // 10. select_for_account — en iyi trust ile seç
    #[test]
    fn test_select_for_account() {
        let candidates = vec![
            make_candidate(1, 5_000, 9_000, 100),
            make_candidate(2, 5_000, 8_000, 100),
            make_candidate(3, 5_000, 7_000, 100),
            make_candidate(4, 5_000, 6_000, 100),
        ];

        let election = PhragmenElection::new(5, 100, 0);
        let selected = election.select_for_account(&candidates, 2, false);

        assert_eq!(selected.len(), 2);
        // En yüksek trust olanlar seçilmeli
        assert!(selected.contains(&addr(1)));
        assert!(selected.contains(&addr(2)));
    }

    // 11. KYC zorunluluğu
    #[test]
    fn test_kyc_required() {
        let mut c1 = make_candidate(1, 5_000, 9_000, 100);
        c1.kyc_verified = true;
        let c2 = make_candidate(2, 5_000, 8_000, 100); // KYC yok

        let election = PhragmenElection::new(5, 100, 0);
        let selected = election.select_for_account(&[c1, c2], 2, true); // KYC zorunlu

        assert_eq!(selected.len(), 1);
        assert!(selected.contains(&addr(1)));
        assert!(!selected.contains(&addr(2)));
    }

    // 12. Kapasite kullanım oranı hesabı
    #[test]
    fn test_capacity_used_bps() {
        let mut c = make_candidate(1, 5_000, 8_000, 100);
        c.current_accounts = 50;
        assert_eq!(c.capacity_used_bps(), 5_000); // %50

        c.current_accounts = 100;
        assert_eq!(c.capacity_used_bps(), 10_000); // %100 dolu
    }

    // 13. Phragmén skoru = minimum backing
    #[test]
    fn test_score_equals_min_backing() {
        let candidates: Vec<_> = (1..=5)
            .map(|n| make_candidate(n, 1_000 * n as u128, 8_000, 100))
            .collect();

        let election = PhragmenElection::new(5, 100, 0);
        let result = election.elect(&candidates, &[]);

        assert_eq!(result.score, result.min_backing);
    }

    // 14. Eşit oy dağılımı (birden fazla hedefe)
    #[test]
    fn test_equal_vote_distribution() {
        let vote = PhragmenVote::new(addr(99), 3_000, vec![addr(1), addr(2), addr(3)]);
        assert_eq!(vote.stake_per_target(), 1_000);
    }
}
