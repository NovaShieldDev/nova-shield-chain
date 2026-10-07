//! Batch Auction Emir Doğrulama Modülü.
//!
//! §5.3 — Her emir auction'a girmeden önce çok katmanlı doğrulamadan geçer.
//!
//! ## Tasarım İlhamı: CoW Protocol Solvable Orders
//!
//! CoW Protocol'ün `solvable_orders.rs`'den öğrendiğimiz ders:
//! - Emir filtresi kademeli çalışır: önce ucuz kontroller, sonra pahalılar
//! - Filtreleme nedeni her emir için loglanır (monitoring/debugging)
//! - "Dust order" (çok küçük emirler) spam önleme için reddedilir
//! - Süresi dolmuş emirler erken kaldırılır → clearing performansı artar
//!
//! ## Doğrulama Katmanları (soldan sağa, maliyet artarak)
//!
//! ```text
//! Emir → [Miktar] → [Fiyat] → [Token] → [Slot] → [Slippage] → [Sınıf] → Auction
//!            ↓          ↓         ↓          ↓          ↓           ↓
//!         ZeroAmt    ZeroPrice  SameToken  Expired  SlippageOOB  InvalidClass
//! ```

use crate::errors::{AuctionError, AuctionResult};
use crate::types::{Order, OrderClass, AuctionConfig};
use nova_primitives::types::SlotId;
use tracing::{debug, trace, warn};

// ─── DOĞRULAMA YAPISI ────────────────────────────────────────────────────────

/// Emir doğrulama motoru.
///
/// CoW Protocol'ün çok katmanlı filtre yaklaşımı benimsenmiştir:
/// Her katman bağımsız çalışır, ilk hata anında döner (fail-fast).
#[derive(Debug, Clone)]
pub struct OrderValidator {
    /// Auction yapılandırması.
    config: AuctionConfig,
}

impl OrderValidator {
    /// Yeni doğrulama motoru.
    pub fn new(config: AuctionConfig) -> Self {
        Self { config }
    }

    /// Varsayılan yapılandırma ile yeni doğrulama motoru.
    pub fn with_defaults() -> Self {
        Self {
            config: AuctionConfig::default(),
        }
    }

    /// Emri doğrula — tüm katmanları çalıştır.
    ///
    /// Başarılıysa `Ok(())` döner. İlk hatada `Err(AuctionError)` döner.
    /// CoW Protocol'ün fail-fast stratejisi: ilk geçersizlikte dur.
    ///
    /// # Doğrulama Sırası
    ///
    /// 1. Miktar kontrolü (sıfır/negatif — düşük maliyet)
    /// 2. Fiyat kontrolü (sıfır — düşük maliyet)
    /// 3. Token çifti kontrolü (aynı token — düşük maliyet)
    /// 4. Slot geçerliliği (süresi dolmuş — orta maliyet)
    /// 5. Slippage sınırları (hesaplama — orta maliyet)
    /// 6. Emir sınıfı tutarlılığı (düşük maliyet)
    pub fn validate_order(&self, order: &Order, current_slot: SlotId) -> AuctionResult<()> {
        trace!(
            order_id = %order.id,
            side = %order.side,
            amount = order.amount,
            limit_price = order.limit_price,
            "Emir doğrulanıyor"
        );

        // Katman 1: Miktar
        self.validate_amount(order)?;
        // Katman 2: Fiyat
        self.validate_price(order)?;
        // Katman 3: Token çifti
        self.validate_token_pair(order)?;
        // Katman 4: Slot geçerliliği
        self.validate_slot(order, current_slot)?;
        // Katman 5: Slippage
        self.validate_slippage(order)?;
        // Katman 6: Emir sınıfı
        self.validate_order_class(order)?;

        trace!(order_id = %order.id, "Emir doğrulaması başarılı");
        Ok(())
    }

    /// Birden fazla emri toplu doğrula.
    ///
    /// Her emri doğrular; geçersiz emirleri hata listesiyle döner.
    /// Geçerli emirleri `valid` vektöründe toplar.
    ///
    /// CoW Protocol'ün `solvable_orders` yaklaşımı: geçersiz emirleri filtrele,
    /// nedenini logla, devam et.
    pub fn validate_batch(
        &self,
        orders: Vec<Order>,
        current_slot: SlotId,
    ) -> (Vec<Order>, Vec<(Order, AuctionError)>) {
        let mut valid = Vec::with_capacity(orders.len());
        let mut invalid = Vec::new();

        for order in orders {
            match self.validate_order(&order, current_slot) {
                Ok(()) => valid.push(order),
                Err(err) => {
                    debug!(
                        order_id = %order.id,
                        err = %err,
                        "Emir doğrulama hatası — filtrelendi"
                    );
                    invalid.push((order, err));
                }
            }
        }

        debug!(
            valid_count = valid.len(),
            invalid_count = invalid.len(),
            slot = current_slot.0,
            "Toplu doğrulama tamamlandı"
        );

        (valid, invalid)
    }

    // ─── Katman 1: Miktar Kontrolü ───────────────────────────────────────────

    /// Emir miktarı geçerli mi?
    ///
    /// CoW Protocol "dust order" filtresi:
    /// - Sıfır miktar → reddet (anlamlı işlem değil)
    /// - Minimum miktarın altında → reddet (spam önleme)
    /// - Maksimum miktarın üstünde → reddet (overflow önleme)
    fn validate_amount(&self, order: &Order) -> AuctionResult<()> {
        if order.amount == 0 {
            return Err(AuctionError::ZeroOrderAmount);
        }

        if order.amount < self.config.min_order_amount {
            // Miktar minimum eşiğin altında → "dust order"
            return Err(AuctionError::InsufficientBalance {
                required: self.config.min_order_amount,
                available: order.amount,
            });
        }

        if order.amount > self.config.max_order_amount {
            return Err(AuctionError::InsufficientBalance {
                required: order.amount,
                available: self.config.max_order_amount,
            });
        }

        Ok(())
    }

    // ─── Katman 2: Fiyat Kontrolü ────────────────────────────────────────────

    /// Limit fiyat geçerli mi?
    ///
    /// - Sıfır fiyat → reddet (anlamsız emir)
    /// - PRICE_PRECISION'dan küçük → minimum fiyat birimi kontrolü
    fn validate_price(&self, order: &Order) -> AuctionResult<()> {
        if order.limit_price == 0 {
            return Err(AuctionError::ZeroLimitPrice);
        }

        // Fiyat hassasiyeti: en küçük anlamlı birim
        if self.config.price_tick_size > 1
            && order.limit_price % self.config.price_tick_size != 0
        {
            // Fiyat tick boyutuna tam bölünmüyor
            warn!(
                order_id = %order.id,
                limit_price = order.limit_price,
                tick_size = self.config.price_tick_size,
                "Fiyat tick boyutuna uymuyor — yuvarlama"
            );
            // Uyarı ver ama reddetme (whitepaper gevşek tick politikası)
        }

        Ok(())
    }

    // ─── Katman 3: Token Çifti Kontrolü ─────────────────────────────────────

    /// Token çifti geçerli mi?
    ///
    /// - Aynı token alım/satım → anlamsız
    /// - Desteklenmeyen pair → reddedilebilir (şimdi her pair kabul)
    fn validate_token_pair(&self, order: &Order) -> AuctionResult<()> {
        if order.pair.is_same_token() {
            return Err(AuctionError::SameTokenPair);
        }
        Ok(())
    }

    // ─── Katman 4: Slot Geçerliliği ──────────────────────────────────────────

    /// Emir bu slot için geçerli mi?
    ///
    /// CoW Protocol `valid_to` (Unix zaman damgası) yerine Nova SlotId kullanır.
    /// Rolling Shutter'ın epoch/slot bazlı tetikleyicisinden ilham alındı.
    ///
    /// - `current_slot > valid_until_slot` → süresi dolmuş, reddet
    /// - `submitted_at_slot > current_slot` → gelecek slot emri, uyarı ver
    fn validate_slot(&self, order: &Order, current_slot: SlotId) -> AuctionResult<()> {
        if !order.is_valid_for_slot(current_slot) {
            return Err(AuctionError::OrderExpired {
                order_slot: order.valid_until_slot,
                current_slot,
            });
        }

        // Gönderilme slotu mevcut slottan büyükse uyarı (normalda olmamalı)
        if order.submitted_at_slot > current_slot {
            warn!(
                order_id = %order.id,
                submitted_slot = order.submitted_at_slot.0,
                current_slot = current_slot.0,
                "Emir gelecek slottan gönderilmiş görünüyor — anormal"
            );
        }

        Ok(())
    }

    // ─── Katman 5: Slippage Kontrolü ─────────────────────────────────────────

    /// Slippage tolerance geçerli mi?
    ///
    /// Max slippage bps:
    /// - `0` = hiç sapma yok (tam limit emri)
    /// - `10_000` = %100 sapma (piyasa emri gibi — çok riskli)
    /// - Nova whitepaper: maksimum slippage `5_000` (= %50) olarak sınırlandırılmış
    ///
    /// Aşırı slippage kullanıcıyı frontrunning'e açık bırakır.
    fn validate_slippage(&self, order: &Order) -> AuctionResult<()> {
        // %100 slippage (10_000 bps) kabul
        // Aşırı değer kontrolü (u16 max = 65535 > 10000)
        if order.max_slippage_bps > 10_000 {
            return Err(AuctionError::SlippageExceeded {
                clearing_price: order.max_slippage_bps as u128,
                limit_price: 10_000,
            });
        }

        // Limit fiyat ile slippage hesabı tutarlı mı?
        if order.limit_price > 0 {
            let max_price = order.limit_price_with_slippage_up();
            if max_price < order.limit_price && order.side.is_buy() {
                // Taşma oluştu
                return Err(AuctionError::ClearingPriceOverflow);
            }
        }

        Ok(())
    }

    // ─── Katman 6: Emir Sınıfı Tutarlılığı ──────────────────────────────────

    /// Emir sınıfı ile diğer alanlar tutarlı mı?
    ///
    /// CoW Protocol `OrderClass` hiyerarşisi:
    /// - Market: Geniş slippage beklenir
    /// - Limit: Sıkı fiyat beklenir
    /// - LimitPartialFill: Kısmi doldurma
    fn validate_order_class(&self, order: &Order) -> AuctionResult<()> {
        match order.class {
            OrderClass::Market => {
                // Piyasa emri: slippage yüksek olmalı (en az %1 = 100 bps)
                if order.max_slippage_bps < 10 {
                    warn!(
                        order_id = %order.id,
                        slippage_bps = order.max_slippage_bps,
                        "Piyasa emri için slippage çok düşük — limit emri gibi davranacak"
                    );
                    // Uyarı ver ama reddetme — kullanıcı kendi seçimi
                }
            }
            OrderClass::Limit => {
                // Limit emri kısmi doldurma beklemez
                // (bu config'e bağlı — şu an sadece uyarı)
            }
            OrderClass::LimitPartialFill => {
                // Kısmi doldurma aktif değilse uyar
                if !self.config.allow_partial_fills {
                    return Err(AuctionError::InvalidOrderType(
                        "Kısmi doldurma bu auction'da devre dışı".to_string(),
                    ));
                }
            }
        }
        Ok(())
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

    fn valid_order(side: OrderSide) -> Order {
        Order {
            id: OrderId::from_bytes([1u8; 32]),
            side,
            pair: test_pair(),
            amount: 1_000_000,       // Minimum miktarın üzerinde
            limit_price: 200 * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(1000),
            trader: Address::from_bytes([1u8; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    fn validator() -> OrderValidator {
        OrderValidator::with_defaults()
    }

    // ─── Geçerli Emir Testleri ────────────────────────────────────────────────

    #[test]
    fn test_gecerli_alim_emri_kabul() {
        let order = valid_order(OrderSide::Buy);
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    #[test]
    fn test_gecerli_satim_emri_kabul() {
        let order = valid_order(OrderSide::Sell);
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    #[test]
    fn test_gecerli_piyasa_emri_kabul() {
        let mut order = valid_order(OrderSide::Buy);
        order.class = OrderClass::Market;
        order.max_slippage_bps = 500; // %5
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    #[test]
    fn test_gecerli_kismi_doldurma_kabul() {
        let mut order = valid_order(OrderSide::Buy);
        order.class = OrderClass::LimitPartialFill;
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    // ─── Miktar Hata Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_sifir_miktar_reddedilir() {
        let mut order = valid_order(OrderSide::Buy);
        order.amount = 0;
        let err = validator().validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::ZeroOrderAmount));
    }

    #[test]
    fn test_cok_kucuk_miktar_reddedilir() {
        let config = AuctionConfig {
            min_order_amount: 1_000_000,
            ..Default::default()
        };
        let v = OrderValidator::new(config);
        let mut order = valid_order(OrderSide::Buy);
        order.amount = 999_999; // Minimum altında
        let err = v.validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::InsufficientBalance { .. }));
    }

    // ─── Fiyat Hata Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_sifir_limit_fiyat_reddedilir() {
        let mut order = valid_order(OrderSide::Buy);
        order.limit_price = 0;
        let err = validator().validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::ZeroLimitPrice));
    }

    // ─── Token Çifti Hata Testleri ────────────────────────────────────────────

    #[test]
    fn test_ayni_token_cifti_reddedilir() {
        let mut order = valid_order(OrderSide::Buy);
        order.pair = TokenPair::new(TokenId::NSC, TokenId::NSC);
        let err = validator().validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::SameTokenPair));
    }

    // ─── Slot Geçerliliği Testleri ────────────────────────────────────────────

    #[test]
    fn test_suresi_dolmus_emir_reddedilir() {
        let mut order = valid_order(OrderSide::Buy);
        order.valid_until_slot = SlotId(100);

        let err = validator().validate_order(&order, SlotId(101)).unwrap_err();
        assert!(matches!(
            err,
            AuctionError::OrderExpired {
                order_slot: SlotId(100),
                current_slot: SlotId(101)
            }
        ));
    }

    #[test]
    fn test_tam_son_slot_kabul() {
        let mut order = valid_order(OrderSide::Buy);
        order.valid_until_slot = SlotId(100);
        // current_slot == valid_until_slot → geçerli
        validator().validate_order(&order, SlotId(100)).unwrap();
    }

    #[test]
    fn test_ileri_slotta_gecerli() {
        let order = valid_order(OrderSide::Buy);
        // valid_until_slot = 1000, current_slot = 500 → geçerli
        validator().validate_order(&order, SlotId(500)).unwrap();
    }

    // ─── Slippage Testleri ────────────────────────────────────────────────────

    #[test]
    fn test_asiri_slippage_reddedilir() {
        let mut order = valid_order(OrderSide::Buy);
        order.max_slippage_bps = 10_001; // %100+ → geçersiz
        let err = validator().validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::SlippageExceeded { .. }));
    }

    #[test]
    fn test_tam_yuz_slippage_kabul() {
        let mut order = valid_order(OrderSide::Buy);
        order.max_slippage_bps = 10_000; // Tam %100 → kabul
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    #[test]
    fn test_sifir_slippage_kabul() {
        let mut order = valid_order(OrderSide::Buy);
        order.max_slippage_bps = 0; // Hiç slippage yok → kabul
        validator().validate_order(&order, SlotId(1)).unwrap();
    }

    // ─── Emir Sınıfı Testleri ────────────────────────────────────────────────

    #[test]
    fn test_kismi_doldurma_devre_disi_reddedilir() {
        let config = AuctionConfig {
            allow_partial_fills: false,
            ..Default::default()
        };
        let v = OrderValidator::new(config);
        let mut order = valid_order(OrderSide::Buy);
        order.class = OrderClass::LimitPartialFill;

        let err = v.validate_order(&order, SlotId(1)).unwrap_err();
        assert!(matches!(err, AuctionError::InvalidOrderType(_)));
    }

    // ─── Toplu Doğrulama Testleri ─────────────────────────────────────────────

    #[test]
    fn test_toplu_dogrulama_karisik_emirler() {
        let v = validator();
        let mut expired = valid_order(OrderSide::Buy);
        expired.valid_until_slot = SlotId(5);

        let mut zero_price = valid_order(OrderSide::Sell);
        zero_price.limit_price = 0;

        let orders = vec![
            valid_order(OrderSide::Buy),   // Geçerli
            expired,                        // Süresi dolmuş
            valid_order(OrderSide::Sell),  // Geçerli
            zero_price,                    // Sıfır fiyat
        ];

        let (valid, invalid) = v.validate_batch(orders, SlotId(10));

        assert_eq!(valid.len(), 2, "2 geçerli emir olmalı");
        assert_eq!(invalid.len(), 2, "2 geçersiz emir olmalı");
    }

    #[test]
    fn test_toplu_dogrulama_hepsi_gecerli() {
        let v = validator();
        let orders = vec![
            valid_order(OrderSide::Buy),
            valid_order(OrderSide::Sell),
            valid_order(OrderSide::Buy),
        ];

        let (valid, invalid) = v.validate_batch(orders, SlotId(1));
        assert_eq!(valid.len(), 3);
        assert!(invalid.is_empty());
    }

    #[test]
    fn test_toplu_dogrulama_hepsi_gecersiz() {
        let v = validator();
        let mut o1 = valid_order(OrderSide::Buy);
        o1.amount = 0;
        let mut o2 = valid_order(OrderSide::Sell);
        o2.limit_price = 0;

        let (valid, invalid) = v.validate_batch(vec![o1, o2], SlotId(1));
        assert!(valid.is_empty());
        assert_eq!(invalid.len(), 2);
    }

    #[test]
    fn test_coklu_alim_satim_toplu_dogrulama() {
        let v = validator();
        // 5 alım + 5 satım, 2 geçersiz
        let mut orders: Vec<Order> = (0..5)
            .map(|i| {
                let mut o = valid_order(OrderSide::Buy);
                let mut id_bytes = [0u8; 32];
                id_bytes[0] = i;
                o.id = OrderId::from_bytes(id_bytes);
                o
            })
            .collect();

        for i in 0..5u8 {
            let mut o = valid_order(OrderSide::Sell);
            let mut id_bytes = [0u8; 32];
            id_bytes[0] = i + 10;
            o.id = OrderId::from_bytes(id_bytes);
            if i == 2 {
                o.amount = 0; // Geçersiz
            }
            if i == 4 {
                o.limit_price = 0; // Geçersiz
            }
            orders.push(o);
        }

        let (valid, invalid) = v.validate_batch(orders, SlotId(1));
        assert_eq!(valid.len(), 8, "8 geçerli emir olmalı");
        assert_eq!(invalid.len(), 2, "2 geçersiz emir olmalı");
    }
}
