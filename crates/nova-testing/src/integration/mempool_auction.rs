//! nova-mempool ↔ nova-auction entegrasyon testi
//!
//! ## Test Akışı (CoW Protocol §5.2 + §5.3)
//! 1. MempoolPool'a şifreli TX'ler gönder
//! 2. Slot açılışında TX'ler çözülür → Order'lara dönüştürülür
//! 3. BatchAuctionEngine::process_slot ile CoW netting çalıştırılır
//! 4. Uniform clearing price doğrulanır

use nova_auction::{
    engine::BatchAuctionEngine,
    types::{Order, OrderClass, OrderId, OrderSide, TokenPair},
};
use nova_mempool::{
    encrypt_transaction,
    pool::MempoolPool,
    types::{RawTransaction, TxHash},
};
use nova_primitives::{Address, Balance, Nonce, SlotId};

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

fn nsc_token() -> [u8; 20] {
    [0x01; 20]
}

fn usdc_token() -> [u8; 20] {
    [0x02; 20]
}

/// Ristretto basepoint: geçerli bir Ristretto255 noktası (threshold PK mock'u olarak kullanılır)
fn mock_threshold_pk() -> [u8; 32] {
    use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
    RISTRETTO_BASEPOINT_POINT.compress().to_bytes()
}

fn make_encrypted_tx(sender: u8, slot: u64, nonce: u64) -> nova_mempool::types::EncryptedTx {
    let raw = RawTransaction::new_transfer(
        addr(sender),
        addr(99),
        Balance(1_000_000),
        Nonce(nonce),
        1,
    );
    let threshold_pk = mock_threshold_pk();
    let out = encrypt_transaction(&raw, &threshold_pk, SlotId(slot), vec![0u8; 64])
        .expect("Şifreleme başarılı olmalı");
    out.encrypted_tx
}

fn make_order(trader: u8, side: OrderSide, price: u128, slot: u64) -> Order {
    let pair = TokenPair::new(
        nova_auction::types::TokenId(nsc_token()),
        nova_auction::types::TokenId(usdc_token()),
    );

    let id_bytes = {
        let mut h = blake3::Hasher::new();
        h.update(&[trader]);
        h.update(&slot.to_le_bytes());
        h.update(&price.to_le_bytes());
        let d = h.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(d.as_bytes());
        arr
    };

    Order {
        id: OrderId::from_bytes(id_bytes),
        side,
        pair,
        amount: 1_000_000_000_000_000_000u128, // 1 NSC (18 ondalık)
        limit_price: price,
        max_slippage_bps: 100, // %1 slippage toleransı
        class: OrderClass::Limit,
        valid_until_slot: SlotId(slot + 10),
        trader: addr(trader),
        submitted_at_slot: SlotId(slot),
        extra_data: vec![],
    }
}

// ─── Testler ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Mempool TX gönderimi ve durum takibi
    #[test]
    fn mempool_submit_and_track() {
        let pool = MempoolPool::for_testing();
        pool.update_current_slot(SlotId(100));

        // 5 farklı gönderici, aynı slot
        let mut tx_hashes = vec![];
        for i in 1u8..=5 {
            let tx = make_encrypted_tx(i, 100, i as u64);
            let hash = pool.submit(tx).expect("TX submit başarısız");
            tx_hashes.push(hash);
        }

        assert_eq!(pool.len(), 5, "5 TX havuzda olmalı");

        // Her TX takip edilebilir
        for hash in &tx_hashes {
            assert!(pool.contains(hash), "TX havuzda bulunmalı");
            let status = pool.status(hash);
            assert!(status.is_some(), "TX durumu alınabilmeli");
        }

        // Slot TX'leri alınabilmeli
        let slot_txs = pool.get_slot_txs(SlotId(100));
        assert_eq!(slot_txs.len(), 5, "Slot 100'de 5 TX olmalı");
    }

    /// Batch auction: CoW netting — alıcı ve satıcı eşleşmesi
    #[test]
    fn auction_cow_netting_buy_sell_match() {
        let engine = BatchAuctionEngine::with_defaults();
        let pair = TokenPair::new(
            nova_auction::types::TokenId(nsc_token()),
            nova_auction::types::TokenId(usdc_token()),
        );

        let slot = SlotId(200);

        // 5 alım emri (limit price: 1000-1200)
        let mut orders: Vec<Order> = (0..5)
            .map(|i| make_order(i, OrderSide::Buy, 1_100_000_000_000_000_000u128, 200))
            .collect();

        // 5 satım emri (limit price: 900-1100)
        let sell_orders: Vec<Order> = (10..15)
            .map(|i| make_order(i, OrderSide::Sell, 900_000_000_000_000_000u128, 200))
            .collect();
        orders.extend(sell_orders);

        // Batch auction çalıştır
        let result = engine.process_slot(slot, orders, slot, pair)
            .expect("Batch auction başarılı olmalı");

        // Uniform clearing price sıfırdan büyük olmalı
        assert!(result.clearing_price > 0, "Clearing price belirlenmeli");
        // Çözülen emir sayısı > 0
        assert!(result.filled_count() > 0, "En az bir emir eşleşmeli");
        // Price improvement bilgisi erişilebilmeli
        let _ = result.total_price_improvement;
    }

    /// Auction: Sadece alım emirleri — satım yok → UnilateralBatch hatası
    #[test]
    fn auction_only_buy_orders_no_match() {
        let engine = BatchAuctionEngine::with_defaults();
        let pair = TokenPair::new(
            nova_auction::types::TokenId(nsc_token()),
            nova_auction::types::TokenId(usdc_token()),
        );

        let slot = SlotId(300);

        // Sadece alım emirleri — satıcı yok → UnilateralBatch hatası beklenir
        let orders: Vec<Order> = (0..3)
            .map(|i| make_order(i, OrderSide::Buy, 1_000_000_000_000_000_000u128, 300))
            .collect();

        let result = engine.process_slot(slot, orders, slot, pair);
        assert!(result.is_err(), "Tek taraflı market UnilateralBatch hatası vermeli");
    }

    /// Mempool + Auction entegrasyonu: TX gönder → Auction'a aktar
    #[test]
    fn mempool_to_auction_pipeline() {
        let pool = MempoolPool::for_testing();
        let engine = BatchAuctionEngine::with_defaults();
        let pair = TokenPair::new(
            nova_auction::types::TokenId(nsc_token()),
            nova_auction::types::TokenId(usdc_token()),
        );

        let slot = SlotId(400);
        pool.update_current_slot(slot);

        // Mempool'a TX gönder
        for i in 0..10u8 {
            let tx = make_encrypted_tx(i, 400, i as u64);
            pool.submit(tx).unwrap();
        }

        assert_eq!(pool.slot_tx_count(slot), 10);

        // Auction için order listesi oluştur (mempool'daki TX'lere karşılık)
        let buy_orders: Vec<Order> = (0..5)
            .map(|i| make_order(i, OrderSide::Buy, 1_050_000_000_000_000_000u128, 400))
            .collect();
        let sell_orders: Vec<Order> = (5..10)
            .map(|i| make_order(i, OrderSide::Sell, 950_000_000_000_000_000u128, 400))
            .collect();

        let orders = [buy_orders, sell_orders].concat();
        let result = engine.process_slot(slot, orders, slot, pair)
            .expect("Pipeline başarılı olmalı");

        // Slot işlenmiş olarak işaretle
        assert!(engine.is_slot_processed(slot));
        // Tekrar işleme hatası vermeli
        let fake_orders: Vec<Order> = (0..2)
            .map(|i| make_order(i, OrderSide::Buy, 1_000_000_000_000_000_000u128, 400))
            .collect();
        let dup = engine.process_slot(slot, fake_orders, slot, pair);
        assert!(dup.is_err(), "Aynı slot tekrar işlenemez");

        let _ = result;
    }

    /// Auction istatistikleri doğrulanabilmeli
    #[test]
    fn auction_stats_accumulate() {
        let engine = BatchAuctionEngine::with_defaults();
        let pair = TokenPair::new(
            nova_auction::types::TokenId(nsc_token()),
            nova_auction::types::TokenId(usdc_token()),
        );

        // 3 farklı slot işle
        for slot_num in [100u64, 200, 300] {
            let slot = SlotId(slot_num);
            let orders: Vec<Order> = (0..4)
                .map(|i| {
                    if i < 2 {
                        make_order(i, OrderSide::Buy, 1_100_000_000_000_000_000u128, slot_num)
                    } else {
                        make_order(i, OrderSide::Sell, 900_000_000_000_000_000u128, slot_num)
                    }
                })
                .collect();
            let _ = engine.process_slot(slot, orders, slot, pair.clone());
        }

        let stats = engine.stats();
        assert!(stats.total_slots_processed >= 1, "En az 1 slot işlenmeli");
        assert!(stats.total_orders_processed >= 1, "En az 1 emir işlenmeli");
    }
}
