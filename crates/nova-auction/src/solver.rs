//! Solver Rekabeti — CoW Protocol Autopilot Analog
//!
//! ## Araştırma Kaynağı: CoW Protocol
//!
//! CoW Protocol'de `driver/` (autopilot) her auction round'unda:
//! 1. Solver'lardan "batch solution" talep eder
//! 2. Her solution'ı "objective function" ile puanlar
//! 3. En yüksek puan alan solver'ı seçer (winner)
//! 4. Winner'ın settlement'ını on-chain gönderir
//!
//! ```text
//! CoW Protocol Go/Rust referansları (research/libs/cow-protocol/):
//!
//! // crates/driver/src/domain/competition/mod.rs:
//! pub struct Competition {
//!     pub auction: Auction,
//!     pub solutions: Vec<RankedSolution>,
//! }
//!
//! // crates/driver/src/domain/competition/solution/scoring.rs:
//! pub struct Score(BigDecimal);
//! impl Score {
//!     fn surplus(&self, order: &Order) -> BigDecimal { ... }
//! }
//!
//! // crates/driver/src/domain/competition/solution/trade.rs:
//! pub struct Trade {
//!     pub order: Order,
//!     pub executed: ExecutedAmounts,
//! }
//! ```
//!
//! ## Nova Uyarlaması
//!
//! Nova'da solver rekabeti şu şekilde çalışır:
//!
//! 1. Her slot'ta `BatchAuctionEngine` emirleri clearing engine'e verir
//! 2. (Gelecekte) birden fazla "routing path" solver'ı çözüm önerir
//! 3. Her çözüm surplus ile puanlanır
//! 4. En yüksek toplam surplus kazanır → nova-runtime'a teslim
//!
//! ## Surplus Hesabı (CoW Protocol Formülü)
//!
//! ```text
//! Buy order surplus:
//!   surplus = (limit_price - execution_price) * fill_amount / PRICE_PRECISION
//!
//! Sell order surplus:
//!   surplus = (execution_price - limit_price) * fill_amount / PRICE_PRECISION
//!
//! Total solution score = Σ surplus_i (tüm emirler için)
//! ```
//!
//! Uniform Price Auction'da tüm emirler aynı clearing price'ta işlenir,
//! bu nedenle limit_price > clearing_price olan emirler pozitif surplus alır.

use crate::types::{OrderId, OrderSide, PRICE_PRECISION};
use nova_primitives::types::SlotId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ─── SOLVER ID ───────────────────────────────────────────────────────────────

/// Solver kimliği (CoW Protocol solver identity analog).
///
/// CoW Protocol: Her solver'ın Ethereum adresi vardır.
/// Nova: 32 byte opaque kimlik.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct SolverId([u8; 32]);

impl SolverId {
    /// Solver ID oluştur.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// İlk N byte'dan solver oluştur (test yardımcısı).
    pub fn from_seed(seed: u8) -> Self {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        Self(bytes)
    }

    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hex gösterimi (ilk 8 byte).
    pub fn short_hex(&self) -> String {
        hex::encode(&self.0[..4])
    }
}

impl std::fmt::Display for SolverId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Solver({})", self.short_hex())
    }
}

// ─── TRADE EXECUTION (CoW Protocol Trade analog) ─────────────────────────────

/// Bir solver'ın önerdiği bireysel emir çalıştırması.
///
/// CoW Protocol (`crates/driver/src/domain/competition/solution/trade.rs`):
/// ```text
/// pub struct Trade {
///     pub order: Order,
///     pub executed: ExecutedAmounts { sell: Amount, buy: Amount },
/// }
/// ```
///
/// Nova'da: Her `TradeExecution` bir emir + onun çalıştırma detaylarını taşır.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeExecution {
    /// Hangi emir çalıştırılıyor.
    pub order_id: OrderId,
    /// Emir yönü (alım/satım).
    pub side: OrderSide,
    /// Limit fiyat (emir sahibinin max ödeyeceği / min alacağı).
    pub limit_price: u128,
    /// Gerçek çalıştırma fiyatı (clearing price).
    pub execution_price: u128,
    /// Doldurulacak miktar.
    pub fill_amount: u128,
    /// Bu emir kısmi doldurma kabul ediyor mu?
    pub is_partial: bool,
}

impl TradeExecution {
    /// Yeni TradeExecution oluştur.
    pub fn new(
        order_id: OrderId,
        side: OrderSide,
        limit_price: u128,
        execution_price: u128,
        fill_amount: u128,
        is_partial: bool,
    ) -> Self {
        Self {
            order_id,
            side,
            limit_price,
            execution_price,
            fill_amount,
            is_partial,
        }
    }

    /// CoW Protocol surplus formülü.
    ///
    /// Buy order surplus:  (limit_price - exec_price) * fill / PRICE_PRECISION
    /// Sell order surplus: (exec_price - limit_price) * fill / PRICE_PRECISION
    ///
    /// Emir limit fiyatının kötü tarafında ise surplus = 0 (bu bir geçersizlik
    /// durumudur — validation aşamasında zaten filtrelenmeli).
    pub fn surplus(&self) -> u128 {
        match self.side {
            OrderSide::Buy => {
                if self.limit_price >= self.execution_price {
                    let price_improvement = self.limit_price - self.execution_price;
                    // surplus = price_improvement * fill_amount / PRICE_PRECISION
                    price_improvement
                        .saturating_mul(self.fill_amount)
                        .saturating_div(PRICE_PRECISION)
                } else {
                    0
                }
            }
            OrderSide::Sell => {
                if self.execution_price >= self.limit_price {
                    let price_improvement = self.execution_price - self.limit_price;
                    price_improvement
                        .saturating_mul(self.fill_amount)
                        .saturating_div(PRICE_PRECISION)
                } else {
                    0
                }
            }
        }
    }

    /// Çalıştırma geçerli mi? (execution_price limit dahilinde mi?)
    pub fn is_valid(&self) -> bool {
        self.fill_amount > 0
            && match self.side {
                OrderSide::Buy => self.execution_price <= self.limit_price,
                OrderSide::Sell => self.execution_price >= self.limit_price,
            }
    }
}

// ─── BATCH SOLUTION (CoW Protocol Solution analog) ────────────────────────────

/// Bir solver'ın önerdiği tam batch çözümü.
///
/// CoW Protocol (`crates/driver/src/domain/competition/solution/mod.rs`):
/// ```text
/// pub struct Solution {
///     pub id: Id,
///     pub solver: eth::Address,
///     pub score: Score,
///     pub trades: Vec<Trade>,
/// }
/// ```
///
/// Nova'da her solver bir `BatchSolution` önerir. Engine en yüksek
/// surplus'lu solution'ı seçer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchSolution {
    /// Bu çözümü üreten solver.
    pub solver_id: SolverId,
    /// Hangi slot için çözüm.
    pub slot: SlotId,
    /// Önerilen trade çalıştırmaları.
    pub trades: Vec<TradeExecution>,
    /// Solver'ın beyan ettiği clearing price (doğrulama için).
    pub claimed_clearing_price: u128,
    /// Bu çözüm "trivial" mi (hiç trade yok)?
    pub is_trivial: bool,
}

impl BatchSolution {
    /// Yeni batch solution oluştur.
    pub fn new(
        solver_id: SolverId,
        slot: SlotId,
        trades: Vec<TradeExecution>,
        claimed_clearing_price: u128,
    ) -> Self {
        let is_trivial = trades.is_empty();
        Self {
            solver_id,
            slot,
            trades,
            claimed_clearing_price,
            is_trivial,
        }
    }

    /// Boş (trivial) solution — hiç emir eşleşmedi.
    pub fn trivial(solver_id: SolverId, slot: SlotId) -> Self {
        Self::new(solver_id, slot, vec![], 0)
    }

    /// CoW Protocol objective function: toplam surplus.
    ///
    /// CoW Protocol scoring.rs: `score = Σ surplus_i(order_i)`
    /// Nova'da da aynı: tüm trade'lerin surplus toplamı.
    pub fn total_surplus(&self) -> u128 {
        self.trades.iter().map(|t| t.surplus()).sum()
    }

    /// Kaç emir dolduruldu?
    pub fn filled_order_count(&self) -> usize {
        self.trades.len()
    }

    /// Kaç kısmi doldurma var?
    pub fn partial_fill_count(&self) -> usize {
        self.trades.iter().filter(|t| t.is_partial).count()
    }

    /// Toplam doldurulan miktar.
    pub fn total_fill_amount(&self) -> u128 {
        self.trades.iter().map(|t| t.fill_amount).sum()
    }

    /// Tüm trade'ler geçerli mi?
    pub fn is_valid(&self) -> bool {
        self.trades.iter().all(|t| t.is_valid())
    }

    /// Solution puan özeti.
    pub fn score_summary(&self) -> SolutionScore {
        SolutionScore {
            solver_id: self.solver_id,
            total_surplus: self.total_surplus(),
            filled_orders: self.filled_order_count(),
            partial_fills: self.partial_fill_count(),
            total_volume: self.total_fill_amount(),
            is_valid: self.is_valid(),
        }
    }
}

// ─── SOLUTION SCORE ───────────────────────────────────────────────────────────

/// Bir batch solution'ın puanı.
///
/// CoW Protocol `Score` analog — surplus + fee metric.
/// Nova'da fee henüz dahil değil (§8.x MEV ücret mekanizması ayrı tasarlanacak).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolutionScore {
    /// Hangi solver'ın puanı.
    pub solver_id: SolverId,
    /// Toplam surplus (ana sıralama kriteri).
    pub total_surplus: u128,
    /// Doldurulan emir sayısı (tie-breaking: daha fazla daha iyi).
    pub filled_orders: usize,
    /// Kısmi doldurma sayısı.
    pub partial_fills: usize,
    /// Toplam işlem hacmi.
    pub total_volume: u128,
    /// Tüm trade'ler geçerli mi?
    pub is_valid: bool,
}

impl SolutionScore {
    /// Bu puan diğerinden daha mı iyi?
    ///
    /// Sıralama kuralları (CoW Protocol ranking logic):
    /// 1. Geçersiz solution → her zaman kaybeder
    /// 2. Toplam surplus büyükse → kazanır
    /// 3. Eşit surplus'ta: daha fazla emir dolduran kazanır
    /// 4. Hâlâ eşit: daha yüksek hacimli kazanır
    pub fn is_better_than(&self, other: &SolutionScore) -> bool {
        if !self.is_valid {
            return false;
        }
        if !other.is_valid {
            return true;
        }
        if self.total_surplus != other.total_surplus {
            return self.total_surplus > other.total_surplus;
        }
        if self.filled_orders != other.filled_orders {
            return self.filled_orders > other.filled_orders;
        }
        self.total_volume > other.total_volume
    }
}

// ─── VCG ÖDEME HESABI ─────────────────────────────────────────────────────────

/// AŞAMA 7 Görev 52: VCG (Vickrey-Clarke-Groves) mekanizması.
///
/// VCG'de her solver'ın ödediği miktar, kendi katılımı sayesinde sağladığı
/// marjinal sosyal refah artışının dışındaki sosyal refahı kısaltır.
///
/// ## Formül
///
/// ```text
/// Sosyal refah (welfare) = Σ surplus_i (tüm kazanan emirlerin toplam artı değeri)
///
/// VCG Ödemesi: winner, kendi çözümünü çıkardığımızda
/// 2. sıra çözümün yarattığı refah kadar ödeme yapar.
///
/// vcg_payment(winner) = second_best_welfare  (ikinci en iyi çözümün surplus'u)
///
/// Bu sayede:
/// - Solver kendi gerçek surplus'unu ortaya koyar (truth-telling dominant strategy)
/// - Sybil saldırıları (aynı kişi birden fazla sahte solver olarak katılırsa)
///   zaten 2. sıra kendi çözümü olacağından kazanç sağlamaz
/// ```
///
/// ## Nova Uyarlaması
///
/// Şu an tam VCG implementasyonu mock olarak hesaplanır.
/// Gelecekte §8.x MEV ücret mekanizması ile entegre edilecek.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcgPayment {
    /// Kazanan solver.
    pub winner: SolverId,
    /// Kazanan surplus (kendi yarattığı refah).
    pub winner_surplus: u128,
    /// VCG ödemesi — ikinci en iyi surplus (winner bu kadar öder).
    pub vcg_payment: u128,
    /// Kazananın net kazancı = winner_surplus - vcg_payment.
    pub net_gain: u128,
}

impl VcgPayment {
    /// Solver rekabetinden VCG ödemesi hesapla.
    ///
    /// `solutions`: tüm geçerli çözümler (SolverId → surplus).
    pub fn compute(solutions: &BTreeMap<SolverId, u128>) -> Option<Self> {
        if solutions.is_empty() {
            return None;
        }

        // Sıralı: (surplus, solver_id) → deterministik
        let mut ranked: Vec<(u128, SolverId)> = solutions
            .iter()
            .map(|(id, &surplus)| (surplus, *id))
            .collect();
        // Azalan surplus, eşitlikte artan solver_id (en küçük id kazanır)
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        let (winner_surplus, winner) = ranked[0];
        // VCG payment = 2. sıra surplus (winner olmadan en iyi alternatif)
        let vcg_payment = if ranked.len() > 1 { ranked[1].0 } else { 0 };
        let net_gain = winner_surplus.saturating_sub(vcg_payment);

        Some(VcgPayment {
            winner,
            winner_surplus,
            vcg_payment,
            net_gain,
        })
    }
}

// ─── SOLVER COMPETITION (CoW Protocol Competition analog) ─────────────────────

/// Solver rekabeti yöneticisi.
///
/// CoW Protocol (`crates/driver/src/domain/competition/mod.rs`):
/// ```text
/// pub struct Competition {
///     auction: Auction,
///     solutions: Vec<RankedSolution>,
/// }
/// impl Competition {
///     pub fn winner(&self) -> Option<&RankedSolution> { ... }
/// }
/// ```
///
/// Nova'da: Her slot için solver'lar `BatchSolution` önerir.
/// `SolverCompetition` en yüksek surplus'lu solution'ı seçer.
///
/// ## Gelecek: Çoklu Solver
///
/// Şu an yalnızca dahili `ClearingEngine` solver olarak çalışır.
/// §10.x harici solver entegrasyonunda bu yapı genişletilebilir.
#[derive(Debug)]
pub struct SolverCompetition {
    /// Hangi slot için rekabet.
    slot: SlotId,
    /// Gönderilen çözümler (solver_id → solution). BTreeMap: deterministik iterasyon.
    solutions: BTreeMap<SolverId, BatchSolution>,
    /// Rekabet tamamlandı mı?
    finalized: bool,
}

impl SolverCompetition {
    /// Yeni solver rekabeti başlat.
    pub fn new(slot: SlotId) -> Self {
        Self {
            slot,
            solutions: BTreeMap::new(),
            finalized: false,
        }
    }

    /// Solver'dan çözüm al.
    ///
    /// CoW Protocol: `competition.add_solution(solution)` analog.
    /// Aynı solver'ın önceki çözümü üzerine yazar.
    pub fn submit_solution(&mut self, solution: BatchSolution) -> Result<(), SolverError> {
        if self.finalized {
            return Err(SolverError::CompetitionFinalized);
        }
        if solution.slot != self.slot {
            return Err(SolverError::WrongSlot {
                expected: self.slot,
                got: solution.slot,
            });
        }
        self.solutions.insert(solution.solver_id, solution);
        Ok(())
    }

    /// Rekabeti sonlandır ve kazananı seç.
    ///
    /// CoW Protocol: `competition.winner()` analog.
    /// En yüksek surplus'lu geçerli solution kazanır.
    pub fn finalize(&mut self) -> Option<&BatchSolution> {
        self.finalized = true;
        self.winner()
    }

    /// Mevcut en iyi solution'ı döndür (finalize etmeden).
    ///
    /// BUG-1 FIX: BTreeMap ile iterasyon deterministik (SolverId lexicographic sırası).
    /// Eşit surplus durumunda en küçük SolverId kazanır → tüm validatorlarda aynı sonuç.
    pub fn winner(&self) -> Option<&BatchSolution> {
        self.solutions
            .values()
            .filter(|s| s.is_valid())
            .max_by(|a, b| {
                let score_a = a.score_summary();
                let score_b = b.score_summary();
                if score_a.is_better_than(&score_b) {
                    std::cmp::Ordering::Greater
                } else if score_b.is_better_than(&score_a) {
                    std::cmp::Ordering::Less
                } else {
                    // Eşit surplus: en küçük SolverId kazanır (deterministik tie-break)
                    b.solver_id.cmp(&a.solver_id)
                }
            })
    }

    /// Tüm solution puanlarını döndür (sıralı).
    pub fn ranked_scores(&self) -> Vec<SolutionScore> {
        let mut scores: Vec<SolutionScore> = self
            .solutions
            .values()
            .map(|s| s.score_summary())
            .collect();
        // Azalan sırala (en iyisi başta)
        scores.sort_by(|a, b| {
            if a.is_better_than(b) {
                std::cmp::Ordering::Less
            } else if b.is_better_than(a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        scores
    }

    /// Kaç solver katıldı?
    pub fn participant_count(&self) -> usize {
        self.solutions.len()
    }

    /// Rekabet tamamlandı mı?
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    /// Geçerli solution sayısı.
    pub fn valid_solution_count(&self) -> usize {
        self.solutions.values().filter(|s| s.is_valid()).count()
    }
}

// ─── SOLVER HATALARI ─────────────────────────────────────────────────────────

/// Solver rekabet hataları.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolverError {
    #[error("rekabet zaten sonlandırıldı")]
    CompetitionFinalized,

    #[error("yanlış slot: beklenen={} gelen={}", expected.0, got.0)]
    WrongSlot { expected: SlotId, got: SlotId },

    #[error("geçersiz çözüm: {reason}")]
    InvalidSolution { reason: String },

    #[error("slot {0} için kilit zaten başka solver tarafından alındı")]
    LockAlreadyHeld(u64),

    #[error("solver kilidi yok, işlem yapamaz")]
    NoLockHeld,
}

// ─── LEADER LOCK TRACKER ─────────────────────────────────────────────────────

/// AŞAMA 7 Görev 51: Eş zamanlı çözüm gönderimini önleyen lider kilidi.
///
/// CoW Protocol'de tek bir "winner" settlement gönderir; diğerleri bloke edilir.
/// Nova'da `LeaderLockTracker` şunu sağlar:
/// - Bir slot için sadece bir solver aynı anda "aktif gönderici" olabilir
/// - Kilidi alan solver belirlenen süre içinde göndermek zorundadır
/// - Süre aşılırsa kilit otomatik serbest bırakılır
///
/// ## Pattern
/// CoW Protocol `driver/src/domain/competition/mod.rs`:
/// ```text
/// // Tek driver instance settlement gönderir
/// // Diğer driver'lar "already settled" hatası alır
/// ```
#[derive(Debug)]
pub struct LeaderLockTracker {
    /// Hangi solver hangi slot'u kilitliyor.
    lock: Option<LeaderLock>,
}

#[derive(Debug, Clone)]
struct LeaderLock {
    solver_id: SolverId,
    slot: SlotId,
    /// Kilidin sona ereceği slot.
    expires_at_slot: SlotId,
}

impl LeaderLockTracker {
    /// Yeni tracker oluştur.
    pub fn new() -> Self {
        Self { lock: None }
    }

    /// Solver belirtilen slot için kilit almak istiyor.
    ///
    /// Kilit süresi dolmuşsa veya başka slot için alınmışsa serbest bırakılır.
    /// Aynı slot için başka solver kilitliyse hata döner.
    pub fn try_acquire(
        &mut self,
        solver_id: SolverId,
        slot: SlotId,
        current_slot: SlotId,
        lock_duration_slots: u64,
    ) -> Result<(), SolverError> {
        // Mevcut kilidi kontrol et
        if let Some(ref lock) = self.lock {
            // Kilit süresi dolmuş veya farklı slot → serbest bırak
            if current_slot.0 > lock.expires_at_slot.0 || lock.slot != slot {
                self.lock = None;
            } else if lock.slot == slot && lock.solver_id != solver_id {
                // Aynı slot, farklı solver → bloke
                return Err(SolverError::LockAlreadyHeld(slot.0));
            }
            // Aynı slot, aynı solver → yenile (ok)
        }

        self.lock = Some(LeaderLock {
            solver_id,
            slot,
            expires_at_slot: SlotId(current_slot.0 + lock_duration_slots),
        });
        Ok(())
    }

    /// Kilidi serbest bırak (çözüm gönderimi sonrası).
    pub fn release(&mut self, solver_id: SolverId, slot: SlotId) -> Result<(), SolverError> {
        match &self.lock {
            Some(lock) if lock.solver_id == solver_id && lock.slot == slot => {
                self.lock = None;
                Ok(())
            }
            _ => Err(SolverError::NoLockHeld),
        }
    }

    /// Belirtilen solver ve slot için kilit aktif mi?
    pub fn is_locked_by(&self, solver_id: SolverId, slot: SlotId, current_slot: SlotId) -> bool {
        match &self.lock {
            Some(lock) => {
                lock.solver_id == solver_id
                    && lock.slot == slot
                    && current_slot.0 <= lock.expires_at_slot.0
            }
            None => false,
        }
    }

    /// Herhangi bir kilit var mı (süresi dolmamış)?
    pub fn has_active_lock(&self, current_slot: SlotId) -> bool {
        match &self.lock {
            Some(lock) => current_slot.0 <= lock.expires_at_slot.0,
            None => false,
        }
    }
}

impl Default for LeaderLockTracker {
    fn default() -> Self { Self::new() }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OrderId;

    fn make_order_id(seed: u8) -> OrderId {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        OrderId::from_bytes(bytes)
    }

    fn make_trade(seed: u8, side: OrderSide, limit: u128, exec: u128, fill: u128) -> TradeExecution {
        TradeExecution::new(
            make_order_id(seed),
            side,
            limit * PRICE_PRECISION,
            exec * PRICE_PRECISION,
            fill,
            false,
        )
    }

    fn make_solver(seed: u8) -> SolverId {
        SolverId::from_seed(seed)
    }

    // ─── SolverId ─────────────────────────────────────────────────────────────

    #[test]
    fn test_solver_id_unique() {
        let a = make_solver(1);
        let b = make_solver(2);
        assert_ne!(a, b);
    }

    #[test]
    fn test_solver_id_eq() {
        let a = make_solver(5);
        let b = SolverId::from_seed(5);
        assert_eq!(a, b);
    }

    #[test]
    fn test_solver_id_display() {
        let s = make_solver(0xAB);
        let repr = format!("{}", s);
        assert!(repr.starts_with("Solver("));
    }

    // ─── TradeExecution Surplus ────────────────────────────────────────────────

    /// Buy order: limit=210, exec=200 → surplus = (210-200) * fill / PREC
    #[test]
    fn test_trade_surplus_buy_positive() {
        let trade = make_trade(1, OrderSide::Buy, 210, 200, 1_000_000);
        let surplus = trade.surplus();
        // (210-200) * PRICE_PRECISION * 1_000_000 / PRICE_PRECISION = 10 * 1_000_000
        assert_eq!(surplus, 10 * 1_000_000);
    }

    /// Sell order: limit=190, exec=200 → surplus = (200-190) * fill / PREC
    #[test]
    fn test_trade_surplus_sell_positive() {
        let trade = make_trade(2, OrderSide::Sell, 190, 200, 1_000_000);
        let surplus = trade.surplus();
        assert_eq!(surplus, 10 * 1_000_000);
    }

    /// Buy order: limit == exec → surplus = 0 (tam eşit, fazla yok)
    #[test]
    fn test_trade_surplus_zero_when_at_limit() {
        let trade = make_trade(3, OrderSide::Buy, 200, 200, 5_000_000);
        assert_eq!(trade.surplus(), 0);
    }

    /// Buy order: exec > limit → geçersiz ama surplus = 0 (sıfıra clip)
    #[test]
    fn test_trade_surplus_buy_exec_above_limit() {
        let trade = make_trade(4, OrderSide::Buy, 200, 210, 1_000_000);
        assert_eq!(trade.surplus(), 0);
        assert!(!trade.is_valid());
    }

    /// Sell order: exec < limit → geçersiz, surplus = 0
    #[test]
    fn test_trade_surplus_sell_exec_below_limit() {
        let trade = make_trade(5, OrderSide::Sell, 200, 190, 1_000_000);
        assert_eq!(trade.surplus(), 0);
        assert!(!trade.is_valid());
    }

    // ─── BatchSolution ────────────────────────────────────────────────────────

    #[test]
    fn test_batch_solution_total_surplus() {
        let solver = make_solver(1);
        let slot = SlotId(100);
        let trades = vec![
            make_trade(1, OrderSide::Buy, 210, 200, 1_000_000),
            make_trade(2, OrderSide::Sell, 190, 200, 2_000_000),
        ];
        let solution = BatchSolution::new(solver, slot, trades, 200 * PRICE_PRECISION);
        // Buy surplus: 10 * 1_000_000 = 10_000_000
        // Sell surplus: 10 * 2_000_000 = 20_000_000
        assert_eq!(solution.total_surplus(), 30_000_000);
    }

    #[test]
    fn test_batch_solution_trivial() {
        let solver = make_solver(2);
        let slot = SlotId(200);
        let solution = BatchSolution::trivial(solver, slot);
        assert!(solution.is_trivial);
        assert_eq!(solution.total_surplus(), 0);
        assert_eq!(solution.filled_order_count(), 0);
    }

    #[test]
    fn test_batch_solution_partial_fill_count() {
        let solver = make_solver(3);
        let slot = SlotId(300);
        let mut t1 = make_trade(1, OrderSide::Buy, 210, 200, 500_000);
        t1.is_partial = true;
        let t2 = make_trade(2, OrderSide::Sell, 190, 200, 1_000_000);
        let solution = BatchSolution::new(solver, slot, vec![t1, t2], 200 * PRICE_PRECISION);
        assert_eq!(solution.partial_fill_count(), 1);
        assert_eq!(solution.filled_order_count(), 2);
    }

    // ─── SolverCompetition ────────────────────────────────────────────────────

    #[test]
    fn test_competition_winner_by_surplus() {
        let slot = SlotId(500);
        let mut comp = SolverCompetition::new(slot);

        // Solver 1: düşük surplus
        let low = BatchSolution::new(
            make_solver(1), slot,
            vec![make_trade(1, OrderSide::Buy, 205, 200, 1_000_000)],
            200 * PRICE_PRECISION,
        );
        // Solver 2: yüksek surplus
        let high = BatchSolution::new(
            make_solver(2), slot,
            vec![
                make_trade(1, OrderSide::Buy, 210, 200, 1_000_000),
                make_trade(2, OrderSide::Sell, 190, 200, 1_000_000),
            ],
            200 * PRICE_PRECISION,
        );

        comp.submit_solution(low).unwrap();
        comp.submit_solution(high).unwrap();

        let winner = comp.winner().unwrap();
        assert_eq!(winner.solver_id, make_solver(2), "Yüksek surplus kazanmalı");
    }

    #[test]
    fn test_competition_empty_no_winner() {
        let slot = SlotId(600);
        let comp = SolverCompetition::new(slot);
        assert!(comp.winner().is_none());
    }

    #[test]
    fn test_competition_wrong_slot_rejected() {
        let slot = SlotId(700);
        let mut comp = SolverCompetition::new(slot);

        let wrong_slot_solution = BatchSolution::new(
            make_solver(1), SlotId(701), vec![], 0,
        );
        let err = comp.submit_solution(wrong_slot_solution).unwrap_err();
        assert!(matches!(err, SolverError::WrongSlot { .. }));
    }

    #[test]
    fn test_competition_finalized_rejects_new() {
        let slot = SlotId(800);
        let mut comp = SolverCompetition::new(slot);
        comp.finalize();

        let solution = BatchSolution::trivial(make_solver(1), slot);
        let err = comp.submit_solution(solution).unwrap_err();
        assert!(matches!(err, SolverError::CompetitionFinalized));
    }

    #[test]
    fn test_competition_ranked_scores_order() {
        let slot = SlotId(900);
        let mut comp = SolverCompetition::new(slot);

        // 3 solver, farklı surplus
        for seed in 1u8..=3 {
            let amount = seed as u128 * 1_000_000;
            let solution = BatchSolution::new(
                make_solver(seed), slot,
                vec![make_trade(seed, OrderSide::Buy, 210, 200, amount)],
                200 * PRICE_PRECISION,
            );
            comp.submit_solution(solution).unwrap();
        }

        let scores = comp.ranked_scores();
        assert_eq!(scores.len(), 3);
        // İlk sıra en yüksek surplus
        assert!(scores[0].total_surplus >= scores[1].total_surplus);
        assert!(scores[1].total_surplus >= scores[2].total_surplus);
    }

    #[test]
    fn test_competition_solver_overwrites_own_solution() {
        let slot = SlotId(1000);
        let mut comp = SolverCompetition::new(slot);
        let solver = make_solver(1);

        // İlk çözüm: 1 trade
        let sol1 = BatchSolution::new(
            solver, slot,
            vec![make_trade(1, OrderSide::Buy, 210, 200, 1_000_000)],
            200 * PRICE_PRECISION,
        );
        // İkinci çözüm: 2 trade (daha iyi)
        let sol2 = BatchSolution::new(
            solver, slot,
            vec![
                make_trade(1, OrderSide::Buy, 210, 200, 2_000_000),
                make_trade(2, OrderSide::Sell, 190, 200, 1_000_000),
            ],
            200 * PRICE_PRECISION,
        );

        comp.submit_solution(sol1).unwrap();
        comp.submit_solution(sol2).unwrap();

        // Aynı solver → sadece son çözüm geçerli
        assert_eq!(comp.participant_count(), 1);
        let winner = comp.winner().unwrap();
        assert_eq!(winner.filled_order_count(), 2);
    }

    #[test]
    fn test_solution_score_invalid_beats_nothing() {
        // Geçersiz trade'li solution hiçbir zaman kazanamaz
        let solver1 = make_solver(1);
        let solver2 = make_solver(2);
        let slot = SlotId(1100);
        let mut comp = SolverCompetition::new(slot);

        // Geçersiz: buy order'da exec_price > limit_price
        let invalid = BatchSolution::new(
            solver1, slot,
            vec![make_trade(1, OrderSide::Buy, 190, 200, 1_000_000)],
            200 * PRICE_PRECISION,
        );
        // Geçerli ama trivial (0 surplus)
        let valid = BatchSolution::trivial(solver2, slot);

        comp.submit_solution(invalid).unwrap();
        comp.submit_solution(valid).unwrap();

        let winner = comp.winner().unwrap();
        assert_eq!(winner.solver_id, solver2, "Geçerli trivial, geçersiz'i yenmeli");
    }

    // ─── VcgPayment Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_vcg_single_solver() {
        let mut solutions = BTreeMap::new();
        solutions.insert(make_solver(1), 1_000_000u128);
        let vcg = VcgPayment::compute(&solutions).unwrap();
        assert_eq!(vcg.winner, make_solver(1));
        assert_eq!(vcg.vcg_payment, 0); // Tek solver → ödeme yok
        assert_eq!(vcg.net_gain, 1_000_000);
    }

    #[test]
    fn test_vcg_two_solvers() {
        let mut solutions = BTreeMap::new();
        solutions.insert(make_solver(1), 2_000_000u128); // kazanan
        solutions.insert(make_solver(2), 1_500_000u128); // 2. sıra
        let vcg = VcgPayment::compute(&solutions).unwrap();
        assert_eq!(vcg.winner, make_solver(1));
        assert_eq!(vcg.winner_surplus, 2_000_000);
        assert_eq!(vcg.vcg_payment, 1_500_000); // 2. sıra surplus
        assert_eq!(vcg.net_gain, 500_000);
    }

    #[test]
    fn test_vcg_tiebreak_smallest_id_wins() {
        let mut solutions = BTreeMap::new();
        // Eşit surplus — en küçük id kazanır
        solutions.insert(make_solver(3), 1_000_000u128);
        solutions.insert(make_solver(1), 1_000_000u128);
        solutions.insert(make_solver(2), 1_000_000u128);
        let vcg = VcgPayment::compute(&solutions).unwrap();
        assert_eq!(vcg.winner, make_solver(1));
        assert_eq!(vcg.vcg_payment, 1_000_000); // 2. sıra eşit
        assert_eq!(vcg.net_gain, 0);
    }

    #[test]
    fn test_vcg_empty_solutions() {
        let solutions: BTreeMap<SolverId, u128> = BTreeMap::new();
        assert!(VcgPayment::compute(&solutions).is_none());
    }

    // ─── LeaderLockTracker Testleri ───────────────────────────────────────────

    #[test]
    fn test_leader_lock_acquire_basic() {
        let mut tracker = LeaderLockTracker::new();
        let solver = make_solver(1);
        let slot = SlotId(100);

        tracker.try_acquire(solver, slot, slot, 3).unwrap();
        assert!(tracker.is_locked_by(solver, slot, slot));
    }

    #[test]
    fn test_leader_lock_blocks_other_solver() {
        let mut tracker = LeaderLockTracker::new();
        let s1 = make_solver(1);
        let s2 = make_solver(2);
        let slot = SlotId(200);

        tracker.try_acquire(s1, slot, slot, 5).unwrap();
        let err = tracker.try_acquire(s2, slot, slot, 5).unwrap_err();
        assert!(matches!(err, SolverError::LockAlreadyHeld(200)));
    }

    #[test]
    fn test_leader_lock_same_solver_renews() {
        let mut tracker = LeaderLockTracker::new();
        let solver = make_solver(1);
        let slot = SlotId(300);

        tracker.try_acquire(solver, slot, slot, 3).unwrap();
        // Aynı solver yeniden acquire → başarılı (yenileme)
        tracker.try_acquire(solver, slot, SlotId(302), 5).unwrap();
        assert!(tracker.is_locked_by(solver, slot, SlotId(306)));
    }

    #[test]
    fn test_leader_lock_expires() {
        let mut tracker = LeaderLockTracker::new();
        let s1 = make_solver(1);
        let s2 = make_solver(2);
        let slot = SlotId(400);

        tracker.try_acquire(s1, slot, slot, 3).unwrap(); // expires_at=403
        // Slot 404'te kilidi süresi doldu → s2 alabilir
        tracker.try_acquire(s2, slot, SlotId(404), 5).unwrap();
        assert!(tracker.is_locked_by(s2, slot, SlotId(404)));
    }

    #[test]
    fn test_leader_lock_release() {
        let mut tracker = LeaderLockTracker::new();
        let solver = make_solver(1);
        let slot = SlotId(500);

        tracker.try_acquire(solver, slot, slot, 5).unwrap();
        tracker.release(solver, slot).unwrap();
        assert!(!tracker.has_active_lock(slot));
    }

    #[test]
    fn test_leader_lock_release_wrong_solver() {
        let mut tracker = LeaderLockTracker::new();
        let s1 = make_solver(1);
        let s2 = make_solver(2);
        let slot = SlotId(600);

        tracker.try_acquire(s1, slot, slot, 5).unwrap();
        let err = tracker.release(s2, slot).unwrap_err();
        assert!(matches!(err, SolverError::NoLockHeld));
    }

    #[test]
    fn test_leader_lock_new_slot_clears_old() {
        let mut tracker = LeaderLockTracker::new();
        let s1 = make_solver(1);
        let s2 = make_solver(2);
        let slot1 = SlotId(700);
        let slot2 = SlotId(701);

        tracker.try_acquire(s1, slot1, slot1, 10).unwrap();
        // Farklı slot → eski kilit serbest bırakılır, s2 alabilir
        tracker.try_acquire(s2, slot2, SlotId(701), 5).unwrap();
        assert!(tracker.is_locked_by(s2, slot2, SlotId(701)));
    }
}
