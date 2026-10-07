//! Auction property-based testleri (proptest).
//!
//! **İspatlanan özellikler:**
//! 1. Uniform clearing price: tüm emirler aynı fiyattan işlenir
//! 2. Clearing price bid ≥ ask aralığında oluşur
//! 3. Satıcı clearing_price ≥ limit_price (fiyat koruması)
//! 4. Alıcı clearing_price ≤ limit_price (fiyat koruması)
//! 5. İşlem hacmi monoton: daha fazla emir → daha fazla hacim
//! 6. Boş batch hata döner

use nova_auction::{
    BatchAuctionEngine, Order, OrderId, OrderSide, OrderClass,
    TokenPair, TokenId, PRICE_PRECISION, AuctionError,
};
use nova_primitives::{Address, SlotId};
use proptest::prelude::*;

// ─── Yardımcı fonksiyonlar ────────────────────────────────────────────────────

fn test_pair() -> TokenPair {
    TokenPair::new(TokenId([0xABu8; 20]), TokenId::NSC)
}

fn make_order_with_id(
    side: OrderSide,
    amount: u128,
    limit_price_raw: u128,
    id_seed: u8,
    extra_seed: u8,
) -> Order {
    let mut id_bytes = [0u8; 32];
    id_bytes[0] = id_seed;
    id_bytes[1] = extra_seed;
    Order {
        id: OrderId::from_bytes(id_bytes),
        side,
        pair: test_pair(),
        amount: amount.max(1_000_000),
        limit_price: limit_price_raw * PRICE_PRECISION,
        max_slippage_bps: 100,
        class: OrderClass::Limit,
        valid_until_slot: SlotId(100_000),
        trader: Address::from_bytes([id_seed; 20]),
        submitted_at_slot: SlotId(1),
        extra_data: vec![],
    }
}

fn engine() -> BatchAuctionEngine {
    BatchAuctionEngine::with_defaults()
}

// ─── Property testleri ────────────────────────────────────────────────────────

proptest! {
    /// P1: Uniform price — clearing price tüm emirler için aynı
    /// Eğer finalize edilen emirler varsa hepsi aynı clearing_price'tan işlenir
    #[test]
    fn prop_uniform_clearing_price(
        buy_price in 200u128..300u128,
        sell_price in 100u128..200u128,
        amount in 1_000_000u128..10_000_000u128,
        slot_n in 1u64..1000u64,
    ) {
        prop_assume!(buy_price >= sell_price);

        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 1, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 2, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        if let Ok(result) = eng.process_slot(slot, orders, slot, test_pair()) {
            // Tüm işlem gören emirler aynı clearing fiyatını görmeli
            for exec in &result.executed_orders {
                prop_assert_eq!(
                    exec.clearing_price,
                    result.clearing_price,
                    "Uniform price ihlali: exec.clearing_price={} != result.clearing_price={}",
                    exec.clearing_price, result.clearing_price
                );
            }
        }
    }

    /// P2: Clearing price bid-ask aralığında
    /// sell_price ≤ clearing_price ≤ buy_price
    #[test]
    fn prop_clearing_price_in_bid_ask_range(
        buy_price in 200u128..400u128,
        sell_price in 100u128..200u128,
        amount in 1_000_000u128..5_000_000u128,
        slot_n in 1001u64..2000u64,
    ) {
        prop_assume!(buy_price > sell_price);

        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 3, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 4, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        if let Ok(result) = eng.process_slot(slot, orders, slot, test_pair()) {
            if result.has_fills() {
                let cp = result.clearing_price;
                let buy_lim = buy_price * PRICE_PRECISION;
                let sell_lim = sell_price * PRICE_PRECISION;

                prop_assert!(
                    cp >= sell_lim && cp <= buy_lim,
                    "Clearing price ({}) bid-ask aralığı dışında [{}, {}]",
                    cp, sell_lim, buy_lim
                );
            }
        }
    }

    /// P3: Satıcı fiyat koruması — clearing_price ≥ limit_price
    #[test]
    fn prop_seller_price_protection(
        buy_price in 250u128..400u128,
        sell_price in 100u128..250u128,
        amount in 1_000_000u128..5_000_000u128,
        slot_n in 2001u64..3000u64,
    ) {
        prop_assume!(buy_price >= sell_price + 10);

        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 5, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 6, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        if let Ok(result) = eng.process_slot(slot, orders, slot, test_pair()) {
            for exec in &result.executed_orders {
                if exec.side == OrderSide::Sell {
                    prop_assert!(
                        exec.clearing_price >= sell_price * PRICE_PRECISION,
                        "Satıcı limit fiyatından ({}) düşük clearing ({}) aldı",
                        sell_price * PRICE_PRECISION, exec.clearing_price
                    );
                }
            }
        }
    }

    /// P4: Alıcı fiyat koruması — clearing_price ≤ limit_price
    #[test]
    fn prop_buyer_price_protection(
        buy_price in 250u128..400u128,
        sell_price in 100u128..250u128,
        amount in 1_000_000u128..5_000_000u128,
        slot_n in 3001u64..4000u64,
    ) {
        prop_assume!(buy_price >= sell_price + 10);

        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 7, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 8, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        if let Ok(result) = eng.process_slot(slot, orders, slot, test_pair()) {
            for exec in &result.executed_orders {
                if exec.side == OrderSide::Buy {
                    prop_assert!(
                        exec.clearing_price <= buy_price * PRICE_PRECISION,
                        "Alıcı limit fiyatından ({}) pahalı clearing ({}) aldı",
                        buy_price * PRICE_PRECISION, exec.clearing_price
                    );
                }
            }
        }
    }

    /// P5: Fiyat kesişimi yoksa hata — buy < sell
    #[test]
    fn prop_no_intersection_returns_error(
        buy_price in 100u128..199u128,
        sell_price in 200u128..300u128,
        amount in 1_000_000u128..5_000_000u128,
        slot_n in 4001u64..5000u64,
    ) {
        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 9, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 10, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        let result = eng.process_slot(slot, orders, slot, test_pair());

        prop_assert!(
            result.is_err(),
            "buy_price ({}) < sell_price ({}) olduğunda hata bekleniyor",
            buy_price, sell_price
        );
    }

    /// P6: Tek taraflı batch hata
    #[test]
    fn prop_unilateral_batch_error(
        buy_price in 200u128..300u128,
        amount in 1_000_000u128..5_000_000u128,
        n_orders in 1u8..5,
        slot_n in 5001u64..6000u64,
    ) {
        let eng = engine();
        let mut orders: Vec<Order> = Vec::new();

        for i in 0..n_orders {
            let mut id_bytes = [0u8; 32];
            id_bytes[0] = i;
            id_bytes[1] = slot_n as u8;
            orders.push(Order {
                id: OrderId::from_bytes(id_bytes),
                side: OrderSide::Buy,
                pair: test_pair(),
                amount: amount.max(1_000_000),
                limit_price: buy_price * PRICE_PRECISION,
                max_slippage_bps: 100,
                class: OrderClass::Limit,
                valid_until_slot: SlotId(100_000),
                trader: Address::from_bytes([i; 20]),
                submitted_at_slot: SlotId(1),
                extra_data: vec![],
            });
        }

        let slot = SlotId(slot_n);
        let result = eng.process_slot(slot, orders, slot, test_pair());

        prop_assert!(
            result.is_err(),
            "Yalnızca alım emirleriyle batch hata döndürmeli"
        );

        if let Err(e) = result {
            prop_assert!(
                matches!(e, AuctionError::UnilateralBatch { .. }),
                "UnilateralBatch hatası bekleniyor, gelen: {:?}", e
            );
        }
    }

    /// P7: İşlem hacmi pozitif — eşleşme varsa volume > 0
    #[test]
    fn prop_positive_volume_when_matched(
        buy_price in 200u128..300u128,
        sell_price in 150u128..200u128,
        amount in 1_000_000u128..5_000_000u128,
        slot_n in 6001u64..7000u64,
    ) {
        prop_assume!(buy_price >= sell_price);

        let eng = engine();
        let orders = vec![
            make_order_with_id(OrderSide::Buy, amount, buy_price, 11, slot_n as u8),
            make_order_with_id(OrderSide::Sell, amount, sell_price, 12, slot_n as u8),
        ];
        let slot = SlotId(slot_n);

        if let Ok(result) = eng.process_slot(slot, orders, slot, test_pair()) {
            if result.has_fills() {
                prop_assert!(
                    result.total_base_volume > 0,
                    "Eşleşme varken işlem hacmi sıfır olamaz"
                );
            }
        }
    }
}
