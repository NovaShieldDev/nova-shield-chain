//! Tam node pipeline entegrasyon testi
//!
//! ## Test Akışı (Nova Shield Chain §5 tam slot simülasyonu)
//! 1. Slot başlar: BftEngine + MempoolPool + BatchAuctionEngine + ParallelExecutor hazır
//! 2. Mempool: şifreli TX'ler gelir
//! 3. Auction: slot sonunda batch çalışır → uniform price
//! 4. Runtime: TX'ler execute edilir
//! 5. Consensus: validator'lar oy verir → QC
//! 6. Network: blok shred'lere bölünür → RS encode
//!
//! Bu test tüm crate'lerin birlikte çalışmasını doğrular.

use nova_auction::{
    engine::BatchAuctionEngine,
    types::{Order, OrderClass, OrderId, OrderSide, TokenPair},
};
use nova_consensus::{
    bft::{BftEngine, Vote},
    types::BlockHeader,
    BftSigner,
};
use nova_crypto::BlsKeypair;
use nova_primitives::BlsPublicKey;
use nova_mempool::pool::MempoolPool;
use nova_network::turbine::erasure::{ErasureConfig, ErasureEncoder};
use nova_network::turbine::shred::{Shred, ShredType};
use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
use nova_primitives::{
    Address, BlockHash, EpochId, SlotId, ValidatorId, crypto::Hash,
};
use nova_runtime::{
    executor::ParallelExecutor,
    state::AccountStateCache,
    tx::{compute_tx_hash, Transaction, TransactionKind},
};
use nova_vm::api::{CallInput, DeployInput, NovaVmExecutor};
use std::sync::Arc;

// ─── Sabitleri ────────────────────────────────────────────────────────────────

const NUM_VALIDATORS: usize = 5;
const TEST_SLOT: u64 = 10_000;

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

fn vid(n: u64) -> ValidatorId {
    ValidatorId(n)
}

fn make_vote(v: ValidatorId, bh: BlockHash, height: u64, slot: u64) -> Vote {
    let mut sig = [0u8; 64];
    let h = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(bh.as_bytes());
        hasher.update(&v.0.to_le_bytes());
        hasher.update(&height.to_le_bytes());
        hasher.finalize()
    };
    sig[..32].copy_from_slice(h.as_bytes());
    Vote { validator: v, block_hash: bh, height, signature: sig, slot: SlotId(slot) }
}

/// Seed'den ECDSA imzalı transfer TX'i üret; türetilen sender adresini de döndür.
fn signed_transfer(seed: u8, to: Address, amount: u128, nonce: u64) -> (Transaction, Address) {
    let mut seed_bytes = [0u8; 32];
    seed_bytes[0] = seed;
    let kp = EcdsaKeypair::from_secret_bytes(&seed_bytes).expect("geçerli seed");
    let pk = kp.public_key().expect("pubkey");
    let sender = public_key_to_address(&pk);

    let mut tx = Transaction {
        sender,
        nonce,
        kind: TransactionKind::Transfer { to, amount },
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 100_000_000,
        gas_limit: 21_000,
        signature: Vec::new(),
        slot: SlotId(0),
    };

    let hash_bytes = compute_tx_hash(&tx);
    let hash = Hash::from_bytes(hash_bytes);
    let sig = kp.sign_hash(&hash).expect("sign");
    tx.signature = sig.as_bytes().to_vec();
    (tx, sender)
}

fn bls_ikm_for(vid: ValidatorId) -> [u8; 32] {
    [(vid.0 as u8).wrapping_mul(37); 32]
}

fn make_bls_engine(n: u64) -> BftEngine {
    let validators: Vec<(ValidatorId, BlsPublicKey)> = (1..=n)
        .map(|i| {
            let v = ValidatorId(i);
            let kp = BlsKeypair::from_ikm(&bls_ikm_for(v)).unwrap();
            (v, kp.bls_public_key())
        })
        .collect();
    BftEngine::new_with_pubkeys(validators)
}

fn signed_vote(v: ValidatorId, bh: BlockHash, height: u64, slot: u64) -> nova_consensus::SignedVote {
    let signer = BftSigner::from_ikm(v, &bls_ikm_for(v)).unwrap();
    signer.sign_vote(bh, height, SlotId(slot)).unwrap()
}

fn make_token(n: u8) -> nova_auction::types::TokenId {
    nova_auction::types::TokenId([n; 20])
}

fn make_order(trader: u8, side: OrderSide, price: u128, slot: u64) -> Order {
    let id_bytes = {
        let mut h = blake3::Hasher::new();
        h.update(&[trader]);
        h.update(&slot.to_le_bytes());
        *h.finalize().as_bytes()
    };
    Order {
        id: OrderId::from_bytes(id_bytes),
        side,
        pair: TokenPair::new(make_token(1), make_token(2)),
        amount: 1_000_000_000_000_000_000u128,
        limit_price: price,
        max_slippage_bps: 200,
        class: OrderClass::Limit,
        valid_until_slot: SlotId(slot + 5),
        trader: addr(trader),
        submitted_at_slot: SlotId(slot),
        extra_data: vec![],
    }
}

fn minimal_wasm() -> Vec<u8> {
    wat::parse_str(r#"
        (module
            (import "env" "nova_storage_write" (func $sw (param i32 i32 i32 i32)))
            (import "env" "nova_storage_read"  (func $sr (param i32 i32 i32) (result i32)))
            (import "env" "nova_emit_event"    (func $ee (param i32 i32)))
            (memory (export "memory") 1)
            (func (export "instantiate") (param i32 i32) (result i32) (i32.const 0))
            (func (export "execute") (param $ip i32) (param $il i32) (result i32)
                (call $ee (local.get $ip) (local.get $il))
                (i32.const 2)
            )
            (func (export "query") (param $kp i32) (param $kl i32) (result i32)
                (local $r i32)
                (local.set $r (call $sr (local.get $kp) (local.get $kl) (i32.const 4096)))
                (select (local.get $r) (i32.const 0) (i32.ge_s (local.get $r) (i32.const 0)))
            )
        )
    "#).expect("WAT parse hatası")
}

// ─── Testler ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Tam slot pipeline: mempool → auction → runtime → consensus → network
    #[test]
    fn full_slot_pipeline() {
        // ── 1. Bileşenleri başlat ─────────────────────────────────────────────
        let bft = make_bls_engine(NUM_VALIDATORS as u64);

        let mempool = MempoolPool::default_pool();
        let slot = SlotId(TEST_SLOT);
        mempool.update_current_slot(slot);

        let auction = BatchAuctionEngine::with_defaults();

        let state = Arc::new(AccountStateCache::new());
        // Hesaplara başlangıç bakiyesi ver
        for i in 1u8..=10 {
            state.set(addr(i), nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
        }
        let runtime = ParallelExecutor::new(state.clone(), 1_000_000_000u128);

        let vm = NovaVmExecutor::new();

        // ── 2. Mempool: TX'ler gönder ────────────────────────────────────────
        // İmzalı transfer TX'leri oluştur; sender adresleri ECDSA keypair'den türer
        let (tx1, s1) = signed_transfer(10, addr(200), 1_000_000_000_000_000_000u128, 0);
        let (tx2, s2) = signed_transfer(11, addr(201), 500_000_000_000_000_000u128, 0);
        let (tx3, s3) = signed_transfer(12, addr(202), 2_000_000_000_000_000_000u128, 0);
        state.set(s1, nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
        state.set(s2, nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
        state.set(s3, nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
        let txs = vec![tx1, tx2, tx3];

        // ── 3. Auction: Slot işleme ───────────────────────────────────────────
        let pair = TokenPair::new(make_token(1), make_token(2));
        let orders: Vec<Order> = (1u8..=6)
            .map(|i| {
                if i <= 3 {
                    make_order(i, OrderSide::Buy, 1_100_000_000_000_000_000u128, TEST_SLOT)
                } else {
                    make_order(i, OrderSide::Sell, 900_000_000_000_000_000u128, TEST_SLOT)
                }
            })
            .collect();

        let auction_result = auction
            .process_slot(slot, orders, slot, pair)
            .expect("Auction başarılı olmalı");

        assert!(auction_result.clearing_price > 0, "Clearing price belirlenmeli");

        // ── 4. Runtime: TX'leri çalıştır ─────────────────────────────────────
        let exec_results = runtime.execute_block(&txs);
        assert_eq!(exec_results.len(), 3, "3 TX sonucu");
        for r in &exec_results {
            assert!(r.success, "TX başarılı olmalı");
        }

        // ── 5. VM: Kontrat deploy + call ──────────────────────────────────────
        let bytecode = minimal_wasm();
        vm.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(200),
            bytecode,
            init_msg: vec![],
            label: "pipeline-contract".into(),
            gas_limit: 2_000_000,
            slot: TEST_SLOT,
        }).expect("Deploy başarısız");

        let call_result = vm.call(CallInput {
            caller: addr(1),
            contract: addr(200),
            method: "execute".into(),
            input: b"pipeline_data".to_vec(),
            value: 0,
            gas_limit: 300_000,
            slot: TEST_SLOT,
        }).expect("Call başarısız");
        assert!(call_result.success, "VM call başarılı olmalı");

        // ── 6. Consensus: BFT oylaması ───────────────────────────────────────
        let block_hash = {
            let header = BlockHeader {
                height: 100,
                slot,
                epoch: EpochId(1),
                parent_hash: BlockHash::from_bytes([0u8; 32]),
                state_root: [0u8; 32],
                proposer: vid(1),
                timestamp: 1_700_000_000 + TEST_SLOT,
                tx_count: txs.len() as u32,
            };
            header.compute_hash()
        };

        let mut qc_opt = None;
        for i in 1u64..=4 {
            let sv = signed_vote(vid(i), block_hash, 100, TEST_SLOT);
            qc_opt = bft.process_signed_vote(sv, slot).expect("Oy işlenmeli");
        }
        let qc = qc_opt.expect("QC üretilmeli");
        assert!(bft.verify_qc(&qc).is_ok(), "QC geçerli olmalı");

        // ── 7. Network: RS Encoding ───────────────────────────────────────────
        let config = ErasureConfig::light_16_8();
        let encoder = ErasureEncoder::new(config);

        // Blok verisini shredlere böl
        let block_data = b"serialized_block_data_for_slot_10000".repeat(100);
        let chunks: Vec<Vec<u8>> = block_data
            .chunks(1024)
            .take(16)
            .map(|c| {
                let mut v = c.to_vec();
                v.resize(1024, 0);
                v
            })
            .collect();

        let shreds: Vec<Shred> = chunks
            .iter()
            .enumerate()
            .map(|(i, data)| Shred::new(TEST_SLOT, i as u32, ShredType::Data, [0u8; 32], data.clone()))
            .collect();

        let code_shreds = encoder.encode(TEST_SLOT, 0, &shreds);
        assert_eq!(code_shreds.len(), 8, "16 data → 8 code shred");

        // ── Pipeline tamamlandı ───────────────────────────────────────────────
        println!(
            "Pipeline tamamlandı: slot={}, clearing_price={}, qc_signers={}, code_shreds={}",
            TEST_SLOT,
            auction_result.clearing_price,
            qc.signers.len(),
            code_shreds.len()
        );
    }

    /// Runtime + VM: Kontrat state izolasyonu
    #[test]
    fn vm_contract_state_isolation() {
        let vm = NovaVmExecutor::new();
        let bytecode = minimal_wasm();

        // 3 farklı kontrat deploy
        for i in 0u8..3 {
            vm.deploy(DeployInput {
                deployer: addr(1),
                contract_address: addr(220 + i),
                bytecode: bytecode.clone(),
                init_msg: vec![],
                label: format!("isolated-{i}"),
                gas_limit: 2_000_000,
                slot: TEST_SLOT + i as u64,
            }).unwrap();
        }

        // Her kontrat bağımsız çalışır
        for i in 0u8..3 {
            let result = vm.call(CallInput {
                caller: addr(1),
                contract: addr(220 + i),
                method: "execute".into(),
                input: format!("data_{i}").into_bytes(),
                value: 0,
                gas_limit: 300_000,
                slot: TEST_SLOT + 100,
            }).unwrap();
            assert!(result.success, "Kontrat {i} başarılı olmalı");
        }
    }

    /// BFT + Network: Quorum kararı → blok yayınlanır
    #[test]
    fn consensus_quorum_then_network_broadcast() {
        let bft = make_bls_engine(5);

        // Blok hash'i
        let bh = BlockHash::from_bytes([0xAB; 32]);
        let height = 50u64;
        let slot = TEST_SLOT + 500;

        // 4 validator gerçek BLS imzalı oy verir → QC
        let mut qc = None;
        for i in 1u64..=4 {
            let sv = signed_vote(vid(i), bh, height, slot);
            qc = bft.process_signed_vote(sv, SlotId(slot)).unwrap();
        }
        let qc = qc.expect("QC üretilmeli");
        assert!(bft.verify_qc(&qc).is_ok());

        // QC sonrası blok shredlere bölünür
        let config = ErasureConfig::light_16_8();
        let encoder = ErasureEncoder::new(config);

        let shreds: Vec<Shred> = (0u32..16)
            .map(|i| Shred::new(slot, i, ShredType::Data, [0u8; 32], vec![i as u8; 1024]))
            .collect();

        let code_shreds = encoder.encode(slot, 0, &shreds);
        assert_eq!(code_shreds.len(), 8);

        println!("QC signers: {}, code shreds: {}", qc.signers.len(), code_shreds.len());
    }

    /// Auction + Runtime: Clearing sonrası settlement
    #[test]
    fn auction_then_runtime_settlement() {
        let state = Arc::new(AccountStateCache::new());
        for i in 1u8..=10 {
            state.set(addr(i), nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
        }
        let runtime = ParallelExecutor::new(state.clone(), 0u128);

        let auction = BatchAuctionEngine::with_defaults();
        let slot = SlotId(TEST_SLOT + 1000);
        let pair = TokenPair::new(make_token(10), make_token(20));

        let orders: Vec<Order> = (1u8..=6)
            .map(|i| {
                if i <= 3 {
                    make_order(i, OrderSide::Buy, 1_000_000_000_000_000_000u128, TEST_SLOT + 1000)
                } else {
                    make_order(i, OrderSide::Sell, 1_000_000_000_000_000_000u128, TEST_SLOT + 1000)
                }
            })
            .collect();

        let result = auction.process_slot(slot, orders, slot, pair).expect("Auction başarısız");
        assert!(result.filled_count() > 0, "Eşleşen emir olmalı");

        // Settlement TX'leri — imzalı; ECDSA sender adreslerini fund et
        let settlement_amount = result.clearing_price / 10;
        let mut settlement_txs = Vec::new();
        for i in 1u8..=3 {
            let (tx, sender) = signed_transfer(20 + i, addr(200 + i), settlement_amount, 0);
            state.set(sender, nova_runtime::state::AccountState::new_eoa(100_000_000_000_000_000_000u128));
            settlement_txs.push(tx);
        }

        let exec_results = runtime.execute_block(&settlement_txs);
        for r in &exec_results {
            assert!(r.success, "Settlement TX başarılı olmalı: {:?}", r.error);
        }
    }
}
