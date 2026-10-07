//! GÖREV 87: Dutch Auction DEX (Penumbra pattern)
//!
//! ## Mimari
//!
//! ### Penumbra Dutch Auction
//! - Satıcı başlangıç fiyatı P_start'tan P_end'e kadar düşürür
//! - Alıcı dilediği zaman mevcut fiyattan satın alabilir
//! - Belirli slot'ta sona erer → kalan miktarı iade eder
//!
//! ### Nova Dutch Auction Tasarımı
//! - `DutchAuction`: tek satış, fiyat lineer düşer
//! - `DutchAuctionBid`: belirli slotta mevcut fiyattan satın al
//! - `DutchAuctionEngine`: çok sayıda aktif açık artırmayı yönetir
//! - Partial fill: alıcı istediği miktarı alabilir (tüm lot değil)

use std::collections::BTreeMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Minimum Dutch auction süresi (slot)
pub const MIN_AUCTION_DURATION_SLOTS: u64 = 10;

/// Maksimum aktif açık artırma sayısı
pub const MAX_ACTIVE_AUCTIONS: usize = 1000;

// ─── DutchAuctionId ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DutchAuctionId(pub u64);

// ─── DutchAuction ─────────────────────────────────────────────────────────────

/// Bir Dutch (azalan fiyatlı) açık artırma
///
/// Penumbra pattern: `dutch_auction.proto`
/// - start_height: açılış slotu
/// - end_height: kapanış slotu
/// - output_total: satışa çıkarılan toplam miktar
/// - reserve_price: alt taban fiyat (P_end)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DutchAuction {
    pub id: DutchAuctionId,
    /// Açık artırmayı başlatan (satıcı)
    pub seller: [u8; 20],
    /// Satılan token ID (ör. 1 = NSC)
    pub input_token: u64,
    /// Alınan token ID (ör. 2 = USDC)
    pub output_token: u64,
    /// Toplam satılan miktar (en küçük birim)
    pub input_amount: u128,
    /// Kalan satılmamış miktar
    pub remaining_input: u128,
    /// Başlangıç fiyatı (output/input oranı, PRICE_PRECISION=1_000_000 tabanlı)
    pub start_price: u128,
    /// Bitiş fiyatı (rezerv / taban)
    pub end_price: u128,
    /// Başlangıç slotu
    pub start_slot: u64,
    /// Bitiş slotu
    pub end_slot: u64,
    /// Toplam toplanan output (alınan token)
    pub collected_output: u128,
    /// Durum
    pub state: DutchAuctionState,
}

/// PRICE_PRECISION: fiyat hassasiyeti (6 ondalık = 1_000_000 birim)
pub const DUTCH_PRICE_PRECISION: u128 = 1_000_000;

impl DutchAuction {
    pub fn new(
        id: DutchAuctionId,
        seller: [u8; 20],
        input_token: u64,
        output_token: u64,
        input_amount: u128,
        start_price: u128,
        end_price: u128,
        start_slot: u64,
        end_slot: u64,
    ) -> Result<Self, DutchAuctionError> {
        if start_price < end_price {
            return Err(DutchAuctionError::InvalidPriceRange { start_price, end_price });
        }
        if end_slot <= start_slot {
            return Err(DutchAuctionError::InvalidDuration);
        }
        if end_slot - start_slot < MIN_AUCTION_DURATION_SLOTS {
            return Err(DutchAuctionError::DurationTooShort);
        }
        if input_amount == 0 {
            return Err(DutchAuctionError::ZeroAmount);
        }
        Ok(Self {
            id,
            seller,
            input_token,
            output_token,
            input_amount,
            remaining_input: input_amount,
            start_price,
            end_price,
            start_slot,
            end_slot,
            collected_output: 0,
            state: DutchAuctionState::Active,
        })
    }

    /// Mevcut slot için lineer fiyat hesapla
    ///
    /// Penumbra: `price(t) = start_price + (end_price - start_price) * (t - start) / duration`
    /// Düşen fiyat: P_start → P_end
    pub fn current_price(&self, current_slot: u64) -> u128 {
        if current_slot <= self.start_slot {
            return self.start_price;
        }
        if current_slot >= self.end_slot {
            return self.end_price;
        }
        let elapsed = current_slot - self.start_slot;
        let duration = self.end_slot - self.start_slot;
        // start_price'tan end_price'a lineer düşüş
        let price_range = self.start_price.saturating_sub(self.end_price);
        let price_drop = price_range
            .saturating_mul(elapsed as u128)
            / duration as u128;
        self.start_price.saturating_sub(price_drop)
    }

    /// Belirli miktarda satın al
    pub fn buy(&mut self, amount: u128, current_slot: u64) -> Result<DutchBidResult, DutchAuctionError> {
        if self.state != DutchAuctionState::Active {
            return Err(DutchAuctionError::NotActive);
        }
        if current_slot < self.start_slot || current_slot > self.end_slot {
            return Err(DutchAuctionError::NotActive);
        }
        if amount == 0 {
            return Err(DutchAuctionError::ZeroAmount);
        }

        let available = self.remaining_input;
        let filled_amount = amount.min(available);
        let price = self.current_price(current_slot);

        // Output miktarı: filled_amount × price / PRECISION
        let output_cost = filled_amount
            .saturating_mul(price)
            / DUTCH_PRICE_PRECISION;

        self.remaining_input -= filled_amount;
        self.collected_output += output_cost;

        if self.remaining_input == 0 {
            self.state = DutchAuctionState::Filled;
        }

        Ok(DutchBidResult {
            auction_id: self.id,
            filled_input: filled_amount,
            output_cost,
            price_at_fill: price,
            slot: current_slot,
            partial_fill: filled_amount < amount,
        })
    }

    /// Açık artırmayı kapat (süre doldu veya satıcı kapattı)
    pub fn close(&mut self, current_slot: u64) -> DutchCloseResult {
        let refund = self.remaining_input;
        self.remaining_input = 0;
        if self.state == DutchAuctionState::Active {
            self.state = if refund > 0 {
                DutchAuctionState::ClosedPartial
            } else {
                DutchAuctionState::Filled
            };
        }
        DutchCloseResult {
            auction_id: self.id,
            seller: self.seller,
            refunded_input: refund,
            collected_output: self.collected_output,
            closed_at_slot: current_slot,
        }
    }

    pub fn is_expired(&self, current_slot: u64) -> bool {
        current_slot > self.end_slot
    }
}

/// Dutch Auction durumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DutchAuctionState {
    Active,
    Filled,
    ClosedPartial,
    Cancelled,
}

/// Teklif sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DutchBidResult {
    pub auction_id: DutchAuctionId,
    pub filled_input: u128,
    pub output_cost: u128,
    pub price_at_fill: u128,
    pub slot: u64,
    pub partial_fill: bool,
}

/// Kapanma sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DutchCloseResult {
    pub auction_id: DutchAuctionId,
    pub seller: [u8; 20],
    pub refunded_input: u128,
    pub collected_output: u128,
    pub closed_at_slot: u64,
}

// ─── DutchAuctionEngine ───────────────────────────────────────────────────────

/// Birden fazla Dutch Auction'ı yöneten motor
#[derive(Debug, Default)]
pub struct DutchAuctionEngine {
    auctions: BTreeMap<DutchAuctionId, DutchAuction>,
    next_id: u64,
}

impl DutchAuctionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni açık artırma oluştur
    pub fn create_auction(
        &mut self,
        seller: [u8; 20],
        input_token: u64,
        output_token: u64,
        input_amount: u128,
        start_price: u128,
        end_price: u128,
        start_slot: u64,
        end_slot: u64,
    ) -> Result<DutchAuctionId, DutchAuctionError> {
        if self.auctions.len() >= MAX_ACTIVE_AUCTIONS {
            return Err(DutchAuctionError::MaxAuctionsReached);
        }
        let id = DutchAuctionId(self.next_id);
        self.next_id += 1;
        let auction = DutchAuction::new(
            id, seller, input_token, output_token,
            input_amount, start_price, end_price, start_slot, end_slot,
        )?;
        self.auctions.insert(id, auction);
        Ok(id)
    }

    /// Belirli açık artırmada teklif ver
    pub fn bid(
        &mut self,
        auction_id: DutchAuctionId,
        amount: u128,
        current_slot: u64,
    ) -> Result<DutchBidResult, DutchAuctionError> {
        let auction = self.auctions.get_mut(&auction_id)
            .ok_or(DutchAuctionError::AuctionNotFound(auction_id))?;
        auction.buy(amount, current_slot)
    }

    /// Slot'u işle: süresi dolan açık artırmaları kapat
    pub fn process_slot(&mut self, current_slot: u64) -> Vec<DutchCloseResult> {
        let expired_ids: Vec<DutchAuctionId> = self.auctions.iter()
            .filter(|(_, a)| a.is_expired(current_slot) && a.state == DutchAuctionState::Active)
            .map(|(id, _)| *id)
            .collect();

        let mut results = Vec::new();
        for id in expired_ids {
            if let Some(auction) = self.auctions.get_mut(&id) {
                results.push(auction.close(current_slot));
            }
        }
        results
    }

    /// Aktif açık artırma sayısı
    pub fn active_count(&self) -> usize {
        self.auctions.values().filter(|a| a.state == DutchAuctionState::Active).count()
    }

    /// Açık artırma durumunu sorgula
    pub fn get(&self, id: DutchAuctionId) -> Option<&DutchAuction> {
        self.auctions.get(&id)
    }

    /// Satıcı kapatsın
    pub fn cancel(
        &mut self,
        auction_id: DutchAuctionId,
        seller: [u8; 20],
        current_slot: u64,
    ) -> Result<DutchCloseResult, DutchAuctionError> {
        let auction = self.auctions.get_mut(&auction_id)
            .ok_or(DutchAuctionError::AuctionNotFound(auction_id))?;
        if auction.seller != seller {
            return Err(DutchAuctionError::Unauthorized);
        }
        auction.state = DutchAuctionState::Cancelled;
        Ok(auction.close(current_slot))
    }
}

// ─── DutchAuctionError ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DutchAuctionError {
    InvalidPriceRange { start_price: u128, end_price: u128 },
    InvalidDuration,
    DurationTooShort,
    ZeroAmount,
    NotActive,
    AuctionNotFound(DutchAuctionId),
    MaxAuctionsReached,
    Unauthorized,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn seller() -> [u8; 20] {
        [0x01u8; 20]
    }

    fn make_auction(start_price: u128, end_price: u128, duration: u64) -> DutchAuction {
        DutchAuction::new(
            DutchAuctionId(1),
            seller(),
            1, 2, // NSC → USDC
            1_000_000, // 1M birim
            start_price,
            end_price,
            100, // start_slot
            100 + duration, // end_slot
        ).unwrap()
    }

    // 1. Fiyat başlangıçta max
    #[test]
    fn price_starts_at_maximum() {
        let auction = make_auction(2_000_000, 500_000, 100);
        assert_eq!(auction.current_price(100), 2_000_000);
        assert_eq!(auction.current_price(0), 2_000_000); // öncesinde de max
    }

    // 2. Fiyat sonda min
    #[test]
    fn price_ends_at_minimum() {
        let auction = make_auction(2_000_000, 500_000, 100);
        assert_eq!(auction.current_price(200), 500_000); // slot 200 = end_slot
        assert_eq!(auction.current_price(300), 500_000); // sonrasında da min
    }

    // 3. Fiyat lineer düşüş
    #[test]
    fn price_decreases_linearly() {
        let auction = make_auction(2_000_000, 0, 100);
        let p50 = auction.current_price(150); // %50 geçildi
        assert_eq!(p50, 1_000_000); // tam ortada
    }

    // 4. Başarılı satın alma
    #[test]
    fn successful_bid() {
        let mut auction = make_auction(1_000_000, 0, 100);
        let result = auction.buy(100_000, 100).unwrap();
        assert_eq!(result.filled_input, 100_000);
        assert_eq!(result.price_at_fill, 1_000_000);
        // output_cost = 100_000 × 1_000_000 / 1_000_000 = 100_000
        assert_eq!(result.output_cost, 100_000);
        assert_eq!(result.partial_fill, false);
    }

    // 5. Kısmi doldurma — istenen > mevcut
    #[test]
    fn partial_fill_when_insufficient() {
        let mut auction = make_auction(1_000_000, 0, 100);
        // 1_500_000 isteniyor ama 1_000_000 var → partial fill
        let result = auction.buy(1_500_000, 100).unwrap();
        assert_eq!(result.filled_input, 1_000_000); // sadece mevcut kadar
        assert!(result.partial_fill, "partial fill bekleniyor");
        assert_eq!(auction.remaining_input, 0);
        assert_eq!(auction.state, DutchAuctionState::Filled);

        // İkinci teklif → NotActive hatası
        let result2 = auction.buy(1, 101);
        assert_eq!(result2, Err(DutchAuctionError::NotActive));
    }

    // 6. Tam dolunca Filled durumuna geçer
    #[test]
    fn auction_fills_completely() {
        let mut auction = make_auction(1_000_000, 0, 100);
        auction.buy(1_000_000, 100).unwrap();
        assert_eq!(auction.state, DutchAuctionState::Filled);
    }

    // 7. Dolu açık artırmaya teklif → hata
    #[test]
    fn bid_on_filled_auction_fails() {
        let mut auction = make_auction(1_000_000, 0, 100);
        auction.buy(1_000_000, 100).unwrap();
        let result = auction.buy(1, 101);
        assert_eq!(result, Err(DutchAuctionError::NotActive));
    }

    // 8. Süresi dolmuş açık artırma kapanır
    #[test]
    fn expired_auction_closes_on_process() {
        let mut engine = DutchAuctionEngine::new();
        let id = engine.create_auction(
            seller(), 1, 2, 500_000, 2_000_000, 0, 0, 50
        ).unwrap();

        // Slot 51 → süresi doldu
        let closed = engine.process_slot(51);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].refunded_input, 500_000);
        assert_eq!(engine.active_count(), 0);
    }

    // 9. Geçersiz fiyat aralığı → hata
    #[test]
    fn invalid_price_range_rejected() {
        let result = DutchAuction::new(
            DutchAuctionId(1), seller(), 1, 2, 100,
            500_000, 2_000_000, // start < end → hata
            0, 100
        );
        assert_eq!(result, Err(DutchAuctionError::InvalidPriceRange { start_price: 500_000, end_price: 2_000_000 }));
    }

    // 10. Çok kısa süre → hata
    #[test]
    fn too_short_duration_rejected() {
        let result = DutchAuction::new(
            DutchAuctionId(1), seller(), 1, 2, 100,
            2_000_000, 0,
            0, 5 // sadece 5 slot
        );
        assert_eq!(result, Err(DutchAuctionError::DurationTooShort));
    }

    // 11. Engine: birden fazla açık artırma
    #[test]
    fn engine_manages_multiple_auctions() {
        let mut engine = DutchAuctionEngine::new();

        // 3 farklı açık artırma
        for i in 0..3u8 {
            let id = engine.create_auction(
                [i; 20], 1, 2, 100_000,
                2_000_000, 0, 0, 100
            ).unwrap();
            assert_eq!(id, DutchAuctionId(i as u64));
        }

        assert_eq!(engine.active_count(), 3);
    }

    // 12. Satıcı iptal edebilir
    #[test]
    fn seller_can_cancel() {
        let mut engine = DutchAuctionEngine::new();
        let id = engine.create_auction(
            seller(), 1, 2, 100_000, 2_000_000, 0, 0, 100
        ).unwrap();

        // Kısmen doldur
        engine.bid(id, 50_000, 50).unwrap();

        // İptal et
        let result = engine.cancel(id, seller(), 60).unwrap();
        assert_eq!(result.refunded_input, 50_000);
        assert_eq!(result.collected_output, 50_000 * 1_000_000 / 1_000_000);
    }

    // 13. Yetkisiz iptal → hata
    #[test]
    fn unauthorized_cancel_rejected() {
        let mut engine = DutchAuctionEngine::new();
        let id = engine.create_auction(
            seller(), 1, 2, 100_000, 2_000_000, 0, 0, 100
        ).unwrap();

        let other = [0x99u8; 20];
        let result = engine.cancel(id, other, 50);
        assert_eq!(result, Err(DutchAuctionError::Unauthorized));
    }

    // 14. current_price monoton azalıyor
    #[test]
    fn price_is_monotonically_decreasing() {
        let auction = make_auction(2_000_000, 100_000, 100);
        let mut prev_price = u128::MAX;
        for slot in (100..=200).step_by(5) {
            let price = auction.current_price(slot);
            assert!(price <= prev_price, "fiyat slot {slot}'da arttı: {prev_price} → {price}");
            prev_price = price;
        }
    }
}
