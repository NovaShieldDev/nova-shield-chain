//! Uniform Price Batch Auction Clearing Motoru.
//!
//! §5.3.2 — Arz/talep eğrisi kesişim algoritması ile clearing price belirlenir.
//!
//! ## Tasarım İlhamı: CoW Protocol + Nova Whitepaper
//!
//! CoW Protocol'ün settlement encoder'ından öğrendiğimiz ders:
//! - Tüm emirler **aynı** clearing price'ta işleme girer (Uniform Price Auction)
//! - Bu fiyat her emrin kendi limitinden **iyi veya eşit** olmalıdır
//! - Aksi hâlde bireysel çalıştırma daha iyidir → fairness guarantee
//!
//! ## Algoritma: Arz/Talep Kesişimi
//!
//! ```text
//! Miktar
//!   ↑
//!   │     ╔══════════╗
//!   │     ║  Talep   ║    ╱╲    ← D(P): limit_price >= P olan alım emirleri toplamı
//!   │     ║  Eğrisi  ╚══╱══╝
//!   │                  ╱
//!   │                ╱ ← KESIŞIM NOKTASI = Clearing Price
//!   │              ╱
//!   │    ╔═══════╱══════════╗
//!   │    ║     ╱  Arz Eğrisi║ → S(P): limit_price <= P olan satım emirleri toplamı
//!   │    ╚═══╱══════════════╝
//!   └──────────────────────────→ Fiyat
//!                P_clear
//! ```
//!
//! **Adımlar:**
//! 1. Tüm benzersiz fiyat noktalarını topla (alım + satım emirlerinden)
//! 2. Her fiyat noktası için kümülatif D(P) ve S(P) hesapla
//! 3. En yüksek işlem hacmini [min(D(P), S(P))] maximize eden P'yi bul
//! 4. Birden fazla optimal P varsa: en düşüğünü seç (alıcı lehine)
//! 5. Sınır (marginal) emir seviyesinde kısmi doldurma uygula
//!
//! ## CoW Protocol Farkı
//!
//! CoW Protocol birden fazla token pair'i tek settlement'ta birleştirir
//! (Coincidence of Wants — CoW). Nova'da her slot'ta her pair bağımsız
//! clearing çalıştırılır — basitlik ve determinizm önceliklidir.

use crate::errors::{AuctionError, AuctionResult};
use crate::orderbook::OrderBook;
use crate::types::{ClearingResult, Order, PRICE_PRECISION};
use nova_primitives::types::SlotId;
use tracing::{debug, info, trace, warn};

// ─── KESİŞİM NOKTALARI ──────────────────────────────────────────────────────

/// Bir fiyat noktasındaki arz/talep analizi.
#[derive(Debug, Clone)]
pub struct PricePoint {
    /// Fiyat (PRICE_PRECISION ile normalleştirilmiş).
    pub price: u128,

    /// Bu fiyatta kümülatif talep [D(P)].
    /// limit_price >= P olan tüm alım emirlerinin toplam miktarı.
    pub cumulative_demand: u128,

    /// Bu fiyatta kümülatif arz [S(P)].
    /// limit_price <= P olan tüm satım emirlerinin toplam miktarı.
    pub cumulative_supply: u128,

    /// Bu fiyatta işlenebilecek maksimum hacim.
    /// `min(cumulative_demand, cumulative_supply)`
    pub tradeable_volume: u128,
}

impl PricePoint {
    /// Yeni fiyat noktası oluştur.
    pub fn new(price: u128, cumulative_demand: u128, cumulative_supply: u128) -> Self {
        let tradeable_volume = cumulative_demand.min(cumulative_supply);
        Self {
            price,
            cumulative_demand,
            cumulative_supply,
            tradeable_volume,
        }
    }

    /// Arz talebe eşit mi veya talebin altında mı?
    /// Eşit noktada clearing mümkün.
    pub fn is_balanced(&self) -> bool {
        self.cumulative_supply >= self.cumulative_demand
    }
}

// ─── CLEARING GİRİŞ/ÇIKIŞ ───────────────────────────────────────────────────

/// Clearing işleminin analiz özeti.
#[derive(Debug, Clone)]
pub struct ClearingAnalysis {
    /// Seçilen clearing price.
    pub clearing_price: u128,

    /// Tüm fiyat noktaları (analiz için).
    pub price_points: Vec<PricePoint>,

    /// Optimal fiyat noktasının indeksi.
    pub optimal_index: usize,

    /// Toplam çalıştırılabilir alım hacmi (clearing price'ta).
    pub total_buy_volume: u128,

    /// Toplam çalıştırılabilir satım hacmi (clearing price'ta).
    pub total_sell_volume: u128,

    /// Maksimum işlenebilecek hacim.
    pub max_tradeable_volume: u128,

    /// Sınır alım emri: clearing fiyatında tam olarak limit olan emir.
    /// Kısmi doldurmaya aday.
    pub marginal_buy_price: Option<u128>,

    /// Sınır satım emri: clearing fiyatında tam olarak limit olan emir.
    pub marginal_sell_price: Option<u128>,
}

// ─── CLEARING MOTORU ─────────────────────────────────────────────────────────

/// Batch Auction Clearing Motoru.
///
/// §5.3.2 — Arz/talep eğrisi kesişim algoritması ile clearing price belirler.
///
/// Rolling Shutter'ın slot-based trigger modelinden ilham alınarak:
/// Her slot'ta `run_clearing()` bir kez çalışır, sonuç deterministiktir.
pub struct ClearingEngine {
    /// Minimum işlem hacmi (çok küçük işlemleri filtrele).
    pub min_tradeable_volume: u128,

    /// Fiyat hassasiyet faktörü.
    pub price_precision: u128,
}

impl Default for ClearingEngine {
    fn default() -> Self {
        Self {
            min_tradeable_volume: 0,
            price_precision: PRICE_PRECISION,
        }
    }
}

impl ClearingEngine {
    /// Yeni clearing motoru oluştur.
    pub fn new(min_tradeable_volume: u128) -> Self {
        Self {
            min_tradeable_volume,
            price_precision: PRICE_PRECISION,
        }
    }

    /// Clearing price bul.
    ///
    /// `OrderBook`'taki tüm emirlere bakarak en uygun clearing price'ı
    /// arz/talep kesişim algoritması ile belirler.
    ///
    /// # Hatalar
    /// - `EmptyBatch`: Emir kitabı boşsa
    /// - `UnilateralBatch`: Sadece alım veya sadece satım varsa
    /// - `NoPriceIntersection`: Alım/satım fiyatları kesişmiyorsa
    pub fn find_clearing_price(
        &self,
        book: &OrderBook,
        slot: SlotId,
    ) -> AuctionResult<ClearingAnalysis> {
        // ── 1. Ön kontroller ─────────────────────────────────────────────────
        if book.is_empty() {
            return Err(AuctionError::EmptyBatch { slot });
        }

        let buy_count = book.buy_count();
        let sell_count = book.sell_count();

        if buy_count == 0 || sell_count == 0 {
            return Err(AuctionError::UnilateralBatch {
                buy_count,
                sell_count,
            });
        }

        let best_bid = book.best_bid().ok_or(AuctionError::UnilateralBatch {
            buy_count,
            sell_count,
        })?;
        let best_ask = book.best_ask().ok_or(AuctionError::UnilateralBatch {
            buy_count,
            sell_count,
        })?;

        // Temel kesişim kontrolü: en yüksek alım >= en düşük satım olmalı
        if best_bid < best_ask {
            return Err(AuctionError::NoPriceIntersection {
                best_bid,
                best_ask,
            });
        }

        debug!(
            slot = slot.0,
            best_bid = best_bid,
            best_ask = best_ask,
            buy_orders = buy_count,
            sell_orders = sell_count,
            "Clearing price aranıyor"
        );

        // ── 2. Fiyat noktaları oluştur ────────────────────────────────────────
        let price_points_raw = book.all_price_points();

        if price_points_raw.is_empty() {
            return Err(AuctionError::EmptyBatch { slot });
        }

        let price_points: Vec<PricePoint> = price_points_raw
            .iter()
            .map(|&p| {
                let demand = book.cumulative_demand_at(p);
                let supply = book.cumulative_supply_at(p);
                PricePoint::new(p, demand, supply)
            })
            .collect();

        trace!(
            slot = slot.0,
            price_point_count = price_points.len(),
            "Fiyat noktaları hesaplandı"
        );

        // ── 3. Optimal clearing price bul ────────────────────────────────────
        //
        // Hedef: tradeable_volume = min(D(P), S(P)) maksimum olan P.
        //
        // CoW Protocol'ün uniform clearing price yaklaşımı:
        // - Arz talep eğrisi kesişim noktasının biraz solunda clearing price seç
        // - Bu hem alıcı hem satıcı için en adil noktadır
        //
        // Özel durum: Birden fazla aynı maksimum hacme sahip P varsa
        // → En DÜŞÜK fiyatı seç (alıcı lehine, whitepaper §5.3.2)

        let max_volume = price_points
            .iter()
            .map(|pp| pp.tradeable_volume)
            .max()
            .unwrap_or(0);

        if max_volume == 0 || max_volume < self.min_tradeable_volume {
            return Err(AuctionError::NoPriceIntersection {
                best_bid,
                best_ask,
            });
        }

        // En yüksek hacmi veren fiyat noktalarından en düşük fiyatı seç
        let optimal_index = price_points
            .iter()
            .position(|pp| pp.tradeable_volume == max_volume)
            .expect("max_volume fiyat noktasında bulunmalı");

        let clearing_price = price_points[optimal_index].price;

        info!(
            slot = slot.0,
            clearing_price = clearing_price,
            clearing_price_human = format!("{:.6}", clearing_price as f64 / PRICE_PRECISION as f64),
            max_volume = max_volume,
            "Clearing price bulundu"
        );

        // ── 4. Sınır fiyat analizi ────────────────────────────────────────────
        //
        // Clearing price'ta tam olarak limit fiyatına eşit olan emirler
        // sınır (marginal) emirlerdir. Bunlar kısmi doldurmaya adaydır.

        let total_buy_volume = price_points[optimal_index].cumulative_demand;
        let total_sell_volume = price_points[optimal_index].cumulative_supply;

        // Sınır alım fiyatı: clearing price'ın hemen altında olan en yüksek alım
        let marginal_buy_price = if total_buy_volume > max_volume {
            // Talep arzı aşıyor → clearing price sınır alım noktası
            Some(clearing_price)
        } else {
            None
        };

        // Sınır satım fiyatı: clearing price'ın hemen üstünde olan en düşük satım
        let marginal_sell_price = if total_sell_volume > max_volume {
            Some(clearing_price)
        } else {
            None
        };

        Ok(ClearingAnalysis {
            clearing_price,
            price_points,
            optimal_index,
            total_buy_volume,
            total_sell_volume,
            max_tradeable_volume: max_volume,
            marginal_buy_price,
            marginal_sell_price,
        })
    }

    /// Clearing price'ın adilliğini doğrula.
    ///
    /// CoW Protocol fairness guarantee:
    /// Hiçbir emir bireysel çalıştırmadan daha kötü fiyat almamalı.
    ///
    /// Nova whitepaper §5.3.2: Tüm emirler kendi limit fiyatından
    /// iyi veya eşit fiyatta çalıştırılır.
    pub fn verify_fairness(&self, clearing_price: u128, orders: &[&Order]) -> bool {
        orders.iter().all(|order| order.price_acceptable(clearing_price))
    }

    /// Price improvement hesapla (kullanıcı lehine kazanım).
    ///
    /// CoW Protocol'ün "surplus" kavramının Nova versiyonu.
    /// Kullanıcı limit fiyat ile clearing price arasındaki fark → MEV önleme kanıtı.
    ///
    /// Alım emrinde: kullanıcı `limit_price`'tan ödemeyi kabul etti,
    /// `clearing_price` daha ucuzsa fark kullanıcının kazancıdır.
    ///
    /// Satım emrinde: kullanıcı `limit_price`'ta satmayı kabul etti,
    /// `clearing_price` daha pahalıysa fark kullanıcının kazancıdır.
    pub fn compute_price_improvement(
        &self,
        order: &Order,
        clearing_price: u128,
        executed_amount: u128,
    ) -> i128 {
        let improvement_per_unit = match order.side {
            crate::types::OrderSide::Buy => {
                // Alım: limit - clearing (clearing < limit ise pozitif)
                order.limit_price as i128 - clearing_price as i128
            }
            crate::types::OrderSide::Sell => {
                // Satım: clearing - limit (clearing > limit ise pozitif)
                clearing_price as i128 - order.limit_price as i128
            }
        };

        // Toplam improvement: birim * miktar / PRICE_PRECISION
        improvement_per_unit
            .saturating_mul(executed_amount as i128)
            .checked_div(PRICE_PRECISION as i128)
            .unwrap_or(0)
    }

    /// Bir emrin tam doldurma için gereken `quote` token miktarını hesapla.
    ///
    /// CoW Protocol settlement encoder'dan öğrenilen yöntem:
    /// `quote_amount = amount * clearing_price / PRICE_PRECISION`
    pub fn compute_quote_amount(
        &self,
        base_amount: u128,
        clearing_price: u128,
    ) -> Option<u128> {
        base_amount
            .checked_mul(clearing_price)?
            .checked_div(self.price_precision)
    }

    /// Kısmi doldurma için gereken miktarı hesapla.
    ///
    /// Arz > Talep durumunda: satım emirleri sınırda kesiliyor.
    /// Talep > Arz durumunda: alım emirleri sınırda kesiliyor.
    ///
    /// Emir kitabındaki marginal fiyat seviyesindeki emirler
    /// pro-rata (oransal) kısmi doldurma alır.
    ///
    /// `total_available`: Bu fiyat seviyesinde kalan slot
    /// `level_total`: Bu fiyat seviyesindeki toplam miktar
    /// `order_amount`: Bu emrin miktarı
    ///
    /// Returns: Bu emrin alacağı doldurma miktarı
    pub fn compute_partial_fill(
        &self,
        total_available: u128,
        level_total: u128,
        order_amount: u128,
    ) -> u128 {
        if level_total == 0 {
            return 0;
        }
        // Pro-rata: order_amount * total_available / level_total
        order_amount
            .saturating_mul(total_available)
            / level_total
    }

    /// AŞAMA 7 Görev 53: Uniform directional clearing price doğrulaması.
    ///
    /// Whitepaper §5.3.2: "Tek bir clearing price — hem alım hem satım
    /// emirlerine uygulanan tek bir uniform fiyat."
    ///
    /// ## Kural
    ///
    /// Bir batch solution geçerli sayılmak için:
    /// 1. Tüm alım emirleri AYNI clearing price'ta çalıştırılmalı
    /// 2. Tüm satım emirleri AYNI clearing price'ta çalıştırılmalı
    /// 3. Alım clearing price == Satım clearing price (tek uniform değer)
    ///
    /// Bu kural MEV'e karşı güvence sağlar: farklı gruplar için
    /// farklı fiyat kullanılması price discrimination yaratır.
    ///
    /// ## CoW Protocol Referans
    ///
    /// `crates/driver/src/domain/competition/solution/mod.rs`:
    /// `prices: ClearingPrices` → token başına tek bir clearing fiyatı
    ///
    /// Tüm trade'ler bu map'ten fiyatını alır — birden fazla fiyat olamaz.
    pub fn verify_uniform_clearing_price(
        &self,
        clearing_price: u128,
        buy_execution_prices: &[u128],
        sell_execution_prices: &[u128],
    ) -> UniformPriceVerdict {
        // Alım emirlerinin tümü clearing price'ta olmalı
        let bad_buys: Vec<u128> = buy_execution_prices
            .iter()
            .copied()
            .filter(|&p| p != clearing_price)
            .collect();

        // Satım emirlerinin tümü clearing price'ta olmalı
        let bad_sells: Vec<u128> = sell_execution_prices
            .iter()
            .copied()
            .filter(|&p| p != clearing_price)
            .collect();

        if bad_buys.is_empty() && bad_sells.is_empty() {
            UniformPriceVerdict::Valid
        } else {
            UniformPriceVerdict::Violation {
                clearing_price,
                non_uniform_buy_prices: bad_buys,
                non_uniform_sell_prices: bad_sells,
            }
        }
    }
}

/// Uniform clearing price doğrulama sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniformPriceVerdict {
    /// Tüm emirler aynı clearing price'ta — geçerli.
    Valid,
    /// Bazı emirler farklı fiyatta — ihlal.
    Violation {
        /// Beklenen clearing price.
        clearing_price: u128,
        /// Yanlış fiyattaki alım emirlerinin fiyatları.
        non_uniform_buy_prices: Vec<u128>,
        /// Yanlış fiyattaki satım emirlerinin fiyatları.
        non_uniform_sell_prices: Vec<u128>,
    },
}

impl UniformPriceVerdict {
    pub fn is_valid(&self) -> bool {
        matches!(self, UniformPriceVerdict::Valid)
    }
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

    fn make_order(side: OrderSide, amount: u128, price: u128, seed: u8) -> Order {
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

    fn make_book_with_orders(
        buys: &[(u128, u128)],  // (amount, price)
        sells: &[(u128, u128)], // (amount, price)
    ) -> OrderBook {
        let mut book = OrderBook::new(SlotId(1), test_pair(), 10_000);
        for (i, &(amount, price)) in buys.iter().enumerate() {
            book.add_order(make_order(OrderSide::Buy, amount, price, i as u8)).unwrap();
        }
        for (i, &(amount, price)) in sells.iter().enumerate() {
            book.add_order(make_order(OrderSide::Sell, amount, price, (i + 100) as u8)).unwrap();
        }
        book
    }

    fn engine() -> ClearingEngine {
        ClearingEngine::default()
    }

    // ─── Temel Clearing Price Testleri ───────────────────────────────────────

    #[test]
    fn test_bos_kitap_hata_verir() {
        let book = OrderBook::new(SlotId(1), test_pair(), 1000);
        let err = engine().find_clearing_price(&book, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::EmptyBatch { .. }));
    }

    #[test]
    fn test_sadece_alim_tek_tarafli_hata() {
        let mut book = OrderBook::new(SlotId(1), test_pair(), 1000);
        book.add_order(make_order(OrderSide::Buy, 100, 200, 1)).unwrap();
        let err = engine().find_clearing_price(&book, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::UnilateralBatch { buy_count: 1, sell_count: 0 }));
    }

    #[test]
    fn test_sadece_satim_tek_tarafli_hata() {
        let mut book = OrderBook::new(SlotId(1), test_pair(), 1000);
        book.add_order(make_order(OrderSide::Sell, 100, 200, 1)).unwrap();
        let err = engine().find_clearing_price(&book, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::UnilateralBatch { buy_count: 0, sell_count: 1 }));
    }

    #[test]
    fn test_fiyat_kesisimi_yok_hata() {
        // bid < ask → kesişim yok
        let book = make_book_with_orders(
            &[(100, 190)],  // alım: $190
            &[(100, 200)],  // satım: $200
        );
        let err = engine().find_clearing_price(&book, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::NoPriceIntersection { .. }));
    }

    #[test]
    fn test_tek_emir_cifti_clearing() {
        // Alım $200, satım $200 → clearing = $200
        let book = make_book_with_orders(
            &[(100, 200)],
            &[(100, 200)],
        );
        let analysis = engine().find_clearing_price(&book, SlotId(1)).unwrap();
        assert_eq!(analysis.clearing_price, 200 * PRICE_PRECISION);
        assert_eq!(analysis.max_tradeable_volume, 100);
    }

    #[test]
    fn test_standart_arzan_talep_kesisimi() {
        // Klasik senaryo:
        // Alım:  300@$210, 100@$205, 200@$200
        // Satım: 80@$198,  75@$200,  120@$205
        //
        // D($198) = 600, S($198) = 80  → tradeable = 80
        // D($200) = 600, S($200) = 155 → tradeable = 155
        // D($205) = 400, S($205) = 275 → tradeable = 275
        // D($210) = 300, S($210) = 275 → tradeable = 275
        //
        // Maksimum: 275 hem $205'te hem $210'da
        // En düşük seçilir → clearing = $205
        let book = make_book_with_orders(
            &[(300, 210), (100, 205), (200, 200)],
            &[(80, 198), (75, 200), (120, 205)],
        );
        let analysis = engine().find_clearing_price(&book, SlotId(1)).unwrap();
        // Max volume olan en düşük fiyat seçilmeli
        assert_eq!(analysis.max_tradeable_volume, 275);
        // Clearing price $205 veya $210 olabilir (max volume aynı)
        // Algoritmamız en düşüğü seçmeli
        assert!(analysis.clearing_price == 205 * PRICE_PRECISION
                || analysis.clearing_price == 210 * PRICE_PRECISION);
    }

    #[test]
    fn test_bid_ask_kesisimi_clearing_dogrulama() {
        // Basit: bid=205, ask=200 → clearing tam kesişim noktası
        // Alım: 500@$205, Satım: 500@$200
        // D($200) = 500, S($200) = 500 → tradeable = 500
        // D($205) = 500, S($205) = 500 → tradeable = 500
        // En düşük: $200
        let book = make_book_with_orders(
            &[(500, 205)],
            &[(500, 200)],
        );
        let analysis = engine().find_clearing_price(&book, SlotId(1)).unwrap();
        assert_eq!(analysis.max_tradeable_volume, 500);
        assert_eq!(analysis.clearing_price, 200 * PRICE_PRECISION);
    }

    // ─── Adillik (Fairness) Testleri ──────────────────────────────────────────

    #[test]
    fn test_fairness_alim_emirleri() {
        let eng = engine();
        let clearing = 200 * PRICE_PRECISION;

        // limit = 210 > clearing = 200 → kabul edilir
        let good_buy = make_order(OrderSide::Buy, 100, 210, 1);
        // limit = 195 < clearing = 200 → reddedilir (slippage %0.5)
        let bad_buy = make_order(OrderSide::Buy, 100, 195, 2);

        assert!(eng.verify_fairness(clearing, &[&good_buy]),
            "limit > clearing, alım kabul edilmeli");
        assert!(!eng.verify_fairness(clearing, &[&bad_buy]),
            "limit < clearing - slippage, alım reddedilmeli");
    }

    #[test]
    fn test_fairness_satim_emirleri() {
        let eng = engine();
        let clearing = 200 * PRICE_PRECISION;

        // limit = 195 < clearing = 200 → kabul edilir (iyi fiyat aldı)
        let good_sell = make_order(OrderSide::Sell, 100, 195, 1);
        // limit = 205 > clearing = 200 (slippage %0.5 → min = 203.97) → reddedilir
        let bad_sell = make_order(OrderSide::Sell, 100, 205, 2);

        assert!(eng.verify_fairness(clearing, &[&good_sell]),
            "clearing > limit, satım kabul edilmeli");
        assert!(!eng.verify_fairness(clearing, &[&bad_sell]),
            "clearing < limit - slippage, satım reddedilmeli");
    }

    // ─── Price Improvement Testleri ───────────────────────────────────────────

    #[test]
    fn test_price_improvement_alim() {
        let eng = engine();
        // limit = 210, clearing = 200 → improvement = (210 - 200) * 100 / PRICE_PRECISION
        let order = make_order(OrderSide::Buy, 100, 210, 1);
        let clearing = 200 * PRICE_PRECISION;
        let improvement = eng.compute_price_improvement(&order, clearing, 100);
        // (210 - 200) * 100 = 1000 (PRICE_PRECISION ile normalize)
        assert_eq!(improvement, 1000);
    }

    #[test]
    fn test_price_improvement_satim() {
        let eng = engine();
        // limit = 195, clearing = 200 → improvement = (200 - 195) * 100 / PRICE_PRECISION
        let order = make_order(OrderSide::Sell, 100, 195, 1);
        let clearing = 200 * PRICE_PRECISION;
        let improvement = eng.compute_price_improvement(&order, clearing, 100);
        assert_eq!(improvement, 500);
    }

    #[test]
    fn test_price_improvement_sifir_tam_limit() {
        let eng = engine();
        // limit = clearing → improvement = 0
        let order = make_order(OrderSide::Buy, 100, 200, 1);
        let clearing = 200 * PRICE_PRECISION;
        let improvement = eng.compute_price_improvement(&order, clearing, 100);
        assert_eq!(improvement, 0);
    }

    // ─── Quote Amount Testleri ────────────────────────────────────────────────

    #[test]
    fn test_quote_amount_hesaplama() {
        let eng = engine();
        // base_amount=100, clearing_price=200 * PRICE_PRECISION
        // quote = 100 * 200 * PRICE_PRECISION / PRICE_PRECISION = 20_000
        let quote = eng.compute_quote_amount(100, 200 * PRICE_PRECISION).unwrap();
        assert_eq!(quote, 20_000);
    }

    #[test]
    fn test_quote_amount_kesirli() {
        let eng = engine();
        // base=3, price=100 * PRICE_PRECISION → quote = 300
        let quote = eng.compute_quote_amount(3, 100 * PRICE_PRECISION).unwrap();
        assert_eq!(quote, 300);
    }

    // ─── Partial Fill Testleri ────────────────────────────────────────────────

    #[test]
    fn test_kismi_doldurma_pro_rata() {
        let eng = engine();
        // Toplam 100 slot var, seviyede 500 toplam miktar, bu emir 200
        // Bu emrin alacağı: 200 * 100 / 500 = 40
        let filled = eng.compute_partial_fill(100, 500, 200);
        assert_eq!(filled, 40);
    }

    #[test]
    fn test_kismi_doldurma_sifir_seviye() {
        let eng = engine();
        let filled = eng.compute_partial_fill(100, 0, 200);
        assert_eq!(filled, 0);
    }

    #[test]
    fn test_kismi_doldurma_tam_dolu() {
        let eng = engine();
        // total_available = level_total → tam doldurma
        let filled = eng.compute_partial_fill(500, 500, 200);
        assert_eq!(filled, 200);
    }

    // ─── Price Point Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_price_point_dengeli_kontrol() {
        // Talep = arz → dengeli
        let pp = PricePoint::new(200, 500, 500);
        assert!(pp.is_balanced());
        assert_eq!(pp.tradeable_volume, 500);

        // Arz > talep → dengeli
        let pp2 = PricePoint::new(200, 300, 500);
        assert!(pp2.is_balanced());
        assert_eq!(pp2.tradeable_volume, 300);

        // Talep > arz → dengesiz
        let pp3 = PricePoint::new(200, 500, 300);
        assert!(!pp3.is_balanced());
        assert_eq!(pp3.tradeable_volume, 300);
    }

    #[test]
    fn test_coklu_alim_satim_clearing() {
        // 10 alım, 10 satım — gerçekçi senaryo
        let book = make_book_with_orders(
            &[
                (100, 215), (200, 212), (150, 210),
                (300, 208), (250, 205), (100, 202),
                (400, 200), (200, 198), (150, 195), (100, 192),
            ],
            &[
                (80, 190), (120, 193), (200, 195),
                (150, 198), (300, 200), (200, 202),
                (100, 205), (250, 208), (150, 210), (100, 212),
            ],
        );

        let analysis = engine().find_clearing_price(&book, SlotId(1)).unwrap();

        // Clearing price belirlenmeli (hata vermemeli)
        assert!(analysis.clearing_price > 0);
        assert!(analysis.max_tradeable_volume > 0);

        // Adillik: clearing price tüm çalıştırılabilir emirler için kabul edilebilir
        let buy_orders: Vec<Order> = book.all_buy_orders_desc()
            .iter()
            .filter(|o| o.limit_price >= analysis.clearing_price)
            .map(|&o| o.clone())
            .collect();
        let sell_orders: Vec<Order> = book.all_sell_orders_asc()
            .iter()
            .filter(|o| o.limit_price <= analysis.clearing_price)
            .map(|&o| o.clone())
            .collect();

        let buy_refs: Vec<&Order> = buy_orders.iter().collect();
        let sell_refs: Vec<&Order> = sell_orders.iter().collect();

        assert!(
            engine().verify_fairness(analysis.clearing_price, &buy_refs),
            "Tüm çalıştırılabilir alım emirleri adil fiyat almalı"
        );
        assert!(
            engine().verify_fairness(analysis.clearing_price, &sell_refs),
            "Tüm çalıştırılabilir satım emirleri adil fiyat almalı"
        );
    }

    // ─── Uniform Clearing Price Testleri ──────────────────────────────────────

    #[test]
    fn test_uniform_price_valid() {
        let cp = 200 * PRICE_PRECISION;
        let buys = vec![cp, cp, cp];
        let sells = vec![cp, cp];
        let verdict = engine().verify_uniform_clearing_price(cp, &buys, &sells);
        assert!(verdict.is_valid());
    }

    #[test]
    fn test_uniform_price_buy_violation() {
        let cp = 200 * PRICE_PRECISION;
        let bad = 195 * PRICE_PRECISION;
        let buys = vec![cp, bad]; // biri yanlış
        let sells = vec![cp];
        let verdict = engine().verify_uniform_clearing_price(cp, &buys, &sells);
        assert!(!verdict.is_valid());
        if let UniformPriceVerdict::Violation { non_uniform_buy_prices, .. } = verdict {
            assert_eq!(non_uniform_buy_prices, vec![bad]);
        } else { panic!("Violation beklendi"); }
    }

    #[test]
    fn test_uniform_price_sell_violation() {
        let cp = 200 * PRICE_PRECISION;
        let bad = 205 * PRICE_PRECISION;
        let buys = vec![cp];
        let sells = vec![bad]; // biri yanlış
        let verdict = engine().verify_uniform_clearing_price(cp, &buys, &sells);
        assert!(!verdict.is_valid());
        if let UniformPriceVerdict::Violation { non_uniform_sell_prices, .. } = verdict {
            assert_eq!(non_uniform_sell_prices, vec![bad]);
        } else { panic!("Violation beklendi"); }
    }

    #[test]
    fn test_uniform_price_empty_valid() {
        // Hiç emir yok → trivially valid
        let cp = 100 * PRICE_PRECISION;
        let verdict = engine().verify_uniform_clearing_price(cp, &[], &[]);
        assert!(verdict.is_valid());
    }
}
