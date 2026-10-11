//! nova-auction temel veri tipleri.
//!
//! §5.3 — Sealed-Bid Batch Auction veri yapıları.
//!
//! ## Tasarım İlhamları
//!
//! - **CoW Protocol**: Uniform Price Auction, Order modeli, Partial Fill, Solution scoring
//! - **Rolling Shutter**: Slot-bazlı execution tetikleyicisi
//! - **Whitepaper §5.3.2**: Batch clearing price hesabı, alım/satım eğrisi kesişimi
//!
//! ## Temel Kavramlar
//!
//! ```text
//! ┌─ Slot kapandı (reveal fazı) ────────────────────────────────────────────┐
//! │  RevealedBatch (nova-mempool) → nova-auction                            │
//! │                                                                          │
//! │  Her TX içindeki order'lar decode edilir:                                │
//! │  RawTransaction.data → Order (alım/satım talebi + limit fiyat)          │
//! │                                                                          │
//! │  OrderBook inşa edilir:                                                  │
//! │  buy_orders:  [300@$210, 100@$205, 200@$200]  ← fiyata göre azalan     │
//! │  sell_orders: [80@$198,  120@$205, 75@$200]   ← fiyata göre artan      │
//! │                                                                          │
//! │  Clearing Price hesapla:                                                 │
//! │  Kümülatif arz/talep kesişimi → P_clear = $200                          │
//! │                                                                          │
//! │  Execute:                                                                 │
//! │  Alım >= $200 → execute at $200                                          │
//! │  Satım <= $200 → execute at $200                                         │
//! │  Sınırdaki emirler → partial fill                                        │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use borsh::{BorshDeserialize, BorshSerialize};
use nova_primitives::types::{Address, SlotId};
use serde::{Deserialize, Serialize};

// ─── TOKEN ───────────────────────────────────────────────────────────────────

/// Token tanımlayıcısı — 32 byte adres (native NSC için özel ZERO adresi).
///
/// §5.3 — Batch auction herhangi bir token çiftini destekler.
/// Native NSC token için özel `NSC_NATIVE_TOKEN_ID` sabit değeri kullanılır.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct TokenId(pub [u8; 20]);

impl TokenId {
    /// Native NSC token — transfer emrinde sell/buy token bu ise NSC harcandı/alındı.
    pub const NSC: Self = Self([0u8; 20]);

    /// Token ID'si oluştur.
    pub fn from_address(addr: Address) -> Self {
        Self(*addr.as_bytes())
    }

    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Native NSC token mi?
    pub fn is_native(&self) -> bool {
        *self == Self::NSC
    }
}

impl std::fmt::Display for TokenId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Token(0x{})", &self.to_hex()[..8])
    }
}

// ─── TOKEN ÇİFTİ ─────────────────────────────────────────────────────────────

/// Alım/satım token çifti. §5.3
///
/// `base`: Ticaret yapılan token (örn: wBTC)
/// `quote`: Fiyatlama tokeni (örn: NSC)
///
/// Clearing price `quote / base` cinsinden ifade edilir.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct TokenPair {
    /// İşlem yapılan token.
    pub base: TokenId,
    /// Fiyatlandırma tokeni.
    pub quote: TokenId,
}

impl TokenPair {
    /// Yeni token çifti oluştur.
    pub fn new(base: TokenId, quote: TokenId) -> Self {
        Self { base, quote }
    }

    /// NSC/herhangi-bir-token çifti.
    pub fn nsc_base(quote: TokenId) -> Self {
        Self { base: TokenId::NSC, quote }
    }

    /// Canonical form — base < quote (lexicographic) sırası.
    /// Market'ları normalize etmek için.
    pub fn canonical(mut self) -> Self {
        if self.base.0 > self.quote.0 {
            std::mem::swap(&mut self.base, &mut self.quote);
        }
        self
    }

    /// Aynı token mu? Geçersiz.
    pub fn is_same_token(&self) -> bool {
        self.base == self.quote
    }
}

impl std::fmt::Display for TokenPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

// ─── EMİR TİPİ ────────────────────────────────────────────────────────────────

/// Emir yönü. §5.3.2
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum OrderSide {
    /// Alım emri: `amount` kadar `base` token almak istiyorum,
    /// en fazla `limit_price * amount` kadar `quote` ödeyebilirim.
    Buy = 0,
    /// Satım emri: `amount` kadar `base` token satmak istiyorum,
    /// en az `limit_price * amount` kadar `quote` almak istiyorum.
    Sell = 1,
}

impl OrderSide {
    /// Alım emri mi?
    pub fn is_buy(&self) -> bool {
        matches!(self, Self::Buy)
    }

    /// Satım emri mi?
    pub fn is_sell(&self) -> bool {
        matches!(self, Self::Sell)
    }

    /// Ters taraf.
    pub fn opposite(&self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "ALIM"),
            Self::Sell => write!(f, "SATIM"),
        }
    }
}

// ─── EMİR SINIFI ─────────────────────────────────────────────────────────────

/// Emir sınıfı — nasıl işleneceğini belirler.
///
/// CoW Protocol'ün order class yapısından ilham alındı.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum OrderClass {
    /// Piyasa emri — mevcut en iyi fiyatta işlem yap.
    /// Limit price geniş tutulur, hızlı execute edilir.
    Market = 0,
    /// Limit emri — yalnızca limit fiyat sağlanırsa işlem yap.
    /// Sağlanamazsa bir sonraki slota taşı veya iptal et.
    Limit = 1,
    /// Kısmi doldurma izni olan limit emri.
    /// Kısmen eşleşirse kalan miktar sonraki slota taşınır.
    LimitPartialFill = 2,
}

impl std::fmt::Display for OrderClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Market => write!(f, "Piyasa"),
            Self::Limit => write!(f, "Limit"),
            Self::LimitPartialFill => write!(f, "LimitKısmi"),
        }
    }
}

// ─── EMİR KİMLİĞİ ─────────────────────────────────────────────────────────────

/// Emir benzersiz kimliği.
///
/// Kullanıcının TX hash'inden türetilir: ilgili DecryptedTx'in commitment hash'i.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct OrderId([u8; 32]);

impl OrderId {
    /// Ham byte'lardan oluştur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// TX commitment hash'inden oluştur.
    pub fn from_commitment(commitment: [u8; 32]) -> Self {
        Self(commitment)
    }

    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Sıfır ID (test yardımcısı).
    pub const ZERO: Self = Self([0u8; 32]);
}

impl std::fmt::Display for OrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Order(0x{}...)", &self.to_hex()[..16])
    }
}

// ─── ORDER UID ────────────────────────────────────────────────────────────────

/// Emir benzersiz kimliği — CoW Protocol `OrderUid([u8; 56])` pattern.
///
/// AŞAMA 7 Görev 49: `digest(32) || owner(20) || validTo(4)` = 56 byte.
///
/// Bu yapı orderlerin küresel tekliğini garantiler:
/// - `digest`: Emir içeriğinin hash'i (tekrarlı emir önleme)
/// - `owner`: Emri veren kullanıcının adresi
/// - `valid_to`: Emrin slot'a kadar geçerli olduğu son slot (u32 LE)
///
/// OrderId([u8;32]) ile farkı: OrderUid replay koruması sağlar.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
    BorshSerialize, BorshDeserialize,
)]
pub struct OrderUid(pub [u8; 56]);

// [u8; 56] serde tarafından desteklenmez (max 32) — manuel impl
impl serde::Serialize for OrderUid {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for OrderUid {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = OrderUid;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "56 byte OrderUid")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<OrderUid, E> {
                if v.len() != 56 {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut b = [0u8; 56];
                b.copy_from_slice(v);
                Ok(OrderUid(b))
            }
        }
        d.deserialize_bytes(V)
    }
}

impl OrderUid {
    /// digest(32) + owner(20) + valid_to_slot(4) → OrderUid
    pub fn new(digest: [u8; 32], owner: Address, valid_to_slot: u32) -> Self {
        let mut bytes = [0u8; 56];
        bytes[..32].copy_from_slice(&digest);
        bytes[32..52].copy_from_slice(owner.as_bytes());
        bytes[52..56].copy_from_slice(&valid_to_slot.to_le_bytes());
        Self(bytes)
    }

    /// Hash (digest) kısmı.
    pub fn digest(&self) -> &[u8; 32] {
        self.0[..32].try_into().unwrap()
    }

    /// Sahip adresi.
    pub fn owner(&self) -> Address {
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&self.0[32..52]);
        Address::from_bytes(bytes)
    }

    /// Son geçerli slot numarası.
    pub fn valid_to_slot(&self) -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.0[52..56]);
        u32::from_le_bytes(b)
    }

    /// Slot'ta hâlâ geçerli mi?
    pub fn is_valid_at(&self, current_slot: u32) -> bool {
        current_slot <= self.valid_to_slot()
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 56] {
        &self.0
    }

    /// Hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// OrderId'ye dönüştür (digest kısmını kullan).
    pub fn to_order_id(&self) -> OrderId {
        OrderId::from_bytes(*self.digest())
    }
}

impl std::fmt::Display for OrderUid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrderUid(0x{}...{})", hex::encode(&self.0[..4]), hex::encode(&self.0[52..]))
    }
}

// ─── EMİR (ORDER) ─────────────────────────────────────────────────────────────

/// Batch auction emri. §5.3.2
///
/// Bir kullanıcının slot için verdiği alım veya satım talebi.
/// Bu yapı `RawTransaction.data`'dan decode edilir (nova-mempool açılma sonrası).
///
/// ## Uniform Price Auction Kuralı
///
/// Clearing price `P` belirlendiğinde:
/// - `Buy`  emri: `limit_price >= P` ise execute at `P` (limit fiyatından iyi fiyat aldı!)
/// - `Sell` emri: `limit_price <= P` ise execute at `P` (limit fiyatından iyi fiyat aldı!)
///
/// Bu "price improvement" kullanıcı lehinedir — MEV-free ve dürüst.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Order {
    /// Emir kimliği (TX commitment hash'inden türetilir).
    pub id: OrderId,

    /// Emir yönü (alım/satım).
    pub side: OrderSide,

    /// Token çifti.
    pub pair: TokenPair,

    /// İstenen miktar (`base` token cinsinden, 18 ondalık hassasiyetle).
    pub amount: u128,

    /// Limit fiyat (`quote` token / `base` token, 18 ondalık hassasiyetle).
    ///
    /// - `Buy`  emri için: Bu fiyatın altında satın al (daha iyi fiyatta kabul et)
    /// - `Sell` emri için: Bu fiyatın üstünde sat (daha iyi fiyatta kabul et)
    pub limit_price: u128,

    /// Maksimum kabul edilebilir fiyat sapması (binde).
    ///
    /// `0` = hiç sapma yok (tam limit fiyat)
    /// `10` = %1 sapma tolere edilir
    /// `1000` = %100 sapma (piyasa emri gibi)
    pub max_slippage_bps: u16,

    /// Emir sınıfı (Market/Limit/LimitPartialFill).
    pub class: OrderClass,

    /// Bu emir hangi slot için geçerli?
    /// Slot geçince emir iptal edilir.
    pub valid_until_slot: SlotId,

    /// Gönderici adresi.
    pub trader: Address,

    /// Hangi slotta gönderildi (commit fazı).
    pub submitted_at_slot: SlotId,

    /// Emir verisi (protokol spesifik ek parametreler).
    pub extra_data: Vec<u8>,
}

impl Order {
    /// Fiyat sınırını kontrol et: clearing price kabul edilebilir mi?
    ///
    /// `clearing_price`: 18 ondalık hassasiyetle fiyat
    pub fn price_acceptable(&self, clearing_price: u128) -> bool {
        match self.side {
            OrderSide::Buy => {
                // Alımda: clearing price <= limit_price + slippage olmalı
                let max_accepted = self.limit_price_with_slippage_up();
                clearing_price <= max_accepted
            }
            OrderSide::Sell => {
                // Satımda: clearing price >= limit_price - slippage olmalı
                let min_accepted = self.limit_price_with_slippage_down();
                clearing_price >= min_accepted
            }
        }
    }

    /// Alım emri için maksimum kabul edilebilir fiyat (slippage dahil).
    pub fn limit_price_with_slippage_up(&self) -> u128 {
        // limit_price * (10000 + slippage_bps) / 10000
        let bps = self.max_slippage_bps as u128;
        self.limit_price
            .saturating_mul(10_000 + bps)
            .saturating_div(10_000)
    }

    /// Satım emri için minimum kabul edilebilir fiyat (slippage dahil).
    pub fn limit_price_with_slippage_down(&self) -> u128 {
        // limit_price * (10000 - slippage_bps) / 10000
        let bps = self.max_slippage_bps as u128;
        let factor = 10_000u128.saturating_sub(bps);
        self.limit_price
            .saturating_mul(factor)
            .saturating_div(10_000)
    }

    /// Kısmi doldurma kabul ediyor mu?
    pub fn allows_partial_fill(&self) -> bool {
        matches!(self.class, OrderClass::LimitPartialFill)
    }

    /// Emir geçerli mi (slot kontrolü)?
    pub fn is_valid_for_slot(&self, current_slot: SlotId) -> bool {
        current_slot <= self.valid_until_slot
    }

    /// Bu emrin tükettiği `quote` token miktarı (tam doldurma için).
    pub fn total_quote_cost(&self) -> Option<u128> {
        // amount (base) * limit_price (quote/base) / PRICE_PRECISION
        self.amount
            .checked_mul(self.limit_price)
            .map(|v| v / PRICE_PRECISION)
    }
}

/// Fiyat hassasiyet faktörü (10^18).
///
/// Tüm fiyatlar bu çarpanla normalleştirilir.
/// 1 NSC = `PRICE_PRECISION` base unit.
pub const PRICE_PRECISION: u128 = 1_000_000_000_000_000_000u128; // 10^18

// ─── EMİR DURUMU ─────────────────────────────────────────────────────────────

/// Emir çalıştırma durumu. §5.3
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Beklemede — clearing bekleniyor.
    Pending,
    /// Tam dolduruldu.
    Filled,
    /// Kısmen dolduruldu — kalan miktar bir sonraki slota taşındı.
    PartiallyFilled {
        /// Doldurulmuş miktar.
        filled_amount: u128,
        /// Kalan miktar (sonraki slota taşındı).
        remaining_amount: u128,
    },
    /// İptal edildi — limit fiyat sağlanamadı veya süre doldu.
    Cancelled {
        /// İptal nedeni.
        reason: CancelReason,
    },
    /// Çalıştırma başarısız.
    Failed {
        /// Hata açıklaması.
        reason: String,
    },
}

/// Emir iptal nedeni.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancelReason {
    /// Limit fiyat sağlanamadı (clearing price dışında).
    PriceNotMet,
    /// Emir süresi doldu.
    Expired,
    /// Bakiye yetersiz.
    InsufficientBalance,
    /// Slippage tolerance aşıldı.
    SlippageExceeded,
}

// ─── ÇALIŞTIRILMIŞ EMİR ───────────────────────────────────────────────────────

/// Bir emrin clearing sonucu. §5.3.2
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ExecutedOrder {
    /// Orijinal emir kimliği.
    pub order_id: OrderId,

    /// Gönderici adresi.
    pub trader: Address,

    /// Emir yönü.
    pub side: OrderSide,

    /// Token çifti.
    pub pair: TokenPair,

    /// İstenen miktar.
    pub requested_amount: u128,

    /// Fiilen çalıştırılan miktar.
    pub executed_amount: u128,

    /// Clearing price.
    pub clearing_price: u128,

    /// Ödenecek/alınacak `quote` token miktarı.
    pub quote_amount: u128,

    /// Price improvement: `limit_price` ile `clearing_price` arasındaki fark.
    ///
    /// Kullanıcının lehine olan bu fark MEV-free yapının temel çıktısıdır.
    pub price_improvement: i128,

    /// Kısmen mi dolduruldu?
    pub is_partial: bool,

    /// Kalan miktar (kısmi doldurma durumunda).
    pub remaining_amount: u128,

    /// Batch sırası.
    pub execution_order: u32,
}

impl ExecutedOrder {
    /// Price improvement yüzdesi (binde).
    pub fn price_improvement_bps(&self) -> i128 {
        if self.clearing_price == 0 {
            return 0;
        }
        (self.price_improvement * 10_000) / self.clearing_price as i128
    }

    /// Tam doldurma mu?
    pub fn is_fully_filled(&self) -> bool {
        !self.is_partial && self.remaining_amount == 0
    }
}

// ─── CLEARING SONUCU ──────────────────────────────────────────────────────────

/// Bir slot'un batch auction clearing sonucu. §5.3.2
///
/// CoW Protocol'ün `Solution` yapısından ilham alındı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearingResult {
    /// Slot numarası.
    pub slot: SlotId,

    /// Belirlenen clearing price (quote/base, PRICE_PRECISION cinsinden).
    pub clearing_price: u128,

    /// Token çifti.
    pub token_pair: TokenPair,

    /// Çalıştırılan emirler.
    pub executed_orders: Vec<ExecutedOrder>,

    /// Sınırda kalan (slippage/price mismatch) iptal edilmiş emirler.
    pub cancelled_orders: Vec<(OrderId, CancelReason)>,

    /// Bir sonraki slota taşınan emirler (kısmi doldurma).
    pub carryover_orders: Vec<Order>,

    /// Toplam işlem hacmi (`base` token, PRICE_PRECISION cinsinden).
    pub total_base_volume: u128,

    /// Toplam işlem hacmi (`quote` token, PRICE_PRECISION cinsinden).
    pub total_quote_volume: u128,

    /// Toplam price improvement (tüm kullanıcılar için toplam kazanım).
    pub total_price_improvement: u128,

    /// İşleme süren hesaplama süresi (mikrosaniye).
    pub computation_time_us: u64,
}

impl ClearingResult {
    /// Başarılı çalıştırılan emir sayısı.
    pub fn filled_count(&self) -> usize {
        self.executed_orders.len()
    }

    /// İptal edilen emir sayısı.
    pub fn cancelled_count(&self) -> usize {
        self.cancelled_orders.len()
    }

    /// Taşınan emir sayısı.
    pub fn carryover_count(&self) -> usize {
        self.carryover_orders.len()
    }

    /// Toplam emir sayısı.
    pub fn total_order_count(&self) -> usize {
        self.filled_count() + self.cancelled_count() + self.carryover_count()
    }

    /// Batch başarılı mı? (En az bir emir çalıştırıldı)
    pub fn has_fills(&self) -> bool {
        !self.executed_orders.is_empty()
    }

    /// Batch doluluk oranı (0.0 - 1.0).
    pub fn fill_rate(&self) -> f64 {
        let total = self.total_order_count();
        if total == 0 {
            return 0.0;
        }
        self.filled_count() as f64 / total as f64
    }
}

// ─── BATCH AUCTION ────────────────────────────────────────────────────────────

/// Tek bir slot için batch auction state. §5.3
#[derive(Debug, Clone)]
pub struct BatchAuction {
    /// Slot numarası.
    pub slot: SlotId,

    /// İşlenecek emirler (nova-mempool RevealedBatch'ten çıkarılan).
    pub orders: Vec<Order>,

    /// Auction çalıştırıldı mı?
    pub executed: bool,

    /// Auction sonucu (executed sonrası dolu).
    pub result: Option<ClearingResult>,

    /// AŞAMA 7 Görev 50: Çözüm gönderme son teslim slot'u.
    ///
    /// `submission_deadline = slot + window_slots` — bu slot geçtikten sonra
    /// yeni çözümler kabul edilmez. CoW Protocol submission window pattern.
    pub submission_deadline: SlotId,
}

impl BatchAuction {
    /// Varsayılan submission window (slot sayısı).
    /// 1 slot ≈ 400ms → 5 slot ≈ 2 saniye
    pub const DEFAULT_SUBMISSION_WINDOW: u64 = 5;

    /// Yeni boş auction oluştur.
    pub fn new(slot: SlotId) -> Self {
        let deadline = SlotId(slot.0 + Self::DEFAULT_SUBMISSION_WINDOW);
        Self {
            slot,
            orders: Vec::new(),
            executed: false,
            result: None,
            submission_deadline: deadline,
        }
    }

    /// Özel submission window ile oluştur.
    pub fn with_submission_window(slot: SlotId, window_slots: u64) -> Self {
        Self {
            slot,
            orders: Vec::new(),
            executed: false,
            result: None,
            submission_deadline: SlotId(slot.0 + window_slots),
        }
    }

    /// Çözüm gönderme hâlâ açık mı?
    pub fn is_submission_open(&self, current_slot: SlotId) -> bool {
        current_slot.0 <= self.submission_deadline.0
    }

    /// Emirleri yükle.
    pub fn with_orders(mut self, orders: Vec<Order>) -> Self {
        self.orders = orders;
        self
    }

    /// Alım emirleri.
    pub fn buy_orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter().filter(|o| o.side.is_buy())
    }

    /// Satım emirleri.
    pub fn sell_orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter().filter(|o| o.side.is_sell())
    }

    /// Alım emri sayısı.
    pub fn buy_count(&self) -> usize {
        self.buy_orders().count()
    }

    /// Satım emri sayısı.
    pub fn sell_count(&self) -> usize {
        self.sell_orders().count()
    }

    /// Toplam emir sayısı.
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }
}

// ─── AUCTION KONFİGÜRASYONU ──────────────────────────────────────────────────

/// Batch auction motor yapılandırması.
#[derive(Debug, Clone)]
pub struct AuctionConfig {
    /// Slot başına maksimum emir sayısı. §5.9.1
    pub max_orders_per_slot: usize,

    /// Varsayılan maksimum slippage (binde).
    /// `0` = slippage yok, `100` = %1
    pub default_max_slippage_bps: u16,

    /// Kısmi doldurma aktif mi?
    pub allow_partial_fills: bool,

    /// Minimum emir miktarı (spam önleme).
    pub min_order_amount: u128,

    /// Maksimum emir miktarı.
    pub max_order_amount: u128,

    /// Minimum clearing hacmi (alım/satım her iki tarafta da bu miktar olmalı).
    pub min_clearing_volume: u128,

    /// Fiyat hassasiyet basamağı (en küçük anlamlı fiyat birimi).
    pub price_tick_size: u128,

    /// Bir sonraki slota taşınan emirler için limit.
    pub max_carryover_orders: usize,

    /// Chain ID.
    pub chain_id: u64,
}

impl Default for AuctionConfig {
    fn default() -> Self {
        Self {
            max_orders_per_slot: 10_000,
            default_max_slippage_bps: 50, // %0.5
            allow_partial_fills: true,
            min_order_amount: 1_000_000,  // 1 microNSC
            // NOVA-RED-AUCTION-002: u128::MAX / 2 overflow riski → safe üst sınır.
            // amount * limit_price (u128 * u128) overflow önlemek için:
            // amount <= u128::MAX / PRICE_PRECISION garantisi sağlanır.
            // Tüm fiyat hesaplamalarında (compute_quote_amount, total_quote_cost) checked_mul
            // ile ek güvence mevcuttur.
            max_order_amount: u128::MAX / PRICE_PRECISION,
            min_clearing_volume: 0,
            price_tick_size: 1,
            max_carryover_orders: 1_000,
            chain_id: nova_primitives::constants::MAINNET_CHAIN_ID,
        }
    }
}

// ─── AUCTION İSTATİSTİKLERİ ────────────────────────────────────────────────────

/// Bir auction döneminin istatistikleri (monitoring/telemetry).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuctionStats {
    /// İşlenen toplam slot sayısı.
    pub total_slots_processed: u64,

    /// Toplam emir sayısı.
    pub total_orders_processed: u64,

    /// Çalıştırılan emir sayısı.
    pub total_orders_filled: u64,

    /// İptal edilen emir sayısı.
    pub total_orders_cancelled: u64,

    /// Taşınan emir sayısı.
    pub total_orders_carried_over: u64,

    /// Toplam işlem hacmi (`base` token).
    pub total_base_volume: u128,

    /// Toplam işlem hacmi (`quote` token).
    pub total_quote_volume: u128,

    /// Toplam kullanıcı kazanımı (price improvement).
    pub total_price_improvement: u128,

    /// Ortalama clearing time (mikrosaniye).
    pub avg_clearing_time_us: u64,

    /// Boş (emirsiz) slot sayısı.
    pub empty_slot_count: u64,

    /// Tek taraflı (kesişim olmayan) slot sayısı.
    pub no_intersection_slot_count: u64,
}

impl AuctionStats {
    /// Ortalama fill rate.
    pub fn avg_fill_rate(&self) -> f64 {
        if self.total_orders_processed == 0 {
            return 0.0;
        }
        self.total_orders_filled as f64 / self.total_orders_processed as f64
    }

    /// Bir clearing sonucunu istatistiklere ekle.
    pub fn update_from_result(&mut self, result: &ClearingResult) {
        self.total_slots_processed += 1;
        let total = result.total_order_count() as u64;
        self.total_orders_processed += total;
        self.total_orders_filled += result.filled_count() as u64;
        self.total_orders_cancelled += result.cancelled_count() as u64;
        self.total_orders_carried_over += result.carryover_count() as u64;
        self.total_base_volume = self.total_base_volume.saturating_add(result.total_base_volume);
        self.total_quote_volume = self.total_quote_volume.saturating_add(result.total_quote_volume);
        self.total_price_improvement = self
            .total_price_improvement
            .saturating_add(result.total_price_improvement);

        // Ortalama süreyi güncelle (basit moving average)
        if self.total_slots_processed == 1 {
            self.avg_clearing_time_us = result.computation_time_us;
        } else {
            let n = self.total_slots_processed;
            self.avg_clearing_time_us =
                (self.avg_clearing_time_us * (n - 1) + result.computation_time_us) / n;
        }

        if !result.has_fills() {
            if result.total_order_count() == 0 {
                self.empty_slot_count += 1;
            } else {
                self.no_intersection_slot_count += 1;
            }
        }
    }
}

// ─── EMIR KİTABI ÖZET ─────────────────────────────────────────────────────────

/// Slot için emir kitabı özeti (monitoring).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderBookSummary {
    /// Slot numarası.
    pub slot: SlotId,

    /// Toplam alım emri sayısı.
    pub buy_order_count: usize,

    /// Toplam satım emri sayısı.
    pub sell_order_count: usize,

    /// En iyi alım fiyatı (en yüksek bid).
    pub best_bid: Option<u128>,

    /// En iyi satım fiyatı (en düşük ask).
    pub best_ask: Option<u128>,

    /// Mid price (bid/ask ortalaması).
    pub mid_price: Option<u128>,

    /// Toplam alım hacmi.
    pub total_buy_volume: u128,

    /// Toplam satım hacmi.
    pub total_sell_volume: u128,
}

impl OrderBookSummary {
    /// Spread (bid-ask farkı).
    pub fn spread(&self) -> Option<u128> {
        match (self.best_ask, self.best_bid) {
            (Some(ask), Some(bid)) if ask >= bid => Some(ask - bid),
            _ => None,
        }
    }

    /// Market derin mi? (Her iki tarafta da yeterli hacim)
    pub fn is_liquid(&self, min_volume: u128) -> bool {
        self.total_buy_volume >= min_volume && self.total_sell_volume >= min_volume
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::types::Address;

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn test_token(seed: u8) -> TokenId {
        TokenId([seed; 20])
    }

    fn test_order(side: OrderSide, amount: u128, limit_price: u128) -> Order {
        Order {
            id: OrderId::from_bytes([1u8; 32]),
            side,
            pair: TokenPair::new(test_token(0), TokenId::NSC),
            amount,
            limit_price,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(1000),
            trader: test_address(1),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        }
    }

    // ─── TokenId Testleri ─────────────────────────────────────────────────────

    #[test]
    fn test_token_id_nsc_native() {
        assert!(TokenId::NSC.is_native(), "NSC native token olmalı");
        assert!(!test_token(1).is_native(), "Diğer token native değil");
    }

    #[test]
    fn test_token_id_from_address() {
        let addr = Address::from_bytes([0xABu8; 20]);
        let token = TokenId::from_address(addr);
        assert_eq!(token.as_bytes(), &[0xABu8; 20]);
    }

    #[test]
    fn test_token_id_hex() {
        let token = test_token(0xFFu8);
        let hex = token.to_hex();
        assert_eq!(hex.len(), 40, "Hex 40 karakter olmalı");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ─── TokenPair Testleri ───────────────────────────────────────────────────

    #[test]
    fn test_token_pair_ayni_token_gecersiz() {
        let pair = TokenPair::new(TokenId::NSC, TokenId::NSC);
        assert!(pair.is_same_token(), "Aynı token çifti geçersiz");
    }

    #[test]
    fn test_token_pair_canonical() {
        let token_a = test_token(5);
        let token_b = test_token(2);
        let pair = TokenPair::new(token_a, token_b);
        let canonical = pair.canonical();
        assert!(canonical.base.0 <= canonical.quote.0, "Canonical form sıralı olmalı");
    }

    #[test]
    fn test_token_pair_display() {
        let pair = TokenPair::new(test_token(1), TokenId::NSC);
        let s = pair.to_string();
        assert!(s.contains("Token("), "Display string token içermeli");
    }

    // ─── OrderSide Testleri ───────────────────────────────────────────────────

    #[test]
    fn test_order_side_is_buy_sell() {
        assert!(OrderSide::Buy.is_buy());
        assert!(!OrderSide::Buy.is_sell());
        assert!(OrderSide::Sell.is_sell());
        assert!(!OrderSide::Sell.is_buy());
    }

    #[test]
    fn test_order_side_opposite() {
        assert_eq!(OrderSide::Buy.opposite(), OrderSide::Sell);
        assert_eq!(OrderSide::Sell.opposite(), OrderSide::Buy);
    }

    #[test]
    fn test_order_side_display() {
        assert_eq!(OrderSide::Buy.to_string(), "ALIM");
        assert_eq!(OrderSide::Sell.to_string(), "SATIM");
    }

    // ─── Order Fiyat Kontrolü Testleri ────────────────────────────────────────

    #[test]
    fn test_alim_emri_fiyat_kabuledilebilir() {
        // limit_price = 200, slippage = 50 bps (%0.5)
        // Maksimum kabul: 200 * (10000 + 50) / 10000 = 201
        let order = test_order(OrderSide::Buy, 100, 200 * PRICE_PRECISION);

        // Clearing price <= 200: kabul
        assert!(order.price_acceptable(200 * PRICE_PRECISION));
        // Clearing price = 201: slippage içinde kabul
        assert!(order.price_acceptable(201 * PRICE_PRECISION));
        // Clearing price = 202: reddet
        assert!(!order.price_acceptable(202 * PRICE_PRECISION));
    }

    #[test]
    fn test_satim_emri_fiyat_kabuledilebilir() {
        // limit_price = 200, slippage = 50 bps
        // Minimum kabul: 200 * (10000 - 50) / 10000 = 199
        let order = test_order(OrderSide::Sell, 100, 200 * PRICE_PRECISION);

        // Clearing price >= 200: kabul
        assert!(order.price_acceptable(200 * PRICE_PRECISION));
        // Clearing price = 199: slippage içinde kabul
        assert!(order.price_acceptable(199 * PRICE_PRECISION));
        // Clearing price = 198: reddet
        assert!(!order.price_acceptable(198 * PRICE_PRECISION));
    }

    #[test]
    fn test_slippage_sifir_katı_limit() {
        let mut order = test_order(OrderSide::Buy, 100, 200 * PRICE_PRECISION);
        order.max_slippage_bps = 0;

        // Tam limit fiyat: kabul
        assert!(order.price_acceptable(200 * PRICE_PRECISION));
        // Limit üstü: slippage 0 olduğunda reddedilir
        assert!(!order.price_acceptable(201 * PRICE_PRECISION));
        // Limit altı: alıcı için her zaman kabul (daha ucuz aldı → price improvement)
        assert!(order.price_acceptable(199 * PRICE_PRECISION));
    }

    #[test]
    fn test_kismi_doldurma_izni() {
        let mut order = test_order(OrderSide::Buy, 100, 200 * PRICE_PRECISION);
        order.class = OrderClass::Market;
        assert!(!order.allows_partial_fill(), "Market emri kısmi doldurma desteklemez");

        order.class = OrderClass::LimitPartialFill;
        assert!(order.allows_partial_fill(), "LimitPartialFill kısmi doldurma destekler");
    }

    #[test]
    fn test_order_slot_gecerliligi() {
        let order = test_order(OrderSide::Buy, 100, 200 * PRICE_PRECISION);
        assert!(order.is_valid_for_slot(SlotId(1000)));
        assert!(order.is_valid_for_slot(SlotId(500)));
        assert!(!order.is_valid_for_slot(SlotId(1001)), "Süresi geçmiş slot geçersiz");
    }

    // ─── BatchAuction Testleri ────────────────────────────────────────────────

    #[test]
    fn test_batch_auction_bos_olusturma() {
        let auction = BatchAuction::new(SlotId(42));
        assert_eq!(auction.slot, SlotId(42));
        assert!(!auction.executed);
        assert!(auction.result.is_none());
        assert_eq!(auction.order_count(), 0);
    }

    #[test]
    fn test_batch_auction_emirler_ile() {
        let orders = vec![
            test_order(OrderSide::Buy, 100, 200 * PRICE_PRECISION),
            test_order(OrderSide::Sell, 80, 195 * PRICE_PRECISION),
            test_order(OrderSide::Buy, 50, 210 * PRICE_PRECISION),
        ];
        let auction = BatchAuction::new(SlotId(1)).with_orders(orders);

        assert_eq!(auction.order_count(), 3);
        assert_eq!(auction.buy_count(), 2);
        assert_eq!(auction.sell_count(), 1);
    }

    // ─── ClearingResult Testleri ──────────────────────────────────────────────

    #[test]
    fn test_clearing_result_fill_rate() {
        let result = ClearingResult {
            slot: SlotId(1),
            clearing_price: 200 * PRICE_PRECISION,
            token_pair: TokenPair::new(test_token(1), TokenId::NSC),
            executed_orders: vec![
                ExecutedOrder {
                    order_id: OrderId::ZERO,
                    trader: test_address(1),
                    side: OrderSide::Buy,
                    pair: TokenPair::new(test_token(1), TokenId::NSC),
                    requested_amount: 100,
                    executed_amount: 100,
                    clearing_price: 200 * PRICE_PRECISION,
                    quote_amount: 200 * 100,
                    price_improvement: 0,
                    is_partial: false,
                    remaining_amount: 0,
                    execution_order: 0,
                },
            ],
            cancelled_orders: vec![
                (OrderId::ZERO, CancelReason::PriceNotMet),
            ],
            carryover_orders: vec![],
            total_base_volume: 100,
            total_quote_volume: 20_000,
            total_price_improvement: 0,
            computation_time_us: 1500,
        };

        assert_eq!(result.filled_count(), 1);
        assert_eq!(result.cancelled_count(), 1);
        assert_eq!(result.carryover_count(), 0);
        assert_eq!(result.total_order_count(), 2);
        assert!((result.fill_rate() - 0.5).abs() < 1e-10);
        assert!(result.has_fills());
    }

    // ─── AuctionStats Testleri ────────────────────────────────────────────────

    #[test]
    fn test_auction_stats_bos_baslangic() {
        let stats = AuctionStats::default();
        assert_eq!(stats.total_slots_processed, 0);
        assert_eq!(stats.avg_fill_rate(), 0.0);
    }

    #[test]
    fn test_auction_stats_guncelleme() {
        let mut stats = AuctionStats::default();
        let result = ClearingResult {
            slot: SlotId(1),
            clearing_price: 100,
            token_pair: TokenPair::new(test_token(1), TokenId::NSC),
            executed_orders: vec![ExecutedOrder {
                order_id: OrderId::ZERO,
                trader: test_address(1),
                side: OrderSide::Buy,
                pair: TokenPair::new(test_token(1), TokenId::NSC),
                requested_amount: 50,
                executed_amount: 50,
                clearing_price: 100,
                quote_amount: 5000,
                price_improvement: 0,
                is_partial: false,
                remaining_amount: 0,
                execution_order: 0,
            }],
            cancelled_orders: vec![],
            carryover_orders: vec![],
            total_base_volume: 50,
            total_quote_volume: 5000,
            total_price_improvement: 0,
            computation_time_us: 2000,
        };

        stats.update_from_result(&result);
        assert_eq!(stats.total_slots_processed, 1);
        assert_eq!(stats.total_orders_processed, 1);
        assert_eq!(stats.total_orders_filled, 1);
        assert_eq!(stats.avg_clearing_time_us, 2000);
        assert!((stats.avg_fill_rate() - 1.0).abs() < 1e-10);
    }

    // ─── OrderBookSummary Testleri ────────────────────────────────────────────

    #[test]
    fn test_order_book_summary_spread() {
        let summary = OrderBookSummary {
            slot: SlotId(1),
            buy_order_count: 3,
            sell_order_count: 3,
            best_bid: Some(198),
            best_ask: Some(202),
            mid_price: Some(200),
            total_buy_volume: 1000,
            total_sell_volume: 1000,
        };
        assert_eq!(summary.spread(), Some(4));
        assert!(summary.is_liquid(500));
        assert!(!summary.is_liquid(2000));
    }

    #[test]
    fn test_order_book_summary_bos_spread() {
        let summary = OrderBookSummary {
            slot: SlotId(1),
            buy_order_count: 0,
            sell_order_count: 0,
            best_bid: None,
            best_ask: None,
            mid_price: None,
            total_buy_volume: 0,
            total_sell_volume: 0,
        };
        assert_eq!(summary.spread(), None);
    }

    #[test]
    fn test_executed_order_price_improvement() {
        let exe_order = ExecutedOrder {
            order_id: OrderId::ZERO,
            trader: test_address(1),
            side: OrderSide::Buy,
            pair: TokenPair::new(test_token(1), TokenId::NSC),
            requested_amount: 100,
            executed_amount: 100,
            clearing_price: 190 * PRICE_PRECISION,
            quote_amount: 19_000,
            price_improvement: 10 * PRICE_PRECISION as i128,  // 10 NSC kazandı
            is_partial: false,
            remaining_amount: 0,
            execution_order: 0,
        };
        // improvement bps = 10 * 10000 / 190 ≈ 526 bps
        let bps = exe_order.price_improvement_bps();
        assert!(bps > 0, "Price improvement pozitif olmalı");
        assert!(exe_order.is_fully_filled());
    }

    #[test]
    fn test_price_precision_sabit_deger() {
        assert_eq!(PRICE_PRECISION, 1_000_000_000_000_000_000u128);
    }

    #[test]
    fn test_order_class_display() {
        assert_eq!(OrderClass::Market.to_string(), "Piyasa");
        assert_eq!(OrderClass::Limit.to_string(), "Limit");
        assert_eq!(OrderClass::LimitPartialFill.to_string(), "LimitKısmi");
    }
}
