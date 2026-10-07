//! Batch auction SDK — CoW Protocol pattern
//!
//! CoW Protocol: `OrderBuilder`, `Order`, `Settlement`, `SolverCompetition`
//! Nova Shield: §5.2 — Şifreli mempool, batch auction, MEV koruması

use crate::errors::{SdkError, SdkResult};
use serde::{Deserialize, Serialize};

// ─── OrderSide ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

// ─── OrderKind ────────────────────────────────────────────────────────────────

/// Emir türü — CoW Protocol `OrderKind`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderKind {
    /// Market order (herhangi bir fiyattan)
    Market,
    /// Limit order (belirli fiyattan)
    Limit { price: u64 },
    /// Fill-or-Kill
    FillOrKill { price: u64 },
}

// ─── BatchOrder ───────────────────────────────────────────────────────────────

/// Nova Shield batch emri — CoW Protocol `Order` pattern
/// `Order { sellToken, buyToken, sellAmount, buyAmount, receiver, validTo, ... }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOrder {
    /// Emir ID (client tarafında üretilen)
    pub order_id: [u8; 32],
    /// Gönderen hesabı
    pub sender: [u8; 20],
    /// Satılan token (addres veya "NOVA")
    pub sell_token: String,
    /// Alınan token
    pub buy_token: String,
    /// Satılan miktar
    pub sell_amount: u64,
    /// Minimum alınan miktar
    pub min_buy_amount: u64,
    /// Emir türü
    pub kind: OrderKind,
    /// Geçerlilik süresi (slot)
    pub valid_until: u64,
    /// Zaman damgası
    pub created_at: u64,
    /// Gönderilen mi?
    pub submitted: bool,
}

impl BatchOrder {
    pub fn is_expired(&self, current_slot: u64) -> bool {
        current_slot > self.valid_until
    }

    /// Emir hash'i (şifreleme öncesi)
    pub fn order_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-order-v1");
        h.update(&self.sender);
        h.update(self.sell_token.as_bytes());
        h.update(self.buy_token.as_bytes());
        h.update(&self.sell_amount.to_le_bytes());
        h.update(&self.min_buy_amount.to_le_bytes());
        *h.finalize().as_bytes()
    }
}

// ─── OrderBuilder ─────────────────────────────────────────────────────────────

/// Emir inşaatçısı — CoW Protocol `OrderBuilder` pattern
pub struct OrderBuilder {
    sender: Option<[u8; 20]>,
    sell_token: Option<String>,
    buy_token: Option<String>,
    sell_amount: Option<u64>,
    min_buy_amount: Option<u64>,
    kind: OrderKind,
    valid_for_slots: u64,
}

impl OrderBuilder {
    pub fn new() -> Self {
        Self {
            sender: None,
            sell_token: None,
            buy_token: None,
            sell_amount: None,
            min_buy_amount: None,
            kind: OrderKind::Market,
            valid_for_slots: 100,
        }
    }

    pub fn sender(mut self, addr: [u8; 20]) -> Self {
        self.sender = Some(addr);
        self
    }

    pub fn sell(mut self, token: impl Into<String>, amount: u64) -> Self {
        self.sell_token = Some(token.into());
        self.sell_amount = Some(amount);
        self
    }

    pub fn buy(mut self, token: impl Into<String>, min_amount: u64) -> Self {
        self.buy_token = Some(token.into());
        self.min_buy_amount = Some(min_amount);
        self
    }

    pub fn limit_price(mut self, price: u64) -> Self {
        self.kind = OrderKind::Limit { price };
        self
    }

    pub fn fill_or_kill(mut self, price: u64) -> Self {
        self.kind = OrderKind::FillOrKill { price };
        self
    }

    pub fn valid_for(mut self, slots: u64) -> Self {
        self.valid_for_slots = slots;
        self
    }

    pub fn build(self, current_slot: u64) -> SdkResult<BatchOrder> {
        let sender = self.sender.ok_or_else(|| SdkError::InvalidParameter {
            param: "sender".into(),
            reason: "gönderici adresi gerekli".into(),
        })?;
        let sell_token = self.sell_token.ok_or_else(|| SdkError::InvalidParameter {
            param: "sell_token".into(),
            reason: "satılan token gerekli".into(),
        })?;
        let buy_token = self.buy_token.ok_or_else(|| SdkError::InvalidParameter {
            param: "buy_token".into(),
            reason: "alınan token gerekli".into(),
        })?;
        let sell_amount = self.sell_amount.ok_or_else(|| SdkError::InvalidParameter {
            param: "sell_amount".into(),
            reason: "satılan miktar gerekli".into(),
        })?;
        let min_buy_amount = self.min_buy_amount.unwrap_or(0);

        if sell_token == buy_token {
            return Err(SdkError::InvalidParameter {
                param: "tokens".into(),
                reason: "satılan ve alınan token aynı olamaz".into(),
            });
        }

        if sell_amount == 0 {
            return Err(SdkError::InvalidParameter {
                param: "sell_amount".into(),
                reason: "miktar sıfır olamaz".into(),
            });
        }

        let mut h = blake3::Hasher::new();
        h.update(b"order-id");
        h.update(&sender);
        h.update(sell_token.as_bytes());
        h.update(&sell_amount.to_le_bytes());
        h.update(&current_slot.to_le_bytes());
        let order_id = *h.finalize().as_bytes();

        Ok(BatchOrder {
            order_id,
            sender,
            sell_token,
            buy_token,
            sell_amount,
            min_buy_amount,
            kind: self.kind,
            valid_until: current_slot + self.valid_for_slots,
            created_at: current_slot,
            submitted: false,
        })
    }
}

// ─── BatchClient ──────────────────────────────────────────────────────────────

/// Batch auction istemcisi
pub struct BatchClient {
    pending: Vec<BatchOrder>,
    submitted: Vec<BatchOrder>,
    current_slot: u64,
}

impl BatchClient {
    pub fn new(current_slot: u64) -> Self {
        Self { pending: vec![], submitted: vec![], current_slot }
    }

    /// Emir gönder (şifreli mempool'a)
    pub fn submit_order(&mut self, mut order: BatchOrder) -> SdkResult<[u8; 32]> {
        if order.is_expired(self.current_slot) {
            return Err(SdkError::BatchError {
                reason: "emir süresi dolmuş".into(),
            });
        }
        let id = order.order_id;
        order.submitted = true;
        self.submitted.push(order);
        Ok(id)
    }

    /// Bekleyen emir ekle
    pub fn queue_order(&mut self, order: BatchOrder) {
        self.pending.push(order);
    }

    /// Tüm bekleyenleri gönder
    pub fn flush_pending(&mut self) -> SdkResult<Vec<[u8; 32]>> {
        let mut ids = vec![];
        let pending = std::mem::take(&mut self.pending);
        for order in pending {
            let id = self.submit_order(order)?;
            ids.push(id);
        }
        Ok(ids)
    }

    /// Slot güncelle
    pub fn advance_slot(&mut self, slot: u64) {
        self.current_slot = slot;
    }

    pub fn submitted_count(&self) -> usize {
        self.submitted.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Belirli token çiftindeki emirler
    pub fn orders_for_pair(&self, sell: &str, buy: &str) -> Vec<&BatchOrder> {
        self.submitted.iter()
            .filter(|o| o.sell_token == sell && o.buy_token == buy)
            .collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sender() -> [u8; 20] {
        [0xAAu8; 20]
    }

    fn make_order(slot: u64) -> BatchOrder {
        OrderBuilder::new()
            .sender(sender())
            .sell("NOVA", 1_000_000_000)
            .buy("USDC", 900_000)
            .valid_for(100)
            .build(slot)
            .unwrap()
    }

    #[test]
    fn test_order_builder_basic() {
        let order = make_order(1000);
        assert_eq!(order.sell_token, "NOVA");
        assert_eq!(order.buy_token, "USDC");
        assert_eq!(order.sell_amount, 1_000_000_000);
        assert!(!order.submitted);
    }

    #[test]
    fn test_order_builder_limit() {
        let order = OrderBuilder::new()
            .sender(sender())
            .sell("NOVA", 1_000)
            .buy("USDC", 900)
            .limit_price(950)
            .build(500)
            .unwrap();

        assert!(matches!(order.kind, OrderKind::Limit { price: 950 }));
    }

    #[test]
    fn test_order_builder_same_token_error() {
        let result = OrderBuilder::new()
            .sender(sender())
            .sell("NOVA", 1_000)
            .buy("NOVA", 900)
            .build(100);
        assert!(matches!(result, Err(SdkError::InvalidParameter { .. })));
    }

    #[test]
    fn test_order_builder_zero_amount_error() {
        let result = OrderBuilder::new()
            .sender(sender())
            .sell("NOVA", 0)
            .buy("USDC", 0)
            .build(100);
        assert!(matches!(result, Err(SdkError::InvalidParameter { .. })));
    }

    #[test]
    fn test_order_hash_deterministic() {
        let o = make_order(100);
        assert_eq!(o.order_hash(), o.order_hash());
    }

    #[test]
    fn test_order_expiry() {
        let order = make_order(100); // valid_until = 200
        assert!(!order.is_expired(150));
        assert!(order.is_expired(201));
    }

    #[test]
    fn test_batch_client_submit() {
        let mut client = BatchClient::new(100);
        let order = make_order(100);
        let id = client.submit_order(order).unwrap();
        assert_ne!(id, [0u8; 32]);
        assert_eq!(client.submitted_count(), 1);
    }

    #[test]
    fn test_batch_client_expired_order() {
        let mut client = BatchClient::new(300);
        let order = make_order(100); // valid_until = 200
        let result = client.submit_order(order);
        assert!(matches!(result, Err(SdkError::BatchError { .. })));
    }

    #[test]
    fn test_batch_client_flush_pending() {
        let mut client = BatchClient::new(100);
        client.queue_order(make_order(100));
        client.queue_order(make_order(100));
        let ids = client.flush_pending().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(client.pending_count(), 0);
        assert_eq!(client.submitted_count(), 2);
    }

    #[test]
    fn test_batch_client_orders_for_pair() {
        let mut client = BatchClient::new(100);
        client.submit_order(make_order(100)).unwrap();
        let nsc_usdc = client.orders_for_pair("NOVA", "USDC");
        assert_eq!(nsc_usdc.len(), 1);
        let btc_eth = client.orders_for_pair("BTC", "ETH");
        assert_eq!(btc_eth.len(), 0);
    }

    #[test]
    fn test_fill_or_kill_order() {
        let order = OrderBuilder::new()
            .sender(sender())
            .sell("NOVA", 5_000)
            .buy("USDC", 4_500)
            .fill_or_kill(4_800)
            .build(100)
            .unwrap();
        assert!(matches!(order.kind, OrderKind::FillOrKill { .. }));
    }
}
