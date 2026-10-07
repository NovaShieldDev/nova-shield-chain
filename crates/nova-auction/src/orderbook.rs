//! Batch Auction Emir Kitabı (Order Book).
//!
//! §5.3 — Slot başına alım/satım emirlerini fiyata göre sıralı tutar.
//!
//! ## Tasarım
//!
//! Bu modül, CoW Protocol'ün order book tasarımından ilham alır:
//! - Alım emirleri (buy): fiyata göre **azalan** (en yüksek bid önce)
//! - Satım emirleri (sell): fiyata göre **artan** (en düşük ask önce)
//!
//! Fiyat eşitliği durumunda `OrderId` sıralaması kullanılır (deterministik).
//!
//! ## Fiyat Seviyeleri
//!
//! ```text
//! ALIM EMİRLERİ (bid, azalan)     SATIM EMİRLERİ (ask, artan)
//! ─────────────────────────────   ────────────────────────────
//! [$210] ──  300 token            [$198] ──  80 token
//! [$205] ──  100 token            [$200] ──  75 token
//! [$200] ──  200 token            [$205] ── 120 token
//!                                          ↑
//!                           clearing price bu iki eğrinin kesişimi
//! ```

use crate::errors::{AuctionError, AuctionResult};
use crate::types::{Order, OrderId, OrderSide, OrderBookSummary, TokenPair, PRICE_PRECISION};
use nova_primitives::types::SlotId;
use std::collections::HashMap;
use tracing::{debug, trace};

// ─── FİYAT SEVİYESİ ─────────────────────────────────────────────────────────

/// Tek bir fiyat seviyesindeki emirler.
///
/// Aynı limit fiyata sahip tüm emirleri tutar.
/// Zaman önceliğine (FIFO) göre işlenir.
#[derive(Debug, Clone)]
pub struct PriceLevel {
    /// Fiyat (PRICE_PRECISION ile normalleştirilmiş).
    pub price: u128,

    /// Bu fiyattaki emirler (ekleme sırasına göre).
    pub orders: Vec<Order>,

    /// Toplam hacim (bu fiyat seviyesindeki tüm emirlerin miktarı).
    pub total_volume: u128,
}

impl PriceLevel {
    /// Yeni fiyat seviyesi oluştur.
    pub fn new(price: u128) -> Self {
        Self {
            price,
            orders: Vec::new(),
            total_volume: 0,
        }
    }

    /// Emir ekle.
    pub fn add_order(&mut self, order: Order) {
        self.total_volume = self.total_volume.saturating_add(order.amount);
        self.orders.push(order);
    }

    /// Emir kaldır (ID ile).
    pub fn remove_order(&mut self, order_id: &OrderId) -> Option<Order> {
        if let Some(pos) = self.orders.iter().position(|o| &o.id == order_id) {
            let order = self.orders.remove(pos);
            self.total_volume = self.total_volume.saturating_sub(order.amount);
            Some(order)
        } else {
            None
        }
    }

    /// Bu fiyat seviyesi boş mu?
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Emir sayısı.
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }
}

// ─── EMİR KİTABI ─────────────────────────────────────────────────────────────

/// Batch Auction için tek slot'un emir kitabı.
///
/// Alım emirlerini bid-desc (yüksekten düşüğe) sıralı,
/// satım emirlerini ask-asc (düşükten yükseğe) sıralı tutar.
///
/// `parking_lot::RwLock` ile thread-safe erişim sağlanır.
///
/// §5.3.1 — Her slot'ta sıfırdan inşa edilir.
#[derive(Debug)]
pub struct OrderBook {
    /// Slot numarası.
    pub slot: SlotId,

    /// Token çifti.
    pub pair: TokenPair,

    /// Alım emirleri — fiyat seviyelerine göre.
    /// Key: limit_price (u128)
    /// Value: PriceLevel
    /// Sıralama: fiyata göre azalan (BTreeMap reverse iteration)
    buy_levels: std::collections::BTreeMap<u128, PriceLevel>,

    /// Satım emirleri — fiyat seviyelerine göre.
    /// Key: limit_price (u128)
    /// Value: PriceLevel
    /// Sıralama: fiyata göre artan (BTreeMap normal iteration)
    sell_levels: std::collections::BTreeMap<u128, PriceLevel>,

    /// Hızlı ID → fiyat arama tablosu.
    order_price_index: HashMap<OrderId, u128>,

    /// Toplam alım hacmi.
    pub total_buy_volume: u128,

    /// Toplam satım hacmi.
    pub total_sell_volume: u128,

    /// Maksimum kapasite.
    pub max_capacity: usize,

    /// Toplam emir sayısı.
    order_count: usize,

    /// Kitap kapalı mı? (Slot cleared sonrası)
    pub closed: bool,
}

impl OrderBook {
    /// Yeni boş emir kitabı oluştur.
    pub fn new(slot: SlotId, pair: TokenPair, max_capacity: usize) -> Self {
        Self {
            slot,
            pair,
            buy_levels: std::collections::BTreeMap::new(),
            sell_levels: std::collections::BTreeMap::new(),
            order_price_index: HashMap::new(),
            total_buy_volume: 0,
            total_sell_volume: 0,
            max_capacity,
            order_count: 0,
            closed: false,
        }
    }

    /// Emir ekle.
    ///
    /// # Hatalar
    /// - `OrderBookClosed`: Kitap kapalıysa
    /// - `OrderBookFull`: Kapasite doluysa
    /// - `DuplicateOrder`: Aynı ID ile emir zaten varsa
    pub fn add_order(&mut self, order: Order) -> AuctionResult<()> {
        if self.closed {
            return Err(AuctionError::OrderBookClosed { slot: self.slot });
        }

        if self.order_count >= self.max_capacity {
            return Err(AuctionError::OrderBookFull {
                current: self.order_count,
                maximum: self.max_capacity,
            });
        }

        if self.order_price_index.contains_key(&order.id) {
            return Err(AuctionError::DuplicateOrder {
                order_id: order.id.to_hex(),
            });
        }

        let price = order.limit_price;
        let amount = order.amount;
        let side = order.side;
        let id = order.id;

        match side {
            OrderSide::Buy => {
                let level = self.buy_levels.entry(price).or_insert_with(|| PriceLevel::new(price));
                level.add_order(order);
                self.total_buy_volume = self.total_buy_volume.saturating_add(amount);
            }
            OrderSide::Sell => {
                let level = self.sell_levels.entry(price).or_insert_with(|| PriceLevel::new(price));
                level.add_order(order);
                self.total_sell_volume = self.total_sell_volume.saturating_add(amount);
            }
        }

        self.order_price_index.insert(id, price);
        self.order_count += 1;

        trace!(
            slot = self.slot.0,
            order_id = %id,
            side = %side,
            price = price,
            amount = amount,
            "Emir kitabına eklendi"
        );

        Ok(())
    }

    /// Emir kaldır (ID ile).
    pub fn remove_order(&mut self, order_id: &OrderId) -> Option<Order> {
        let price = self.order_price_index.remove(order_id)?;

        // Alım emirlerinde ara
        if let Some(level) = self.buy_levels.get_mut(&price) {
            if let Some(order) = level.remove_order(order_id) {
                self.total_buy_volume = self.total_buy_volume.saturating_sub(order.amount);
                self.order_count -= 1;
                if level.is_empty() {
                    self.buy_levels.remove(&price);
                }
                return Some(order);
            }
        }

        // Satım emirlerinde ara
        if let Some(level) = self.sell_levels.get_mut(&price) {
            if let Some(order) = level.remove_order(order_id) {
                self.total_sell_volume = self.total_sell_volume.saturating_sub(order.amount);
                self.order_count -= 1;
                if level.is_empty() {
                    self.sell_levels.remove(&price);
                }
                return Some(order);
            }
        }

        None
    }

    /// En iyi alım fiyatı (en yüksek bid).
    pub fn best_bid(&self) -> Option<u128> {
        self.buy_levels.keys().next_back().copied()
    }

    /// En iyi satım fiyatı (en düşük ask).
    pub fn best_ask(&self) -> Option<u128> {
        self.sell_levels.keys().next().copied()
    }

    /// Mid-price (bid/ask ortalaması).
    pub fn mid_price(&self) -> Option<u128> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        Some((bid / 2) + (ask / 2))
    }

    /// Spread (ask - bid).
    pub fn spread(&self) -> Option<u128> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        if ask >= bid {
            Some(ask - bid)
        } else {
            None
        }
    }

    /// Fiyat kesişimi var mı? (Alım ve satım fiyatları örtüşüyor mu?)
    ///
    /// Clearing için gerekli koşul: `best_bid >= best_ask`
    pub fn has_price_intersection(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid >= ask,
            _ => false,
        }
    }

    /// Toplam emir sayısı.
    pub fn order_count(&self) -> usize {
        self.order_count
    }

    /// Alım emri sayısı.
    pub fn buy_count(&self) -> usize {
        self.buy_levels.values().map(|l| l.order_count()).sum()
    }

    /// Satım emri sayısı.
    pub fn sell_count(&self) -> usize {
        self.sell_levels.values().map(|l| l.order_count()).sum()
    }

    /// Emir kitabı boş mu?
    pub fn is_empty(&self) -> bool {
        self.order_count == 0
    }

    /// Emir kitabı kapalı mı?
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Emir kitabını kapat (slot cleared sonrası).
    pub fn close(&mut self) {
        self.closed = true;
        debug!(slot = self.slot.0, "Emir kitabı kapatıldı");
    }

    /// Tüm alım emirlerini fiyata göre azalan sırada döndür.
    ///
    /// Her iterator adımı bir `&PriceLevel` döndürür.
    pub fn buy_levels_desc(&self) -> impl Iterator<Item = &PriceLevel> {
        self.buy_levels.values().rev()
    }

    /// Tüm satım emirlerini fiyata göre artan sırada döndür.
    pub fn sell_levels_asc(&self) -> impl Iterator<Item = &PriceLevel> {
        self.sell_levels.values()
    }

    /// Tüm alım emirlerini fiyata göre azalan sırada (düz liste).
    pub fn all_buy_orders_desc(&self) -> Vec<&Order> {
        self.buy_levels
            .values()
            .rev()
            .flat_map(|level| level.orders.iter())
            .collect()
    }

    /// Tüm satım emirlerini fiyata göre artan sırada (düz liste).
    pub fn all_sell_orders_asc(&self) -> Vec<&Order> {
        self.sell_levels
            .values()
            .flat_map(|level| level.orders.iter())
            .collect()
    }

    /// P fiyatı veya üzerindeki alım emirlerinin toplam hacmi (kümülatif talep).
    ///
    /// Demand curve: `D(P) = Σ amount for buy orders where limit_price >= P`
    pub fn cumulative_demand_at(&self, price: u128) -> u128 {
        self.buy_levels
            .range(price..)
            .map(|(_, level)| level.total_volume)
            .fold(0u128, |acc, v| acc.saturating_add(v))
    }

    /// P fiyatı veya altındaki satım emirlerinin toplam hacmi (kümülatif arz).
    ///
    /// Supply curve: `S(P) = Σ amount for sell orders where limit_price <= P`
    pub fn cumulative_supply_at(&self, price: u128) -> u128 {
        self.sell_levels
            .range(..=price)
            .map(|(_, level)| level.total_volume)
            .fold(0u128, |acc, v| acc.saturating_add(v))
    }

    /// Tüm benzersiz fiyat noktaları (hem alım hem satım, sıralı).
    ///
    /// Clearing price araması için kullanılır.
    pub fn all_price_points(&self) -> Vec<u128> {
        let mut prices: Vec<u128> = self
            .buy_levels
            .keys()
            .chain(self.sell_levels.keys())
            .copied()
            .collect();
        prices.sort_unstable();
        prices.dedup();
        prices
    }

    /// Emir kitabı özeti.
    pub fn summary(&self) -> OrderBookSummary {
        OrderBookSummary {
            slot: self.slot,
            buy_order_count: self.buy_count(),
            sell_order_count: self.sell_count(),
            best_bid: self.best_bid(),
            best_ask: self.best_ask(),
            mid_price: self.mid_price(),
            total_buy_volume: self.total_buy_volume,
            total_sell_volume: self.total_sell_volume,
        }
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Order, OrderClass, OrderId, OrderSide, TokenId, TokenPair, PRICE_PRECISION};
    use nova_primitives::types::{Address, SlotId};

    fn test_pair() -> TokenPair {
        TokenPair::new(TokenId([1u8; 20]), TokenId::NSC)
    }

    fn test_order(side: OrderSide, amount: u128, price: u128, seed: u8) -> Order {
        let mut id_bytes = [0u8; 32];
        id_bytes[0] = seed;
        Order {
            id: OrderId::from_bytes(id_bytes),
            side,
            pair: test_pair(),
            amount,
            limit_price: price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(1000),
            trader: Address::from_bytes([seed; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    fn make_book() -> OrderBook {
        OrderBook::new(SlotId(1), test_pair(), 10_000)
    }

    #[test]
    fn test_emir_kitabi_bos_olusturma() {
        let book = make_book();
        assert!(book.is_empty());
        assert_eq!(book.order_count(), 0);
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert!(!book.has_price_intersection());
    }

    #[test]
    fn test_alim_emri_ekleme() {
        let mut book = make_book();
        let order = test_order(OrderSide::Buy, 100, 200, 1);
        book.add_order(order).unwrap();

        assert_eq!(book.order_count(), 1);
        assert_eq!(book.buy_count(), 1);
        assert_eq!(book.sell_count(), 0);
        assert_eq!(book.best_bid(), Some(200 * PRICE_PRECISION));
        assert_eq!(book.total_buy_volume, 100);
    }

    #[test]
    fn test_satim_emri_ekleme() {
        let mut book = make_book();
        let order = test_order(OrderSide::Sell, 80, 198, 2);
        book.add_order(order).unwrap();

        assert_eq!(book.order_count(), 1);
        assert_eq!(book.sell_count(), 1);
        assert_eq!(book.best_ask(), Some(198 * PRICE_PRECISION));
        assert_eq!(book.total_sell_volume, 80);
    }

    #[test]
    fn test_en_iyi_bid_ask_siralamasi() {
        let mut book = make_book();

        // 3 alım emri
        book.add_order(test_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 50, 205, 2)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 200, 195, 3)).unwrap();

        // En yüksek bid = 205
        assert_eq!(book.best_bid(), Some(205 * PRICE_PRECISION));

        // 3 satım emri
        book.add_order(test_order(OrderSide::Sell, 80, 198, 4)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 120, 205, 5)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 75, 200, 6)).unwrap();

        // En düşük ask = 198
        assert_eq!(book.best_ask(), Some(198 * PRICE_PRECISION));
    }

    #[test]
    fn test_fiyat_kesisimi_kontrolu() {
        let mut book = make_book();

        // bid < ask → kesişim yok
        book.add_order(test_order(OrderSide::Buy, 100, 195, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 200, 2)).unwrap();
        assert!(!book.has_price_intersection(), "bid=195 < ask=200, kesişim yok");

        let mut book2 = make_book();
        // bid >= ask → kesişim var
        book2.add_order(test_order(OrderSide::Buy, 100, 205, 3)).unwrap();
        book2.add_order(test_order(OrderSide::Sell, 80, 200, 4)).unwrap();
        assert!(book2.has_price_intersection(), "bid=205 >= ask=200, kesişim var");
    }

    #[test]
    fn test_spread_hesaplamasi() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Buy, 100, 198, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 202, 2)).unwrap();

        let spread = book.spread().unwrap();
        assert_eq!(spread, 4 * PRICE_PRECISION, "spread = ask - bid = 202 - 198 = 4");
    }

    #[test]
    fn test_mid_price_hesaplamasi() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Buy, 100, 198, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 202, 2)).unwrap();

        let mid = book.mid_price().unwrap();
        assert_eq!(mid, 200 * PRICE_PRECISION, "mid = (198 + 202) / 2 = 200");
    }

    #[test]
    fn test_emir_kaldirma() {
        let mut book = make_book();
        let order = test_order(OrderSide::Buy, 100, 200, 1);
        let id = order.id;
        book.add_order(order).unwrap();

        assert_eq!(book.order_count(), 1);

        let removed = book.remove_order(&id);
        assert!(removed.is_some());
        assert_eq!(book.order_count(), 0);
        assert_eq!(book.total_buy_volume, 0);
        assert_eq!(book.best_bid(), None);
    }

    #[test]
    fn test_yinelenen_emir_reddedilir() {
        let mut book = make_book();
        let order = test_order(OrderSide::Buy, 100, 200, 1);
        book.add_order(order.clone()).unwrap();

        let err = book.add_order(order).unwrap_err();
        assert!(matches!(err, AuctionError::DuplicateOrder { .. }));
    }

    #[test]
    fn test_kapali_kitap_emir_reddeder() {
        let mut book = make_book();
        book.close();

        let order = test_order(OrderSide::Buy, 100, 200, 1);
        let err = book.add_order(order).unwrap_err();
        assert!(matches!(err, AuctionError::OrderBookClosed { .. }));
    }

    #[test]
    fn test_kapasite_doluyor() {
        let mut book = OrderBook::new(SlotId(1), test_pair(), 2);

        book.add_order(test_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 198, 2)).unwrap();

        let err = book.add_order(test_order(OrderSide::Buy, 50, 205, 3)).unwrap_err();
        assert!(matches!(err, AuctionError::OrderBookFull { current: 2, maximum: 2 }));
    }

    #[test]
    fn test_kumulatif_talep_hesaplama() {
        let mut book = make_book();

        // 3 alım emri: $210'da 300, $205'te 100, $200'de 200
        book.add_order(test_order(OrderSide::Buy, 300, 210, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 100, 205, 2)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 200, 200, 3)).unwrap();

        // D($210) = 300 (sadece $210+ emirler)
        assert_eq!(book.cumulative_demand_at(210 * PRICE_PRECISION), 300);
        // D($205) = 300 + 100 = 400
        assert_eq!(book.cumulative_demand_at(205 * PRICE_PRECISION), 400);
        // D($200) = 300 + 100 + 200 = 600
        assert_eq!(book.cumulative_demand_at(200 * PRICE_PRECISION), 600);
        // D($215) = 0 (bu fiyata alım yok)
        assert_eq!(book.cumulative_demand_at(215 * PRICE_PRECISION), 0);
    }

    #[test]
    fn test_kumulatif_arz_hesaplama() {
        let mut book = make_book();

        // 3 satım emri: $198'de 80, $200'de 75, $205'te 120
        book.add_order(test_order(OrderSide::Sell, 80, 198, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 75, 200, 2)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 120, 205, 3)).unwrap();

        // S($198) = 80
        assert_eq!(book.cumulative_supply_at(198 * PRICE_PRECISION), 80);
        // S($200) = 80 + 75 = 155
        assert_eq!(book.cumulative_supply_at(200 * PRICE_PRECISION), 155);
        // S($205) = 80 + 75 + 120 = 275
        assert_eq!(book.cumulative_supply_at(205 * PRICE_PRECISION), 275);
        // S($190) = 0
        assert_eq!(book.cumulative_supply_at(190 * PRICE_PRECISION), 0);
    }

    #[test]
    fn test_alim_emirleri_azalan_sira() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 50, 210, 2)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 200, 195, 3)).unwrap();

        let prices: Vec<u128> = book
            .buy_levels_desc()
            .map(|l| l.price)
            .collect();

        assert_eq!(prices, vec![210 * PRICE_PRECISION, 200 * PRICE_PRECISION, 195 * PRICE_PRECISION],
            "Alım emirleri fiyata göre azalan sırada olmalı");
    }

    #[test]
    fn test_satim_emirleri_artan_sira() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Sell, 80, 205, 1)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 120, 198, 2)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 75, 200, 3)).unwrap();

        let prices: Vec<u128> = book
            .sell_levels_asc()
            .map(|l| l.price)
            .collect();

        assert_eq!(prices, vec![198 * PRICE_PRECISION, 200 * PRICE_PRECISION, 205 * PRICE_PRECISION],
            "Satım emirleri fiyata göre artan sırada olmalı");
    }

    #[test]
    fn test_ozet_hesaplama() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 50, 205, 2)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 198, 3)).unwrap();

        let summary = book.summary();
        assert_eq!(summary.buy_order_count, 2);
        assert_eq!(summary.sell_order_count, 1);
        assert_eq!(summary.best_bid, Some(205 * PRICE_PRECISION));
        assert_eq!(summary.best_ask, Some(198 * PRICE_PRECISION));
        assert_eq!(summary.total_buy_volume, 150);
        assert_eq!(summary.total_sell_volume, 80);
    }

    #[test]
    fn test_tum_fiyat_noktalari() {
        let mut book = make_book();
        book.add_order(test_order(OrderSide::Buy, 100, 210, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 50, 205, 2)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 80, 198, 3)).unwrap();
        book.add_order(test_order(OrderSide::Sell, 75, 205, 4)).unwrap();

        let prices = book.all_price_points();
        // Sıralı ve tekrarsız: [198, 205, 210]
        assert_eq!(prices, vec![198 * PRICE_PRECISION, 205 * PRICE_PRECISION, 210 * PRICE_PRECISION]);
    }

    #[test]
    fn test_coklu_emir_ayni_fiyat_seviyesi() {
        let mut book = make_book();

        // Aynı fiyatta birden fazla emir
        book.add_order(test_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 200, 200, 2)).unwrap();
        book.add_order(test_order(OrderSide::Buy, 50, 200, 3)).unwrap();

        assert_eq!(book.buy_count(), 3);
        assert_eq!(book.total_buy_volume, 350);
        // Sadece 1 fiyat seviyesi
        assert_eq!(book.buy_levels.len(), 1);
        assert_eq!(book.cumulative_demand_at(200 * PRICE_PRECISION), 350);
    }
}
