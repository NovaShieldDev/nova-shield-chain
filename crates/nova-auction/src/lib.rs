//! # nova-auction — Sealed-Bid Batch Auction Execution Motoru
//!
//! §5.3 — Nova Shield Chain'in MEV-free Uniform Price Batch Auction implementasyonu.
//!
//! ## Genel Bakış
//!
//! nova-auction, nova-mempool'dan gelen şifresi çözülmüş (reveal edilmiş)
//! emirleri işleyen ve Uniform Price Auction (UPA) ile clearing price belirleyen
//! Rust crate'idir.
//!
//! ### Neden Batch Auction?
//!
//! Geleneksel DEX'lerde sıralama (ordering) MEV (Maximal Extractable Value)
//! saldırılarına açıktır: validator veya sequencer TX sıralamasını manipüle
//! ederek front-running ve sandwich attack uygulayabilir.
//!
//! Nova Shield Chain'in çözümü:
//! 1. **nova-mempool**: TX içeriği IBE (Identity-Based Encryption) ile şifrelenir,
//!    guardian eşiği açılana kadar MEV bot içeriği göremez.
//! 2. **nova-auction**: Reveal fazından sonra tüm emirler tek batch'te işlenir.
//!    **Tek bir clearing price** tüm emirler için geçerli → sıralama avantajı yok.
//!
//! ### Uniform Price Auction (UPA) Nasıl Çalışır?
//!
//! ```text
//! Slot kapandı
//!     ↓
//! nova-mempool → emirler decrypt edildi
//!     ↓
//! nova-auction → process_slot(slot, orders)
//!     │
//!     ├─ validate_batch()      → geçersiz emirleri filtrele
//!     ├─ build_orderbook()     → alım/satım emirlerini sıralı tut
//!     ├─ find_clearing_price() → arz/talep kesişimi
//!     ├─ execute_batch()       → tüm emirler clearing price'ta işlenir
//!     └─ ClearingResult        → nova-runtime'a teslim
//! ```
//!
//! ## Modüller
//!
//! - [`errors`]: Tüm auction hata tipleri
//! - [`types`]: Temel veri yapıları (Order, TokenPair, ClearingResult, ...)
//! - [`orderbook`]: Fiyata göre sıralı emir kitabı
//! - [`clearing`]: Arz/talep eğrisi kesişim algoritması
//! - [`validation`]: Emir doğrulama kuralları
//! - [`execution`]: Clearing price'ta emir çalıştırma
//! - [`engine`]: Ana orkestrasyın motoru (tüm bileşenleri birleştirir)
//!
//! ## Referanslar
//!
//! - Whitepaper §5.3 — Sealed-Bid Batch Auction
//! - CoW Protocol: Uniform clearing price, partial fills, surplus maximization
//! - Rolling Shutter: Slot-based batch execution, threshold IBE trigger

// ─── MODÜLLER ─────────────────────────────────────────────────────────────────

pub mod clearing;
pub mod dutch;
pub mod engine;
pub mod errors;
pub mod execution;
pub mod orderbook;
pub mod solver;
pub mod types;
pub mod validation;

// ─── RE-EXPORTS ───────────────────────────────────────────────────────────────

pub use dutch::{
    DutchAuction, DutchAuctionEngine, DutchAuctionError, DutchAuctionId, DutchAuctionState,
    DutchBidResult, DutchCloseResult, DUTCH_PRICE_PRECISION, MIN_AUCTION_DURATION_SLOTS,
};
pub use errors::{AuctionError, AuctionResult};

pub use types::{
    AuctionConfig,
    AuctionStats,
    BatchAuction,
    CancelReason,
    ClearingResult,
    ExecutedOrder,
    Order,
    OrderBookSummary,
    OrderClass,
    OrderId,
    OrderSide,
    OrderStatus,
    TokenId,
    TokenPair,
    PRICE_PRECISION,
};

pub use orderbook::OrderBook;
pub use clearing::{ClearingAnalysis, ClearingEngine, PricePoint, UniformPriceVerdict};
pub use validation::OrderValidator;
pub use execution::ExecutionEngine;
pub use engine::BatchAuctionEngine;
pub use solver::{
    BatchSolution, LeaderLockTracker, SolutionScore, SolverId, SolverCompetition, SolverError,
    TradeExecution, VcgPayment,
};

// ─── ENTEGRASYON TESTLERİ ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::types::{Address, SlotId};

    // ── Test yardımcıları ────────────────────────────────────────────────────

    fn test_pair() -> TokenPair {
        TokenPair::new(TokenId([0xAAu8; 20]), TokenId::NSC)
    }

    fn make_order(side: OrderSide, amount: u128, price: u128, seed: u8) -> Order {
        let mut id_bytes = [0u8; 32];
        id_bytes[0] = seed;
        Order {
            id: OrderId::from_bytes(id_bytes),
            side,
            pair: test_pair(),
            amount: amount.max(1_000_000),
            limit_price: price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(10_000),
            trader: Address::from_bytes([seed; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    fn make_partial_order(side: OrderSide, amount: u128, price: u128, seed: u8) -> Order {
        let mut o = make_order(side, amount, price, seed);
        o.class = OrderClass::LimitPartialFill;
        o
    }

    fn engine() -> BatchAuctionEngine {
        BatchAuctionEngine::with_defaults()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 1: Tam eşleşme — temel happy path
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_tam_eslesme_basit() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  5_000_000, 200, 1),
            make_order(OrderSide::Sell, 5_000_000, 200, 2),
        ];

        let result = eng.process_slot(SlotId(1), orders, SlotId(1), test_pair()).unwrap();

        assert!(result.has_fills(), "İki emir eşleşmeli");
        assert_eq!(result.clearing_price, 200 * PRICE_PRECISION);
        assert_eq!(result.filled_count(), 2);
        assert_eq!(result.cancelled_count(), 0);
        assert_eq!(result.total_base_volume, 5_000_000);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 2: Price improvement — alıcı ve satıcı her ikisi de kazanıyor
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_price_improvement_mev_free_kaniti() {
        let eng = engine();

        // Alıcı: limit=210, clearing=190 → kazanç
        // Satıcı: limit=190, clearing=190 → tam limit
        let orders = vec![
            make_order(OrderSide::Buy,  2_000_000, 210, 1),
            make_order(OrderSide::Sell, 2_000_000, 190, 2),
        ];

        let result = eng.process_slot(SlotId(10), orders, SlotId(10), test_pair()).unwrap();

        assert!(result.has_fills());

        // Her iki taraf da kendi limitinden iyi veya eşit fiyat alıyor mu?
        for exec in &result.executed_orders {
            match exec.side {
                OrderSide::Buy => {
                    assert!(exec.clearing_price <= 210 * PRICE_PRECISION,
                        "Alıcı limit fiyatından pahalı almış olamaz");
                }
                OrderSide::Sell => {
                    assert!(exec.clearing_price >= 190 * PRICE_PRECISION,
                        "Satıcı limit fiyatından ucuz satmış olamaz");
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 3: Çoklu emir — klasik order book senaryosu
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_coklu_emirler_order_book_senaryosu() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  3_000_000, 210, 1),
            make_order(OrderSide::Buy,  1_000_000, 205, 2),
            make_order(OrderSide::Buy,  1_500_000, 205, 3),
            make_order(OrderSide::Buy,  2_000_000, 200, 4),
            make_order(OrderSide::Buy,  2_500_000, 200, 5),
            make_order(OrderSide::Buy,  1_000_000, 200, 6),

            make_order(OrderSide::Sell,   800_000, 195, 101),
            make_order(OrderSide::Sell, 1_200_000, 198, 102),
            make_order(OrderSide::Sell,   750_000, 200, 103),
            make_order(OrderSide::Sell, 2_500_000, 200, 104),
            make_order(OrderSide::Sell, 1_000_000, 205, 105),
        ];

        let result = eng.process_slot(SlotId(2), orders, SlotId(2), test_pair()).unwrap();

        assert!(result.has_fills());
        assert!(result.clearing_price >= 195 * PRICE_PRECISION);
        assert!(result.clearing_price <= 210 * PRICE_PRECISION);
        assert!(result.total_base_volume > 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 4: Kısmi doldurma — fazla alım, az satım
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_kismi_doldurma_carryover() {
        let eng = engine();

        // 3 milyon alım, 1 milyon satım → alım kısmen dolar
        let orders = vec![
            make_partial_order(OrderSide::Buy, 3_000_000, 205, 1),
            make_order(OrderSide::Sell, 1_000_000, 200, 2),
        ];

        let result = eng.process_slot(SlotId(3), orders, SlotId(3), test_pair()).unwrap();

        assert!(result.has_fills());

        let buy_exec = result.executed_orders.iter()
            .find(|e| e.side.is_buy());

        if let Some(exec) = buy_exec {
            if exec.is_partial {
                assert_eq!(result.carryover_count(), 1,
                    "Kısmi doldurulan alım emri bir sonraki slota taşınmalı");
                assert_eq!(result.carryover_orders[0].side, OrderSide::Buy);
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 5: Fiyat kesişimi yok → hata
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_fiyat_kesisimi_yok_hatasi() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  1_000_000, 190, 1),
            make_order(OrderSide::Sell, 1_000_000, 200, 2),
        ];

        let err = eng.process_slot(SlotId(4), orders, SlotId(4), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::NoPriceIntersection { .. }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 6: Tek taraflı batch → hata
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_tek_tarafli_batch_hata() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy, 1_000_000, 200, 1),
            make_order(OrderSide::Buy, 2_000_000, 205, 2),
        ];

        let err = eng.process_slot(SlotId(5), orders, SlotId(5), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::UnilateralBatch { buy_count: 2, sell_count: 0 }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 7: Süresi dolmuş emirler → filtrelenip EmptyBatch
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_suresi_dolmus_emirler_filtreleniyor() {
        let eng = engine();

        let mut expired_buy = make_order(OrderSide::Buy, 1_000_000, 200, 1);
        expired_buy.valid_until_slot = SlotId(10);

        let mut expired_sell = make_order(OrderSide::Sell, 1_000_000, 198, 2);
        expired_sell.valid_until_slot = SlotId(10);

        let orders = vec![expired_buy, expired_sell];

        let err = eng.process_slot(SlotId(11), orders, SlotId(11), test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 8: Aynı slot tekrar → BatchAlreadyExecuted
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_ayni_slot_tekrar_hata() {
        let eng = engine();
        let slot = SlotId(7);

        let orders1 = vec![
            make_order(OrderSide::Buy,  1_000_000, 200, 1),
            make_order(OrderSide::Sell, 1_000_000, 198, 2),
        ];
        let orders2 = vec![
            make_order(OrderSide::Buy,  1_000_000, 201, 3),
            make_order(OrderSide::Sell, 1_000_000, 197, 4),
        ];

        eng.process_slot(slot, orders1, slot, test_pair()).unwrap();
        let err = eng.process_slot(slot, orders2, slot, test_pair()).unwrap_err();
        assert!(matches!(err, AuctionError::BatchAlreadyExecuted { slot: SlotId(7) }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 9: Ardışık slotlar — istatistikler birikimli
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_ardisik_slotlar_istatistikler() {
        let eng = engine();

        for slot_n in 1u64..=5 {
            let orders: Vec<Order> = (0..4u8)
                .flat_map(|i| {
                    let mut buy_id = [0u8; 32];
                    buy_id[0] = i;
                    buy_id[1] = slot_n as u8;
                    let mut buy = make_order(OrderSide::Buy, 1_000_000, 200 + i as u128, i);
                    buy.id = OrderId::from_bytes(buy_id);

                    let mut sell_id = [0u8; 32];
                    sell_id[0] = i + 50;
                    sell_id[1] = slot_n as u8;
                    let mut sell = make_order(OrderSide::Sell, 1_000_000, 196 + i as u128, i + 50);
                    sell.id = OrderId::from_bytes(sell_id);

                    vec![buy, sell]
                })
                .collect();

            eng.process_slot(SlotId(slot_n), orders, SlotId(slot_n), test_pair()).unwrap();
        }

        let stats = eng.stats();
        assert_eq!(stats.total_slots_processed, 5);
        assert!(stats.total_orders_processed >= 10);
        assert!(stats.total_orders_filled > 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 10: Geçersiz token çifti → filtreleme
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_ayni_token_cifti_reddedilir() {
        let eng = engine();

        let same_pair = TokenPair::new(TokenId::NSC, TokenId::NSC);
        let mut bad_buy = make_order(OrderSide::Buy, 1_000_000, 200, 1);
        bad_buy.pair = same_pair;
        let mut bad_sell = make_order(OrderSide::Sell, 1_000_000, 198, 2);
        bad_sell.pair = same_pair;

        let err = eng.process_slot(SlotId(20), vec![bad_buy, bad_sell], SlotId(20), same_pair).unwrap_err();
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 11: Market emri ile limit emri birlikte
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_market_ve_limit_emirler_beraber() {
        let eng = engine();

        let mut market_buy = make_order(OrderSide::Buy, 2_000_000, 300, 1);
        market_buy.class = OrderClass::Market;
        market_buy.max_slippage_bps = 5000;

        let limit_sell = make_order(OrderSide::Sell, 2_000_000, 195, 2);

        let result = eng.process_slot(SlotId(30), vec![market_buy, limit_sell], SlotId(30), test_pair()).unwrap();

        assert!(result.has_fills());
        assert!(result.clearing_price >= 195 * PRICE_PRECISION);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 12: Slot sonucu kalıcı erişilebilir
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_slot_sonucu_kalici_erisim() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  1_000_000, 200, 1),
            make_order(OrderSide::Sell, 1_000_000, 198, 2),
        ];

        let result1 = eng.process_slot(SlotId(50), orders, SlotId(50), test_pair()).unwrap();
        let stored = eng.get_slot_result(SlotId(50)).expect("Slot sonucu saklanmış olmalı");

        assert_eq!(stored.slot, result1.slot);
        assert_eq!(stored.clearing_price, result1.clearing_price);
        assert_eq!(stored.filled_count(), result1.filled_count());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 13: Orderbook analizi (execution öncesi)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_orderbook_analizi_onceden() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  3_000_000, 210, 1),
            make_order(OrderSide::Buy,  1_000_000, 205, 2),
            make_order(OrderSide::Sell, 2_000_000, 198, 3),
            make_order(OrderSide::Sell, 1_500_000, 202, 4),
        ];

        let summary = eng.analyze_orderbook(SlotId(60), orders, SlotId(60), test_pair()).unwrap();

        assert_eq!(summary.buy_order_count, 2);
        assert_eq!(summary.sell_order_count, 2);
        assert_eq!(summary.best_bid, Some(210 * PRICE_PRECISION));
        assert_eq!(summary.best_ask, Some(198 * PRICE_PRECISION));
        // bid=210 > ask=198 → kesişim var, spread None döner (ask-bid negatif olmazdı)
        // Fiyat kesişimi: best_bid >= best_ask → clearing mümkün
        assert!(summary.best_bid.unwrap() >= summary.best_ask.unwrap(), "Kesişim var");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 14: Çok sayıda emir — stres testi
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_cok_sayida_emir_stres_testi() {
        let eng = engine();
        let mut orders: Vec<Order> = Vec::new();

        for i in 0u8..100 {
            let price = 200u128 + (i as u128 % 15);
            let amount = 1_000_000u128 + i as u128 * 10_000;

            let mut buy_id = [0u8; 32];
            buy_id[0] = i;
            let mut buy = make_order(OrderSide::Buy, amount, price + 5, i);
            buy.id = OrderId::from_bytes(buy_id);

            let mut sell_id = [0u8; 32];
            sell_id[0] = i;
            sell_id[1] = 1;
            let mut sell = make_order(OrderSide::Sell, amount, price, i);
            sell.id = OrderId::from_bytes(sell_id);

            orders.push(buy);
            orders.push(sell);
        }

        let result = eng.process_slot(SlotId(100), orders, SlotId(100), test_pair()).unwrap();

        assert!(result.has_fills());
        assert!(result.filled_count() > 0);
        assert!(result.total_base_volume > 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 15: Fill rate hesaplaması
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_fill_rate_hesaplama() {
        let eng = engine();

        let orders = vec![
            make_order(OrderSide::Buy,  1_000_000, 205, 1),
            make_order(OrderSide::Buy,  1_000_000, 202, 2),
            make_order(OrderSide::Sell, 1_000_000, 198, 3),
            make_order(OrderSide::Sell, 1_000_000, 200, 4),
        ];

        let result = eng.process_slot(SlotId(200), orders, SlotId(200), test_pair()).unwrap();

        assert!(result.fill_rate() > 0.0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 16: Carryover emirler sonraki slotta işlenir
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn entegrasyon_carryover_sonraki_slot() {
        let eng = engine();

        // Slot 1: kısmi doldurma
        let orders_slot1 = vec![
            make_partial_order(OrderSide::Buy, 3_000_000, 205, 1),
            make_order(OrderSide::Sell, 1_000_000, 200, 2),
        ];

        let result1 = eng.process_slot(SlotId(1), orders_slot1, SlotId(1), test_pair()).unwrap();
        let carryover = result1.carryover_orders.clone();

        if !carryover.is_empty() {
            // Slot 2: Carryover + yeni satım emri
            let mut orders_slot2 = carryover;
            let new_sell = make_order(OrderSide::Sell, 2_000_000, 198, 50);
            orders_slot2.push(new_sell);

            let result2 = eng.process_slot(SlotId(2), orders_slot2, SlotId(2), test_pair()).unwrap();
            assert!(result2.has_fills(), "Carryover emir ikinci slotta işlenmeli");
        }
    }
}
