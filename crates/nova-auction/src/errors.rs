//! nova-auction hata tipleri.
//!
//! §5.3 — Sealed-Bid Batch Auction tüm hataları bu modülde tanımlanır.
//! CoW Protocol hata hiyerarşisinden ilham alınmıştır: her hata türü
//! hangi aşamadan geldiğini açıkça belirtir.

use nova_primitives::types::SlotId;
use thiserror::Error;

/// Auction operasyonu için sonuç tipi.
pub type AuctionResult<T> = Result<T, AuctionError>;

/// Nova Shield Chain Batch Auction hataları. §5.3
#[derive(Debug, Error)]
pub enum AuctionError {
    // ─── Emir (Order) Hataları ────────────────────────────────────────────────

    /// Emir miktarı sıfır veya negatif. §5.3.2
    #[error("Geçersiz emir miktarı: miktar sıfır veya negatif olamaz")]
    ZeroOrderAmount,

    /// Limit fiyat sıfır. §5.3.2
    #[error("Geçersiz limit fiyat: sıfır olamaz")]
    ZeroLimitPrice,

    /// Emir süresi dolmuş. §5.3.3
    #[error("Emir süresi dolmuş: slot={order_slot} mevcut={current_slot}")]
    OrderExpired {
        /// Emrin hedef slotu.
        order_slot: SlotId,
        /// Mevcut slot.
        current_slot: SlotId,
    },

    /// Emir zaten bu batch'te işlendi.
    #[error("Emir zaten işlendi: order_id={order_id}")]
    OrderAlreadyFilled {
        /// Emir kimliği.
        order_id: String,
    },

    /// Yinelenen emir (aynı commitment hash).
    #[error("Yinelenen emir: order_id={order_id}")]
    DuplicateOrder {
        /// Emir kimliği.
        order_id: String,
    },

    /// Emir geçersiz tip.
    #[error("Geçersiz emir tipi: {0}")]
    InvalidOrderType(String),

    /// Maksimum slippage tolerance aşıldı. §5.3.4
    #[error(
        "Slippage tolerance aşıldı: clearing_price={clearing_price} limit_price={limit_price}"
    )]
    SlippageExceeded {
        /// Hesaplanan clearing price.
        clearing_price: u128,
        /// Kullanıcının belirlediği limit fiyat.
        limit_price: u128,
    },

    /// Emir gönderici bakiyesi yetersiz.
    #[error("Yetersiz bakiye: gerekli={required} mevcut={available}")]
    InsufficientBalance {
        /// Gerekli tutar.
        required: u128,
        /// Mevcut bakiye.
        available: u128,
    },

    // ─── Clearing (Fiyat Belirleme) Hataları ─────────────────────────────────

    /// Alım ve satım emirleri fiyat aralığında kesişmiyor. §5.3.2
    #[error("Fiyat kesişimi yok: en yüksek alım={best_bid} en düşük satım={best_ask}")]
    NoPriceIntersection {
        /// En yüksek alım fiyatı.
        best_bid: u128,
        /// En düşük satım fiyatı.
        best_ask: u128,
    },

    /// Batch boş — hiç emir yok. §5.3.1
    #[error("Boş batch: auction slot={slot} — işlenecek emir bulunamadı")]
    EmptyBatch {
        /// Slot numarası.
        slot: SlotId,
    },

    /// Tek taraflı batch — sadece alım veya sadece satım var.
    #[error("Tek taraflı batch: alım_sayısı={buy_count} satım_sayısı={sell_count}")]
    UnilateralBatch {
        /// Alım emri sayısı.
        buy_count: usize,
        /// Satım emri sayısı.
        sell_count: usize,
    },

    /// Clearing price hesaplaması taştı.
    #[error("Clearing price hesaplama hatası: taşma (overflow)")]
    ClearingPriceOverflow,

    /// Clearing price hesaplama iç hatası.
    #[error("Clearing price iç hatası: {0}")]
    ClearingPriceInternal(String),

    // ─── Emir Kitabı (OrderBook) Hataları ────────────────────────────────────

    /// Emir kitabı doldu. §5.9.1
    #[error("Emir kitabı dolu: {current}/{maximum} emir")]
    OrderBookFull {
        /// Mevcut emir sayısı.
        current: usize,
        /// Maksimum kapasite.
        maximum: usize,
    },

    /// Slot için emir kitabı zaten kapatıldı.
    #[error("Slot {slot} emir kitabı kapalı — yeni emir kabul edilmiyor")]
    OrderBookClosed {
        /// Slot numarası.
        slot: SlotId,
    },

    // ─── Execution (Yürütme) Hataları ────────────────────────────────────────

    /// Batch yürütme sırasında token transferi başarısız.
    #[error("Token transferi başarısız: {reason}")]
    TransferFailed {
        /// Hata açıklaması.
        reason: String,
    },

    /// Kısmi doldurma bir sonraki slota taşınırken hata.
    #[error("Kısmi doldurma taşıma hatası slot={next_slot}: {reason}")]
    PartialFillCarryoverFailed {
        /// Bir sonraki slot.
        next_slot: SlotId,
        /// Hata açıklaması.
        reason: String,
    },

    /// Settlement hesaplama hatası.
    #[error("Settlement hesaplama hatası: {0}")]
    SettlementError(String),

    /// Zaten execute edilmiş batch tekrar çalıştırılmaya çalışıldı.
    #[error("Batch zaten execute edildi: slot={slot}")]
    BatchAlreadyExecuted {
        /// Slot numarası.
        slot: SlotId,
    },

    // ─── Token Çifti (TokenPair) Hataları ────────────────────────────────────

    /// Alım ve satım aynı token için gerçekleştirilmeye çalışıldı.
    #[error("Alım ve satım tokeni aynı olamaz")]
    SameTokenPair,

    /// Desteklenmeyen token çifti.
    #[error("Desteklenmeyen token çifti: {token_a}/{token_b}")]
    UnsupportedTokenPair {
        /// Token A.
        token_a: String,
        /// Token B.
        token_b: String,
    },

    // ─── Genel Hatalar ────────────────────────────────────────────────────────

    /// Serializasyon hatası.
    #[error("Serializasyon hatası: {0}")]
    SerializationError(String),

    /// Dahili hesaplama hatası.
    #[error("Dahili hata: {0}")]
    Internal(String),
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sifir_miktar_hatasi_mesaji() {
        let hata = AuctionError::ZeroOrderAmount;
        let mesaj = hata.to_string();
        assert!(mesaj.contains("sıfır"), "hata mesajı anlamlı olmalı");
    }

    #[test]
    fn test_fiyat_kesisimi_yok_mesaji() {
        let hata = AuctionError::NoPriceIntersection {
            best_bid: 100,
            best_ask: 200,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("100"), "en yüksek alım fiyatı mesajda olmalı");
        assert!(mesaj.contains("200"), "en düşük satım fiyatı mesajda olmalı");
    }

    #[test]
    fn test_slippage_exceeded_mesaji() {
        let hata = AuctionError::SlippageExceeded {
            clearing_price: 105,
            limit_price: 100,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("105"));
        assert!(mesaj.contains("100"));
    }

    #[test]
    fn test_bos_batch_mesaji() {
        let hata = AuctionError::EmptyBatch { slot: SlotId(42) };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("42"), "slot numarası mesajda olmalı");
    }

    #[test]
    fn test_emir_kitabi_dolu_mesaji() {
        let hata = AuctionError::OrderBookFull {
            current: 10_000,
            maximum: 10_000,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("10000"));
    }

    #[test]
    fn test_yetersiz_bakiye_mesaji() {
        let hata = AuctionError::InsufficientBalance {
            required: 1_000_000,
            available: 500_000,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("1000000"));
        assert!(mesaj.contains("500000"));
    }

    #[test]
    fn test_debug_impl() {
        let hata = AuctionError::ZeroLimitPrice;
        let debug_str = format!("{hata:?}");
        assert!(!debug_str.is_empty());
    }

    #[test]
    fn test_emir_suresi_dolmus_mesaji() {
        let hata = AuctionError::OrderExpired {
            order_slot: SlotId(10),
            current_slot: SlotId(20),
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("10"));
        assert!(mesaj.contains("20"));
    }

    #[test]
    fn test_tek_tarafli_batch_mesaji() {
        let hata = AuctionError::UnilateralBatch {
            buy_count: 5,
            sell_count: 0,
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("5"));
        assert!(mesaj.contains("0"));
    }

    #[test]
    fn test_ayni_token_hata_mesaji() {
        let hata = AuctionError::SameTokenPair;
        let mesaj = hata.to_string();
        assert!(!mesaj.is_empty());
    }

    #[test]
    fn test_batch_zaten_execute_mesaji() {
        let hata = AuctionError::BatchAlreadyExecuted { slot: SlotId(999) };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("999"));
    }

    #[test]
    fn test_desteklenmeyen_token_cifti() {
        let hata = AuctionError::UnsupportedTokenPair {
            token_a: "NOVA".into(),
            token_b: "XYZ".into(),
        };
        let mesaj = hata.to_string();
        assert!(mesaj.contains("NOVA"));
        assert!(mesaj.contains("XYZ"));
    }
}
