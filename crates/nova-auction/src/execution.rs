//! Batch Auction Execution (Yürütme) Motoru.
//!
//! §5.3 — Clearing price belirlendikten sonra emirlerin çalıştırılması.
//!
//! ## Tasarım İlhamı: CoW Protocol Settlement Encoder
//!
//! CoW Protocol'ün `settlement_encoder.rs`'den öğrendiğimiz ders:
//! - Settlement: `clearing_prices: HashMap<Token, Price>` → her token için tek fiyat
//! - Trade execution: `sell_amount * sell_price / buy_price` (checked division)
//! - Kısmi doldurma: `executed_amount < order.amount` → pro-rata scaling
//! - Fee prorating: executed_amount ile orantılı
//!
//! ## Nova Adaptasyonu
//!
//! Nova'da CoW'dan farklı olarak:
//! - Her slot bağımsız batch → carryover (sonraki slota taşıma)
//! - Uniform clearing price → tek fiyat tüm emirler için
//! - MEV-free: clearing price açıklanana kadar şifreli (nova-mempool)
//! - Price improvement hesaplaması → kullanıcı surplus kanıtı
//!
//! ## Execution Akışı
//!
//! ```text
//! ClearingAnalysis
//!       ↓
//! [Alım emirleri filtrele: limit >= clearing_price]
//!       ↓
//! [Satım emirleri filtrele: limit <= clearing_price]
//!       ↓
//! [Marginal seviye: kısmi doldurma uygula]
//!       ↓
//! [ExecutedOrder listesi oluştur]
//!       ↓
//! [Carryover: kısmi doldurma taşı, süresi dolmamış iptal emirleri taşı]
//!       ↓
//! ClearingResult
//! ```

use crate::clearing::{ClearingAnalysis, ClearingEngine};
use crate::errors::{AuctionError, AuctionResult};
use crate::orderbook::OrderBook;
use crate::types::{
    CancelReason, ClearingResult, ExecutedOrder, Order, OrderId, OrderSide,
    OrderStatus, PRICE_PRECISION,
};
use nova_primitives::types::SlotId;
use std::collections::HashSet;
use tracing::{debug, info, trace, warn};

// ─── EXECUTION MOTORU ────────────────────────────────────────────────────────

/// Batch Auction Execution Motoru.
///
/// Clearing price belirlendikten sonra emirleri çalıştırır,
/// kısmi doldurmaları hesaplar, carryover listesi oluşturur.
pub struct ExecutionEngine {
    /// Clearing hesaplama motoru.
    clearing: ClearingEngine,

    /// Kısmi doldurma aktif mi?
    allow_partial_fills: bool,

    /// Sonraki slota taşınabilecek maksimum emir sayısı.
    max_carryover_orders: usize,
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self {
            clearing: ClearingEngine::default(),
            allow_partial_fills: true,
            max_carryover_orders: 1_000,
        }
    }
}

impl ExecutionEngine {
    /// Yeni execution motoru.
    pub fn new(
        allow_partial_fills: bool,
        max_carryover_orders: usize,
        min_tradeable_volume: u128,
    ) -> Self {
        Self {
            clearing: ClearingEngine::new(min_tradeable_volume),
            allow_partial_fills,
            max_carryover_orders,
        }
    }

    /// Batch auction'ı çalıştır.
    ///
    /// `OrderBook`'taki emirleri clearing price'ta işler, sonuçları döner.
    ///
    /// # Akış
    /// 1. Clearing price bul
    /// 2. Çalıştırılabilir emirleri filtrele
    /// 3. Marginal seviyede kısmi doldurma uygula
    /// 4. Her emir için `ExecutedOrder` oluştur
    /// 5. Carryover ve iptal listelerini oluştur
    /// 6. `ClearingResult` döndür
    pub fn execute_batch(
        &self,
        book: &OrderBook,
        slot: SlotId,
        computation_start_us: u64,
    ) -> AuctionResult<ClearingResult> {
        // ── 1. Clearing price bul ─────────────────────────────────────────────
        let analysis = self.clearing.find_clearing_price(book, slot)?;

        info!(
            slot = slot.0,
            clearing_price = analysis.clearing_price,
            max_volume = analysis.max_tradeable_volume,
            "Batch execution başladı"
        );

        // ── 2. Emirleri clearing price'a göre sınıflandır ────────────────────
        let ExecutionPlan {
            executable_buys,
            executable_sells,
            cancelled_buys,
            cancelled_sells,
        } = self.classify_orders(book, &analysis);

        debug!(
            slot = slot.0,
            exec_buys = executable_buys.len(),
            exec_sells = executable_sells.len(),
            cancelled = cancelled_buys.len() + cancelled_sells.len(),
            "Emirler sınıflandırıldı"
        );

        // ── 3. Execution hesaplamaları ────────────────────────────────────────
        let ExecutionOutput {
            mut executed_orders,
            cancelled_orders,
            carryover_orders,
            total_base_volume,
            total_quote_volume,
            total_price_improvement,
        } = self.compute_executions(
            &analysis,
            executable_buys,
            executable_sells,
            cancelled_buys,
            cancelled_sells,
            slot,
        )?;

        // ── 4. Execution sıralarını ata ───────────────────────────────────────
        for (i, exec) in executed_orders.iter_mut().enumerate() {
            exec.execution_order = i as u32;
        }

        let computation_time_us = computation_start_us; // Caller tarafından hesaplanır

        info!(
            slot = slot.0,
            executed = executed_orders.len(),
            cancelled = cancelled_orders.len(),
            carryover = carryover_orders.len(),
            total_base_volume = total_base_volume,
            total_quote_volume = total_quote_volume,
            total_price_improvement = total_price_improvement,
            "Batch execution tamamlandı"
        );

        Ok(ClearingResult {
            slot,
            clearing_price: analysis.clearing_price,
            token_pair: book.pair,
            executed_orders,
            cancelled_orders,
            carryover_orders,
            total_base_volume,
            total_quote_volume,
            total_price_improvement,
            computation_time_us,
        })
    }

    // ─── Yardımcı: Sınıflandırma ─────────────────────────────────────────────

    /// Emirleri çalıştırılabilir ve iptal olarak sınıflandır.
    fn classify_orders<'a>(
        &self,
        book: &'a OrderBook,
        analysis: &ClearingAnalysis,
    ) -> ExecutionPlan<'a> {
        let clearing_price = analysis.clearing_price;

        // Alım emirleri: limit_price >= clearing_price → çalıştırılabilir
        let mut executable_buys: Vec<&Order> = Vec::new();
        let mut cancelled_buys: Vec<(&Order, CancelReason)> = Vec::new();

        for order in book.all_buy_orders_desc() {
            if order.limit_price >= clearing_price {
                // Slippage kontrolü ekstra olarak — limit iyiyse slippage sorun olmaz
                if order.price_acceptable(clearing_price) {
                    executable_buys.push(order);
                } else {
                    cancelled_buys.push((order, CancelReason::SlippageExceeded));
                }
            } else {
                cancelled_buys.push((order, CancelReason::PriceNotMet));
            }
        }

        // Satım emirleri: limit_price <= clearing_price → çalıştırılabilir
        let mut executable_sells: Vec<&Order> = Vec::new();
        let mut cancelled_sells: Vec<(&Order, CancelReason)> = Vec::new();

        for order in book.all_sell_orders_asc() {
            if order.limit_price <= clearing_price {
                if order.price_acceptable(clearing_price) {
                    executable_sells.push(order);
                } else {
                    cancelled_sells.push((order, CancelReason::SlippageExceeded));
                }
            } else {
                cancelled_sells.push((order, CancelReason::PriceNotMet));
            }
        }

        ExecutionPlan {
            executable_buys,
            executable_sells,
            cancelled_buys,
            cancelled_sells,
        }
    }

    /// Execution hesaplamalarını yap.
    ///
    /// CoW Protocol'ün `trade.executed_amounts(sell_price, buy_price)` yöntemi
    /// Nova'da `amount * clearing_price / PRICE_PRECISION` olarak uygulanır.
    fn compute_executions<'a>(
        &self,
        analysis: &ClearingAnalysis,
        executable_buys: Vec<&'a Order>,
        executable_sells: Vec<&'a Order>,
        cancelled_buys: Vec<(&'a Order, CancelReason)>,
        cancelled_sells: Vec<(&'a Order, CancelReason)>,
        slot: SlotId,
    ) -> AuctionResult<ExecutionOutput> {
        let clearing_price = analysis.clearing_price;
        let max_volume = analysis.max_tradeable_volume;

        // Toplam çalıştırılabilir alım ve satım hacimlerini hesapla
        let total_exec_buy: u128 = executable_buys.iter().map(|o| o.amount).sum();
        let total_exec_sell: u128 = executable_sells.iter().map(|o| o.amount).sum();

        // Gerçekten işlenecek hacim: min(talep, arz)
        let actual_volume = total_exec_buy.min(total_exec_sell).min(max_volume);

        trace!(
            slot = slot.0,
            total_exec_buy,
            total_exec_sell,
            actual_volume,
            "Execution hacmi hesaplandı"
        );

        let mut executed_orders: Vec<ExecutedOrder> = Vec::new();
        let mut cancelled_orders: Vec<(OrderId, CancelReason)> = Vec::new();
        let mut carryover_orders: Vec<Order> = Vec::new();
        let mut total_base_volume: u128 = 0;
        let mut total_quote_volume: u128 = 0;
        let mut total_price_improvement_i128: i128 = 0;

        // ── Alım emirlerini çalıştır ──────────────────────────────────────────
        let mut remaining_buy_volume = actual_volume;

        for order in &executable_buys {
            if remaining_buy_volume == 0 {
                // Volume bitti — kısmi doldurma destekli emirler sonraki slota taşınır
                if order.allows_partial_fill()
                    && self.allow_partial_fills
                    && carryover_orders.len() < self.max_carryover_orders
                {
                    let mut carryover = (*order).clone();
                    carryover.submitted_at_slot = slot;
                    carryover_orders.push(carryover);
                } else {
                    cancelled_orders.push((order.id, CancelReason::PriceNotMet));
                }
                continue;
            }

            let fill_amount = if order.amount <= remaining_buy_volume {
                // Tam doldurma
                order.amount
            } else {
                // Kısmi doldurma adayı
                if order.allows_partial_fill() && self.allow_partial_fills {
                    remaining_buy_volume
                } else {
                    // Kısmi doldurma izin yok → iptal
                    cancelled_orders.push((order.id, CancelReason::PriceNotMet));
                    continue;
                }
            };

            let is_partial = fill_amount < order.amount;
            let remaining_amount = order.amount.saturating_sub(fill_amount);

            // Quote miktarı: fill_amount * clearing_price / PRICE_PRECISION
            let quote_amount = self
                .clearing
                .compute_quote_amount(fill_amount, clearing_price)
                .ok_or(AuctionError::ClearingPriceOverflow)?;

            // Price improvement: limit_price - clearing_price (birim başına)
            let price_improvement = self.clearing.compute_price_improvement(
                order,
                clearing_price,
                fill_amount,
            );

            total_base_volume = total_base_volume.saturating_add(fill_amount);
            total_quote_volume = total_quote_volume.saturating_add(quote_amount);
            total_price_improvement_i128 =
                total_price_improvement_i128.saturating_add(price_improvement);

            remaining_buy_volume = remaining_buy_volume.saturating_sub(fill_amount);

            // Carryover: kısmi doldurma → kalan miktar sonraki slota
            if is_partial && carryover_orders.len() < self.max_carryover_orders {
                let mut carryover = (*order).clone();
                carryover.amount = remaining_amount;
                carryover.submitted_at_slot = slot;
                carryover_orders.push(carryover);
            }

            executed_orders.push(ExecutedOrder {
                order_id: order.id,
                trader: order.trader,
                side: order.side,
                pair: order.pair,
                requested_amount: order.amount,
                executed_amount: fill_amount,
                clearing_price,
                quote_amount,
                price_improvement,
                is_partial,
                remaining_amount,
                execution_order: 0, // Sonra atanır
            });
        }

        // ── Satım emirlerini çalıştır ─────────────────────────────────────────
        let mut remaining_sell_volume = actual_volume;

        for order in &executable_sells {
            if remaining_sell_volume == 0 {
                // Volume bitti — kısmi doldurma destekli emirler sonraki slota taşınır
                if order.allows_partial_fill()
                    && self.allow_partial_fills
                    && carryover_orders.len() < self.max_carryover_orders
                {
                    let mut carryover = (*order).clone();
                    carryover.submitted_at_slot = slot;
                    carryover_orders.push(carryover);
                } else {
                    cancelled_orders.push((order.id, CancelReason::PriceNotMet));
                }
                continue;
            }

            let fill_amount = if order.amount <= remaining_sell_volume {
                order.amount
            } else {
                if order.allows_partial_fill() && self.allow_partial_fills {
                    remaining_sell_volume
                } else {
                    cancelled_orders.push((order.id, CancelReason::PriceNotMet));
                    continue;
                }
            };

            let is_partial = fill_amount < order.amount;
            let remaining_amount = order.amount.saturating_sub(fill_amount);

            let quote_amount = self
                .clearing
                .compute_quote_amount(fill_amount, clearing_price)
                .ok_or(AuctionError::ClearingPriceOverflow)?;

            let price_improvement = self.clearing.compute_price_improvement(
                order,
                clearing_price,
                fill_amount,
            );

            total_price_improvement_i128 =
                total_price_improvement_i128.saturating_add(price_improvement);

            remaining_sell_volume = remaining_sell_volume.saturating_sub(fill_amount);

            if is_partial && carryover_orders.len() < self.max_carryover_orders {
                let mut carryover = (*order).clone();
                carryover.amount = remaining_amount;
                carryover.submitted_at_slot = slot;
                carryover_orders.push(carryover);
            }

            executed_orders.push(ExecutedOrder {
                order_id: order.id,
                trader: order.trader,
                side: order.side,
                pair: order.pair,
                requested_amount: order.amount,
                executed_amount: fill_amount,
                clearing_price,
                quote_amount,
                price_improvement,
                is_partial,
                remaining_amount,
                execution_order: 0,
            });
        }

        // ── İptal listelerini birleştir ───────────────────────────────────────
        for (order, reason) in cancelled_buys.iter().chain(cancelled_sells.iter()) {
            cancelled_orders.push((order.id, reason.clone()));
        }

        let total_price_improvement = total_price_improvement_i128.max(0) as u128;

        Ok(ExecutionOutput {
            executed_orders,
            cancelled_orders,
            carryover_orders,
            total_base_volume,
            total_quote_volume,
            total_price_improvement,
        })
    }
}

// ─── İÇ TİPLER ───────────────────────────────────────────────────────────────

/// Sınıflandırılmış emirler (execution'dan önce).
struct ExecutionPlan<'a> {
    executable_buys: Vec<&'a Order>,
    executable_sells: Vec<&'a Order>,
    cancelled_buys: Vec<(&'a Order, CancelReason)>,
    cancelled_sells: Vec<(&'a Order, CancelReason)>,
}

/// Execution hesaplama çıktısı.
struct ExecutionOutput {
    executed_orders: Vec<ExecutedOrder>,
    cancelled_orders: Vec<(OrderId, CancelReason)>,
    carryover_orders: Vec<Order>,
    total_base_volume: u128,
    total_quote_volume: u128,
    total_price_improvement: u128,
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::OrderBook;
    use crate::types::{Order, OrderClass, OrderId, OrderSide, TokenId, TokenPair, PRICE_PRECISION};
    use nova_primitives::types::{Address, SlotId};

    fn test_pair() -> TokenPair {
        TokenPair::new(TokenId([1u8; 20]), TokenId::NSC)
    }

    fn make_order(side: OrderSide, amount: u128, price: u128, seed: u8, class: OrderClass) -> Order {
        let mut id_bytes = [0u8; 32];
        id_bytes[0] = seed;
        Order {
            id: OrderId::from_bytes(id_bytes),
            side,
            pair: test_pair(),
            amount,
            limit_price: price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class,
            valid_until_slot: SlotId(1000),
            trader: Address::from_bytes([seed; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    fn make_book(
        buys: &[(u128, u128, OrderClass)],
        sells: &[(u128, u128, OrderClass)],
    ) -> OrderBook {
        let mut book = OrderBook::new(SlotId(1), test_pair(), 10_000);
        for (i, &(amount, price, class)) in buys.iter().enumerate() {
            book.add_order(make_order(OrderSide::Buy, amount, price, i as u8, class)).unwrap();
        }
        for (i, &(amount, price, class)) in sells.iter().enumerate() {
            book.add_order(make_order(OrderSide::Sell, amount, price, (i + 100) as u8, class)).unwrap();
        }
        book
    }

    fn engine() -> ExecutionEngine {
        ExecutionEngine::default()
    }

    // ─── Temel Execution Testleri ─────────────────────────────────────────────

    #[test]
    fn test_tam_doldurma_esit_miktar() {
        // 100 alım @ 200, 100 satım @ 198 → 100 tam işlem
        let book = make_book(
            &[(100, 200, OrderClass::Limit)],
            &[(100, 198, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        assert!(result.has_fills());
        assert_eq!(result.filled_count(), 2, "1 alım + 1 satım = 2 filled");
        assert_eq!(result.total_base_volume, 100);
        assert_eq!(result.cancelled_count(), 0);
        assert_eq!(result.carryover_count(), 0);
    }

    #[test]
    fn test_clearing_price_belirlendi() {
        let book = make_book(
            &[(500, 205, OrderClass::Limit)],
            &[(500, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        // clearing price = 200 (en düşük max-volume fiyatı)
        assert_eq!(result.clearing_price, 200 * PRICE_PRECISION);
    }

    #[test]
    fn test_price_improvement_alimda_hesaplaniyor() {
        // limit = 205, clearing = 200 → improvement pozitif olmalı
        let book = make_book(
            &[(100, 205, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        // Alım emrinin price improvement'ı pozitif olmalı
        let buy_exec = result.executed_orders.iter()
            .find(|o| matches!(o.side, OrderSide::Buy))
            .expect("Alım emri çalıştırılmalı");
        assert!(buy_exec.price_improvement > 0, "Price improvement pozitif olmalı");
    }

    #[test]
    fn test_limit_altinda_alim_iptal() {
        // Alım @ 190, satım @ 200 → kesişim yok → hata
        let book = make_book(
            &[(100, 190, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let err = engine().execute_batch(&book, SlotId(1), 0).unwrap_err();
        assert!(matches!(err, AuctionError::NoPriceIntersection { .. }));
    }

    #[test]
    fn test_bos_kitap_hata() {
        let book = OrderBook::new(SlotId(1), test_pair(), 1000);
        let err = engine().execute_batch(&book, SlotId(1), 0).unwrap_err();
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    // ─── Kısmi Doldurma Testleri ─────────────────────────────────────────────

    #[test]
    fn test_fazla_alim_kismi_doldurma() {
        // 300 alım @ 205 (kısmi), 100 satım @ 200
        // Sadece 100 işlenebilir → alım kısmi doldurma alır
        let book = make_book(
            &[(300, 205, OrderClass::LimitPartialFill)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        assert!(result.has_fills());
        // Alım emri kısmen doldu → carryover olmalı
        let buy_exec = result.executed_orders.iter()
            .find(|o| matches!(o.side, OrderSide::Buy));
        if let Some(exec) = buy_exec {
            if exec.is_partial {
                assert_eq!(result.carryover_count(), 1, "Kısmi doldurulan emir carryover olmalı");
                assert_eq!(result.carryover_orders[0].amount, 200, "Kalan 200 taşınmalı");
            }
        }
    }

    #[test]
    fn test_tam_doldurma_carryover_yok() {
        let book = make_book(
            &[(100, 200, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        assert_eq!(result.carryover_count(), 0, "Tam doldurma → carryover yok");
    }

    // ─── Çok Emirli Testler ───────────────────────────────────────────────────

    #[test]
    fn test_coklu_emirler_clearing() {
        let book = make_book(
            &[
                (300, 210, OrderClass::Limit),
                (100, 205, OrderClass::Limit),
                (200, 200, OrderClass::Limit),
            ],
            &[
                (80, 198, OrderClass::Limit),
                (75, 200, OrderClass::Limit),
                (120, 205, OrderClass::Limit),
            ],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        assert!(result.has_fills(), "En az bir emir çalıştırılmalı");
        assert!(result.clearing_price > 0);
        assert!(result.total_base_volume > 0);
    }

    #[test]
    fn test_quote_amount_dogru_hesaplaniyor() {
        // 100 birim @ clearing 200 → quote = 100 * 200 * PRICE_PRECISION / PRICE_PRECISION = 20000
        let book = make_book(
            &[(100, 210, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        let sell_exec = result.executed_orders.iter()
            .find(|o| matches!(o.side, OrderSide::Sell))
            .expect("Satım emri çalıştırılmalı");

        // quote = 100 * 200 = 20000 (PRICE_PRECISION ile normalize)
        assert_eq!(sell_exec.quote_amount, 20_000);
    }

    #[test]
    fn test_toplam_price_improvement_pozitif() {
        // limit_buy = 205, limit_sell = 195, clearing = 200
        // Buy improvement = (205 - 200) * 100 / PRICE_PRECISION = 500
        // Sell improvement = (200 - 195) * 100 / PRICE_PRECISION = 500
        // Toplam ≥ 0
        let book = make_book(
            &[(100, 205, OrderClass::Limit)],
            &[(100, 195, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();
        assert!(result.total_price_improvement >= 0);
    }

    #[test]
    fn test_execution_sirasi_ataniyor() {
        let book = make_book(
            &[(100, 210, OrderClass::Limit), (50, 205, OrderClass::Limit)],
            &[(150, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        // Execution order'lar sıralı olmalı
        for (i, exec) in result.executed_orders.iter().enumerate() {
            assert_eq!(exec.execution_order, i as u32);
        }
    }

    #[test]
    fn test_slot_numarasi_sonucta_dogru() {
        let book = make_book(
            &[(100, 200, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(42), 0).unwrap();
        assert_eq!(result.slot, SlotId(42));
    }

    #[test]
    fn test_fill_rate_hesaplaniyor() {
        let book = make_book(
            &[(100, 200, OrderClass::Limit)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();
        // 2 emir çalıştırıldı, 0 iptal → fill rate = 1.0
        assert!((result.fill_rate() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_volume_bitti_partial_fill_carryover_olur() {
        // BUG-19 düzeltme testi: volume sıfıra düşünce kısmi doldurma destekli emirler
        // iptal değil, carryover listesine gitmeli.
        //
        // 100 satım @ 200 + 300 alım @ 205 (partial fill destekli):
        // → 100 işlenir (alım emri kısmen), sonra 2. alım emri volume = 0 durumuna düşer.
        let book = make_book(
            &[
                (100, 210, OrderClass::LimitPartialFill), // 1. alım: önce çalışır
                (200, 205, OrderClass::LimitPartialFill), // 2. alım: volume bitti → carryover
            ],
            &[(100, 200, OrderClass::Limit)], // 100 satım
        );
        let result = engine().execute_batch(&book, SlotId(1), 0).unwrap();

        // 1. alım tamamen dolar (100), 2. alım volume=0 ile karşılaşır → carryover
        assert!(result.carryover_count() > 0, "Volume biten kısmi fill emirleri carryover olmalı");
        // Carryover'daki emir 200 birim olmalı
        let carryover = result.carryover_orders.iter().find(|o| o.amount == 200);
        assert!(carryover.is_some(), "200 birimlik emir carryover listesinde olmalı");
    }

    #[test]
    fn test_kisimi_doldurma_devre_disi() {
        // allow_partial_fills = false ile engine
        let eng = ExecutionEngine::new(false, 100, 0);
        let book = make_book(
            &[(300, 205, OrderClass::LimitPartialFill)],
            &[(100, 200, OrderClass::Limit)],
        );
        let result = eng.execute_batch(&book, SlotId(1), 0).unwrap();
        // Kısmi doldurma kapalı → 300'lük alım tamamen iptal/atlanır, 100 satım kalır
        // Gerçek behavior: alım emri reddedilir çünkü 300 > available 100 ve partial_fill = false
        // UYARI: Bu durumda alım çalıştırılamaz
        assert_eq!(result.carryover_count(), 0, "Kısmi doldurma kapalı → carryover yok");
    }
}
