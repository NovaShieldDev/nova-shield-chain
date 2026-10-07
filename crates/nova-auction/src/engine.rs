//! Batch Auction Ana Motoru.
//!
//! §5.3 — Tüm bileşenleri bir araya getiren ana orkestrasyın katmanı.
//!
//! ## Tasarım İlhamı: CoW Autopilot + Rolling Shutter Keyper
//!
//! **CoW Protocol Autopilot'tan:**
//! - Auction run loop: her slot'ta bir kez tetiklenir
//! - Solvable orders filtreleme → validation → auction
//! - Solution ranking ve winner selection
//! - Circuit breaker: misbehaving durumda otomatik durdurma
//!
//! **Rolling Shutter Keyper'dan:**
//! - `DecryptionTrigger` → Nova'da slot finalization trigger
//! - Slot-based execution: her slot bağımsız, deterministik
//! - P2P mesajlaşma → Nova'da guardian ağı (nova-guardian'a bağlanacak)
//! - `KeyperCore.Start()` → `BatchAuctionEngine.process_slot()`
//!
//! ## Engine Durumları
//!
//! ```text
//! ┌─────────────┐
//! │   Created   │ → init()
//! └─────┬───────┘
//!       ↓
//! ┌─────────────┐
//! │    Ready    │ ← slot emirleri bekleniyor
//! └─────┬───────┘
//!       ↓
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │  process_slot(slot, orders):                                        │
//! │  1. validate_batch()  → geçersiz emirleri filtrele                 │
//! │  2. build_orderbook() → OrderBook inşa et                          │
//! │  3. execute_batch()   → clearing price bul, emirleri çalıştır     │
//! │  4. update_stats()    → telemetri güncelle                         │
//! │  5. ClearingResult    → nova-runtime'a teslim et                   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

use crate::errors::{AuctionError, AuctionResult};
use crate::execution::ExecutionEngine;
use crate::orderbook::OrderBook;
use crate::types::{AuctionConfig, AuctionStats, BatchAuction, ClearingResult, Order, OrderBookSummary, TokenPair};
use crate::validation::OrderValidator;
use nova_primitives::types::SlotId;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

// ─── ENGINE STATE ─────────────────────────────────────────────────────────────

/// Thread-safe engine state.
///
/// `parking_lot::RwLock` kullanılır — CoW Protocol'ün Tokio mutex'i yerine
/// Nova'nın senkron execution modeliyle uyumlu.
#[derive(Debug)]
struct EngineState {
    /// İşlenmiş slotlar (tekrar çalışmayı önle).
    processed_slots: HashMap<SlotId, ClearingResult>,

    /// Biriktirilen istatistikler.
    stats: AuctionStats,

    /// Son işlenen slot.
    last_processed_slot: Option<SlotId>,
}

impl EngineState {
    fn new() -> Self {
        Self {
            processed_slots: HashMap::new(),
            stats: AuctionStats::default(),
            last_processed_slot: None,
        }
    }
}

// ─── BATCH AUCTION ENGINE ─────────────────────────────────────────────────────

/// Nova Shield Chain Batch Auction Ana Motoru.
///
/// §5.3 — Sealed-Bid Batch Auction orkestrasyonunu yönetir.
///
/// Thread-safe: `Arc<RwLock<EngineState>>` ile birden fazla thread kullanabilir.
///
/// ## Kullanım
///
/// ```no_run
/// # use nova_auction::engine::BatchAuctionEngine;
/// # use nova_auction::types::AuctionConfig;
/// # use nova_primitives::types::SlotId;
/// let engine = BatchAuctionEngine::new(AuctionConfig::default());
/// // let result = engine.process_slot(SlotId(42), orders, SlotId(42)).unwrap();
/// ```
pub struct BatchAuctionEngine {
    /// Yapılandırma.
    config: AuctionConfig,

    /// Doğrulama motoru.
    validator: OrderValidator,

    /// Execution motoru.
    executor: ExecutionEngine,

    /// Mutable state (thread-safe).
    state: Arc<RwLock<EngineState>>,
}

impl BatchAuctionEngine {
    /// Yeni engine oluştur.
    pub fn new(config: AuctionConfig) -> Self {
        let validator = OrderValidator::new(config.clone());
        let executor = ExecutionEngine::new(
            config.allow_partial_fills,
            config.max_carryover_orders,
            config.min_clearing_volume,
        );

        info!(
            max_orders_per_slot = config.max_orders_per_slot,
            allow_partial_fills = config.allow_partial_fills,
            chain_id = config.chain_id,
            "BatchAuctionEngine başlatılıyor"
        );

        Self {
            config,
            validator,
            executor,
            state: Arc::new(RwLock::new(EngineState::new())),
        }
    }

    /// Varsayılan yapılandırma ile engine.
    pub fn with_defaults() -> Self {
        Self::new(AuctionConfig::default())
    }

    /// Bir slot'u işle: emirleri doğrula, emir kitabı inşa et, çalıştır.
    ///
    /// Bu fonksiyon Rolling Shutter'ın `epochkghandler.KeyShareHandler`
    /// benzeri bir "tetikleyici alındı → işle" döngüsüdür.
    ///
    /// # Parametreler
    /// - `slot`: İşlenecek slot numarası
    /// - `orders`: Bu slot için gelen emirler (nova-mempool'dan reveal edilmiş)
    /// - `current_slot`: Mevcut chain slotu (geçerlilik kontrolü için)
    ///
    /// # Hatalar
    /// - `BatchAlreadyExecuted`: Bu slot zaten işlendi
    /// - `EmptyBatch`: Geçerli emir yok
    /// - `UnilateralBatch`: Tek taraflı (sadece alım veya satım)
    /// - `NoPriceIntersection`: Alım/satım fiyatları kesişmiyor
    pub fn process_slot(
        &self,
        slot: SlotId,
        orders: Vec<Order>,
        current_slot: SlotId,
        pair: TokenPair,
    ) -> AuctionResult<ClearingResult> {
        // ── 1. Tekrar çalışma koruması ─────────────────────────────────────
        {
            let state = self.state.read();
            if state.processed_slots.contains_key(&slot) {
                return Err(AuctionError::BatchAlreadyExecuted { slot });
            }
        }

        info!(
            slot = slot.0,
            order_count = orders.len(),
            "Slot işleniyor"
        );

        let start_us = self.current_time_us();

        // ── 2. Emir doğrulama ──────────────────────────────────────────────
        let (valid_orders, invalid_orders) = self.validator.validate_batch(orders, current_slot);

        debug!(
            slot = slot.0,
            valid = valid_orders.len(),
            invalid = invalid_orders.len(),
            "Emir doğrulama tamamlandı"
        );

        if valid_orders.is_empty() {
            return Err(AuctionError::EmptyBatch { slot });
        }

        // ── 3. Kapasite kontrolü ───────────────────────────────────────────
        if valid_orders.len() > self.config.max_orders_per_slot {
            return Err(AuctionError::OrderBookFull {
                current: valid_orders.len(),
                maximum: self.config.max_orders_per_slot,
            });
        }

        // ── 4. Emir kitabı inşası ──────────────────────────────────────────
        let mut book = OrderBook::new(slot, pair, self.config.max_orders_per_slot);

        for order in valid_orders {
            if let Err(err) = book.add_order(order) {
                warn!(slot = slot.0, err = %err, "Emir kitabına eklenemedi");
            }
        }

        let book_summary = book.summary();
        debug!(
            slot = slot.0,
            buy_orders = book_summary.buy_order_count,
            sell_orders = book_summary.sell_order_count,
            best_bid = ?book_summary.best_bid,
            best_ask = ?book_summary.best_ask,
            "Emir kitabı inşa edildi"
        );

        // ── 5. Batch execution ────────────────────────────────────────────
        let elapsed_us = self.current_time_us().saturating_sub(start_us);
        let mut result = self.executor.execute_batch(&book, slot, elapsed_us)?;

        // Computation time'ı gerçek değerle güncelle
        result.computation_time_us = self.current_time_us().saturating_sub(start_us);

        // ── 6. State güncelle ─────────────────────────────────────────────
        {
            let mut state = self.state.write();
            state.stats.update_from_result(&result);
            state.last_processed_slot = Some(slot);
            state.processed_slots.insert(slot, result.clone());
        }

        info!(
            slot = slot.0,
            clearing_price = result.clearing_price,
            executed = result.filled_count(),
            cancelled = result.cancelled_count(),
            carryover = result.carryover_count(),
            computation_us = result.computation_time_us,
            "Slot başarıyla işlendi"
        );

        Ok(result)
    }

    /// Bir slot için emir kitabı özeti (auction öncesi analiz).
    pub fn analyze_orderbook(
        &self,
        slot: SlotId,
        orders: Vec<Order>,
        current_slot: SlotId,
        pair: TokenPair,
    ) -> AuctionResult<OrderBookSummary> {
        let (valid_orders, _) = self.validator.validate_batch(orders, current_slot);

        let mut book = OrderBook::new(slot, pair, self.config.max_orders_per_slot);
        for order in valid_orders {
            let _ = book.add_order(order);
        }

        Ok(book.summary())
    }

    /// Bu slot daha önce işlendi mi?
    pub fn is_slot_processed(&self, slot: SlotId) -> bool {
        self.state.read().processed_slots.contains_key(&slot)
    }

    /// Slot sonucunu getir (işlenmişse).
    pub fn get_slot_result(&self, slot: SlotId) -> Option<ClearingResult> {
        self.state.read().processed_slots.get(&slot).cloned()
    }

    /// Son işlenen slot.
    pub fn last_processed_slot(&self) -> Option<SlotId> {
        self.state.read().last_processed_slot
    }

    /// Birikmiş istatistikler.
    pub fn stats(&self) -> AuctionStats {
        self.state.read().stats.clone()
    }

    /// Yapılandırma.
    pub fn config(&self) -> &AuctionConfig {
        &self.config
    }

    /// Sistem saati (mikrosaniye). Monotonic clock kullanılır.
    fn current_time_us(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Order, OrderClass, OrderId, OrderSide, TokenId, PRICE_PRECISION};
    use nova_primitives::types::{Address, SlotId};

    fn test_pair() -> TokenPair {
        TokenPair::new(TokenId([1u8; 20]), TokenId::NSC)
    }

    fn make_order(side: OrderSide, amount: u128, price: u128, seed: u8) -> Order {
        let mut id_bytes = [0u8; 32];
        id_bytes[0] = seed;
        Order {
            id: OrderId::from_bytes(id_bytes),
            side,
            pair: test_pair(),
            amount: amount.max(1_000_000), // minimum garantisi
            limit_price: price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(10_000),
            trader: Address::from_bytes([seed; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    fn engine() -> BatchAuctionEngine {
        BatchAuctionEngine::with_defaults()
    }

    fn orders_basic() -> Vec<Order> {
        vec![
            make_order(OrderSide::Buy, 1_000_000, 200, 1),
            make_order(OrderSide::Sell, 1_000_000, 198, 2),
        ]
    }

    // ─── Temel Engine Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_engine_olusturma() {
        let eng = engine();
        assert!(eng.last_processed_slot().is_none());
        assert_eq!(eng.stats().total_slots_processed, 0);
    }

    #[test]
    fn test_basit_slot_isleme() {
        let eng = engine();
        let result = eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap();

        assert!(result.has_fills());
        assert_eq!(result.slot, SlotId(1));
        assert!(result.clearing_price > 0);
    }

    #[test]
    fn test_zaten_islenmis_slot_hata() {
        let eng = engine();
        eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap();

        let err = eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::BatchAlreadyExecuted { slot: SlotId(1) }));
    }

    #[test]
    fn test_slot_islendi_mi_kontrolu() {
        let eng = engine();
        assert!(!eng.is_slot_processed(SlotId(1)));

        eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap();
        assert!(eng.is_slot_processed(SlotId(1)));
        assert!(!eng.is_slot_processed(SlotId(2)));
    }

    #[test]
    fn test_slot_sonucu_getirme() {
        let eng = engine();
        assert!(eng.get_slot_result(SlotId(1)).is_none());

        eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap();
        let result = eng.get_slot_result(SlotId(1));
        assert!(result.is_some());
        assert_eq!(result.unwrap().slot, SlotId(1));
    }

    #[test]
    fn test_son_islenen_slot_guncelleniyor() {
        let eng = engine();
        assert!(eng.last_processed_slot().is_none());

        eng.process_slot(SlotId(5), orders_basic(), SlotId(5), test_pair()).unwrap();
        assert_eq!(eng.last_processed_slot(), Some(SlotId(5)));
    }

    #[test]
    fn test_istatistikler_guncelleniyor() {
        let eng = engine();
        assert_eq!(eng.stats().total_slots_processed, 0);

        eng.process_slot(SlotId(1), orders_basic(), SlotId(1), test_pair()).unwrap();
        let stats = eng.stats();
        assert_eq!(stats.total_slots_processed, 1);
        assert!(stats.total_orders_processed >= 2);
    }

    #[test]
    fn test_bos_emirler_hatasi() {
        let eng = engine();
        let err = eng.process_slot(SlotId(1), vec![], SlotId(1), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    #[test]
    fn test_suresi_dolmus_emirler_filtreleniyor() {
        let eng = engine();
        // current_slot = 100, valid_until_slot = 50 → süresi dolmuş
        let mut expired = make_order(OrderSide::Buy, 1_000_000, 200, 1);
        expired.valid_until_slot = SlotId(50);

        let orders = vec![expired];
        let err = eng.process_slot(SlotId(100), orders, SlotId(100), test_pair()).unwrap_err();
        // Tüm emirler geçersiz → EmptyBatch
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    #[test]
    fn test_coklu_slot_sirali_islem() {
        let eng = engine();

        for slot_num in 1..=5u64 {
            let mut buy_orders: Vec<Order> = Vec::new();
            let mut sell_orders: Vec<Order> = Vec::new();

            for i in 0..3u8 {
                let mut b = make_order(OrderSide::Buy, 1_000_000, 200 + i as u128, i);
                let mut id = [0u8; 32];
                id[0] = i;
                id[1] = slot_num as u8;
                b.id = OrderId::from_bytes(id);
                buy_orders.push(b);

                let mut s = make_order(OrderSide::Sell, 1_000_000, 195 + i as u128, i + 100);
                let mut id2 = [0u8; 32];
                id2[0] = i + 100;
                id2[1] = slot_num as u8;
                s.id = OrderId::from_bytes(id2);
                sell_orders.push(s);
            }

            buy_orders.extend(sell_orders);
            let result = eng.process_slot(
                SlotId(slot_num),
                buy_orders,
                SlotId(slot_num),
                test_pair(),
            ).unwrap();

            assert!(result.has_fills(), "Slot {slot_num} clearing başarılı olmalı");
        }

        let stats = eng.stats();
        assert_eq!(stats.total_slots_processed, 5);
        assert!(stats.total_orders_filled > 0);
    }

    #[test]
    fn test_orderbook_analizi() {
        let eng = engine();
        let orders = vec![
            make_order(OrderSide::Buy, 1_000_000, 210, 1),
            make_order(OrderSide::Buy, 1_000_000, 205, 2),
            make_order(OrderSide::Sell, 1_000_000, 198, 3),
        ];

        let summary = eng.analyze_orderbook(SlotId(1), orders, SlotId(1), test_pair()).unwrap();
        assert_eq!(summary.buy_order_count, 2);
        assert_eq!(summary.sell_order_count, 1);
        assert_eq!(summary.best_bid, Some(210 * PRICE_PRECISION));
        assert_eq!(summary.best_ask, Some(198 * PRICE_PRECISION));
    }

    #[test]
    fn test_ayni_anda_bircok_emirle_tek_tarafli_hata() {
        let eng = engine();
        // Sadece alım emirleri → UnilateralBatch
        let orders = vec![
            make_order(OrderSide::Buy, 1_000_000, 200, 1),
            make_order(OrderSide::Buy, 1_000_000, 205, 2),
            make_order(OrderSide::Buy, 1_000_000, 210, 3),
        ];
        let err = eng.process_slot(SlotId(1), orders, SlotId(1), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::UnilateralBatch { .. }));
    }

    #[test]
    fn test_fiyat_kesisimi_yok_hatasi() {
        let eng = engine();
        // bid < ask → kesişim yok
        let orders = vec![
            make_order(OrderSide::Buy, 1_000_000, 190, 1),
            make_order(OrderSide::Sell, 1_000_000, 200, 2),
        ];
        let err = eng.process_slot(SlotId(1), orders, SlotId(1), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::NoPriceIntersection { .. }));
    }
}
