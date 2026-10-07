/// Batch Auction motoru benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - BatchAuctionEngine::process_slot()  — 10 / 100 / 500 emirle batch clearing
/// - Uniform price hesabı throughput     — emir sayısına göre ölçekleme
/// - Orderbook analyze()                 — sadece fiyat analizi (execute yok)
/// - Clearing price kararlılığı          — farklı spread'lerde hız
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_auction::{
    BatchAuctionEngine, Order, OrderId, OrderSide, OrderClass,
    TokenPair, TokenId, PRICE_PRECISION,
};
use nova_primitives::{Address, SlotId};

fn test_pair() -> TokenPair {
    TokenPair::new(TokenId([0xBBu8; 20]), TokenId::NSC)
}

fn make_order(side: OrderSide, amount: u128, price: u128, seed: u8, slot_n: u64) -> Order {
    let mut id_bytes = [0u8; 32];
    id_bytes[0] = seed;
    id_bytes[1] = (slot_n & 0xFF) as u8;
    id_bytes[2] = ((slot_n >> 8) & 0xFF) as u8;
    Order {
        id: OrderId::from_bytes(id_bytes),
        side,
        pair: test_pair(),
        amount: amount.max(1_000_000),
        limit_price: price * PRICE_PRECISION,
        max_slippage_bps: 100,
        class: OrderClass::Limit,
        valid_until_slot: SlotId(10_000_000),
        trader: Address::from_bytes([seed; 20]),
        submitted_at_slot: SlotId(1),
        extra_data: vec![],
    }
}

/// N alım + N satım emirinden oluşan batch oluştur
fn make_balanced_batch(n: usize, slot_n: u64) -> Vec<Order> {
    let mut orders = Vec::with_capacity(n * 2);
    for i in 0..n {
        let buy_price = 200u128 + (i as u128 % 20);
        let sell_price = 190u128 + (i as u128 % 20);
        let amount = 1_000_000u128 + i as u128 * 1000;

        let mut buy_id = [0u8; 32];
        buy_id[0] = (i % 255) as u8;
        buy_id[1] = (slot_n & 0xFF) as u8;
        orders.push(Order {
            id: OrderId::from_bytes(buy_id),
            side: OrderSide::Buy,
            pair: test_pair(),
            amount,
            limit_price: buy_price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(10_000_000),
            trader: Address::from_bytes([(i % 255) as u8; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        });

        let mut sell_id = [0u8; 32];
        sell_id[0] = (i % 255) as u8;
        sell_id[1] = (slot_n & 0xFF) as u8;
        sell_id[2] = 0xFF;
        orders.push(Order {
            id: OrderId::from_bytes(sell_id),
            side: OrderSide::Sell,
            pair: test_pair(),
            amount,
            limit_price: sell_price * PRICE_PRECISION,
            max_slippage_bps: 50,
            class: OrderClass::Limit,
            valid_until_slot: SlotId(10_000_000),
            trader: Address::from_bytes([((i + 100) % 255) as u8; 20]),
            submitted_at_slot: SlotId(1),
            extra_data: vec![],
        });
    }
    orders
}

// ─── process_slot throughput — N emir ────────────────────────────────────────

fn bench_auction_process_slot(c: &mut Criterion) {
    let order_counts = [10usize, 50, 100, 500];
    let mut group = c.benchmark_group("auction_process_slot");

    for &n in &order_counts {
        group.throughput(Throughput::Elements(n as u64 * 2)); // n alım + n satım

        group.bench_with_input(
            BenchmarkId::new("orders", n * 2),
            &n,
            |b, &n| {
                let eng = BatchAuctionEngine::with_defaults();
                let mut slot_counter = 1u64;

                b.iter(|| {
                    let orders = make_balanced_batch(n, slot_counter);
                    let slot = SlotId(slot_counter);
                    let _ = eng.process_slot(slot, orders, slot, test_pair());
                    slot_counter += 1;
                });
            },
        );
    }
    group.finish();
}

// ─── Orderbook analizi (execute yok) ─────────────────────────────────────────

fn bench_auction_analyze_orderbook(c: &mut Criterion) {
    let order_counts = [10usize, 100, 500];
    let mut group = c.benchmark_group("auction_analyze_orderbook");

    for &n in &order_counts {
        group.bench_with_input(
            BenchmarkId::new("orders", n * 2),
            &n,
            |b, &n| {
                let eng = BatchAuctionEngine::with_defaults();
                let orders = make_balanced_batch(n, 9_999_999);
                let slot = SlotId(9_999_999);

                b.iter(|| {
                    let _ = eng.analyze_orderbook(slot, orders.clone(), slot, test_pair());
                });
            },
        );
    }
    group.finish();
}

// ─── Tek emir çifti — minimal latency ────────────────────────────────────────

fn bench_auction_single_pair(c: &mut Criterion) {
    c.bench_function("auction_single_buy_sell_pair", |b| {
        let eng = BatchAuctionEngine::with_defaults();
        let mut slot_n = 20_000_000u64;

        b.iter(|| {
            let orders = vec![
                make_order(OrderSide::Buy, 5_000_000, 200, 1, slot_n),
                make_order(OrderSide::Sell, 5_000_000, 195, 2, slot_n),
            ];
            let slot = SlotId(slot_n);
            let _ = eng.process_slot(slot, orders, slot, test_pair());
            slot_n += 1;
        });
    });
}

criterion_group!(
    benches,
    bench_auction_process_slot,
    bench_auction_analyze_orderbook,
    bench_auction_single_pair,
);
criterion_main!(benches);
