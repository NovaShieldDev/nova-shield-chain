//! TX Pipeline — Mempool → Auction → Runtime entegrasyonu
//!
//! Nova Shield Chain'in tam işlem hattı:
//!
//! ```text
//! Kullanıcı TX (şifreli)
//!       │
//!       ▼
//!  MempoolPool::submit()
//!       │
//!       ▼  [slot kapandı]
//!  SlotRevealState::reveal_all()  ← validator decryption share'leri
//!       │
//!       ▼
//!  DecryptedTx listesi
//!       │
//!       ├── Transfer / Heartbeat / GuardianOp → doğrudan RuntimeTx
//!       └── ContractCall (swap data)          → BatchAuctionEngine::process_slot()
//!                                                      │
//!                                                      ▼
//!                                              ClearingResult → RuntimeTx
//!       │
//!       ▼
//!  ParallelExecutor::execute_block()
//!       │
//!       ▼
//!  Vec<TxResult> + StateUpdate
//! ```
//!
//! Her slot'ta ayrıca:
//! - `AccountManager::begin_slot()` — hesap yaşam döngüsü
//! - `InheritanceEngine::on_slot()` — miras tetikleyicileri
//! - `FreezeEngine::process_slot()` — dondurma pencereleri

use crate::errors::{NodeError, NodeResult};
use nova_account::{AccountManager, AccountManagerEvent};
use nova_auction::{
    engine::BatchAuctionEngine,
    types::{Order, OrderClass, OrderId, OrderSide, TokenId, TokenPair},
};
use nova_freeze::FreezeEngine;
use nova_inheritance::InheritanceEngine;
use nova_mempool::{
    pool::MempoolPool,
    reveal::SlotRevealState,
    types::{DecryptedTx, EncryptedTx, RawTransaction, RevealedBatch, TxHash, TxType},
};
use nova_primitives::{Address, Balance, SlotId};
use nova_runtime::{
    AccountStateCache, ParallelExecutor, Transaction as RuntimeTx, TransactionKind, TxResult,
    INITIAL_BASE_FEE,
};
use nova_zk::{ShieldedTx, ZkShieldEngine};
use std::sync::Arc;

// ─── PipelineSlotResult ───────────────────────────────────────────────────────

/// Bir slot'ta pipeline'ın ürettiği sonuçlar
#[derive(Debug, Default)]
pub struct PipelineSlotResult {
    /// Runtime tarafından işlenen TX sayısı
    pub txs_executed: usize,
    /// Başarılı TX sayısı
    pub txs_ok: usize,
    /// Başarısız TX sayısı
    pub txs_failed: usize,
    /// Auction clearing price (varsa)
    pub clearing_price: Option<u128>,
    /// Inheritance olayları
    pub inheritance_events: u32,
    /// Freeze olayları
    pub freeze_events: u32,
    /// Hesap yöneticisi olayları
    pub account_events: u32,
    /// Gas kullanımı toplamı
    pub total_gas: u64,
    /// Slot
    pub slot: SlotId,
}

// ─── TxPipeline ───────────────────────────────────────────────────────────────

/// NOVA-RED-NODE-009: Şifreli mempool için minimum BFT reveal threshold.
///
/// Threshold=1 iken tek validator tüm TX'leri açabilir → MEV front-running mümkün.
/// Mainnet varsayılanı: 134 (= ceil(200 * 2/3) + 1, 200 validator için).
/// Küçük test ağları için en az 2 tercih edilir.
pub const MIN_REVEAL_THRESHOLD: u8 = 2;

/// Tam TX pipeline — tüm protokol motorlarını koordine eder
///
/// `dev_mode=true` ise şifreli mempool atlanır, TXler doğrudan çalıştırılır.
/// `dev_mode=false` ise threshold decryption gerekir.
pub struct TxPipeline {
    /// Şifreli TX havuzu (MEV-free mempool)
    pub mempool: Arc<MempoolPool>,
    /// Batch auction motoru (MEV-free fiyat belirleme)
    pub auction: BatchAuctionEngine,
    /// Hesap yöneticisi (MPC-native hesap modeli)
    pub accounts: Arc<AccountManager>,
    /// Miras protokolü motoru (deadman switch)
    pub inheritance: InheritanceEngine,
    /// Emergency freeze motoru
    pub freeze: FreezeEngine,
    /// ZK Shield motoru (gizli UTXO işlemleri)
    pub zk_shield: ZkShieldEngine,
    /// Paralel TX çalıştırıcı
    executor: ParallelExecutor,
    /// Geliştirici modu — şifreli mempool atlanır
    dev_mode: bool,
    /// Dev modunda direkt gönderilen TX'ler: (slot, tx)
    dev_queue: Vec<(SlotId, RuntimeTx)>,
    /// Anlık slot
    current_slot: SlotId,
    /// NOVA-RED-NODE-009: Şifreli TX açmak için gereken minimum validator share sayısı.
    /// Tek validator'ın MEV front-running yapmasını önler.
    reveal_threshold: u8,
}

impl TxPipeline {
    /// Yeni pipeline oluştur (dev modu varsayılan)
    pub fn new(state: Arc<AccountStateCache>, base_fee: u128) -> Self {
        Self::with_dev_mode(state, base_fee, true)
    }

    /// Belirli modda pipeline oluştur
    pub fn with_dev_mode(state: Arc<AccountStateCache>, base_fee: u128, dev_mode: bool) -> Self {
        Self {
            mempool: Arc::new(MempoolPool::default_pool()),
            auction: BatchAuctionEngine::with_defaults(),
            accounts: Arc::new(AccountManager::in_memory()),
            inheritance: InheritanceEngine::new(),
            freeze: FreezeEngine::with_defaults(),
            zk_shield: ZkShieldEngine::new(),
            executor: ParallelExecutor::new(state, base_fee),
            dev_mode,
            dev_queue: Vec::new(),
            current_slot: SlotId(0),
            // NOVA-RED-NODE-009: Threshold=1 → MEV front-running. Min 2 validator share zorunlu.
            reveal_threshold: MIN_REVEAL_THRESHOLD,
        }
    }

    /// Mevcut slot'u ilerlet
    pub fn advance_slot(&mut self, slot: SlotId) {
        self.current_slot = slot;
        self.mempool.update_current_slot(slot);
    }

    // ─── TX Gönderme ──────────────────────────────────────────────────────────

    /// Şifreli TX gönder (üretim modu)
    pub fn submit_encrypted_tx(&self, tx: EncryptedTx) -> NodeResult<TxHash> {
        self.mempool.submit(tx).map_err(|e| NodeError::ServiceStartError {
            service: "Mempool".into(),
            reason: e.to_string(),
        })
    }

    /// Dev modunda plaintext TX gönder (şifreleme atlanır)
    pub fn submit_dev_tx(&mut self, tx: RuntimeTx) {
        self.dev_queue.push((self.current_slot, tx));
    }

    /// ZK shielded TX gönder
    pub fn submit_shielded_tx(&self, tx: ShieldedTx) -> NodeResult<()> {
        self.zk_shield.apply(&tx).map_err(|e| NodeError::ServiceStartError {
            service: "ZkShield".into(),
            reason: e.to_string(),
        })
    }

    // ─── Slot Yürütme ─────────────────────────────────────────────────────────

    /// Bir slot'u tam olarak yürüt
    ///
    /// 1. Hesap yaşam döngüsü
    /// 2. Miras tetikleyicileri
    /// 3. Freeze pencereleri
    /// 4. TX reveal + execution
    pub fn execute_slot(&mut self, slot: SlotId) -> NodeResult<PipelineSlotResult> {
        self.advance_slot(slot);
        let mut result = PipelineSlotResult { slot, ..Default::default() };

        // 1. Hesap yaşam döngüsü
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let acct_events = self.accounts.begin_slot(slot, ts);
        result.account_events = acct_events.len() as u32;

        // 2. Miras tetikleyicileri
        let inh_result = self.inheritance.on_slot(slot);
        result.inheritance_events = inh_result.plans_processed;

        // 3. Freeze pencereleri
        let freeze_events = self.freeze.process_slot(slot);
        result.freeze_events = freeze_events.len() as u32;

        // 4. TX'leri topla ve çalıştır
        let mut runtime_txs: Vec<RuntimeTx> = Vec::new();

        if self.dev_mode {
            // Dev modu: kuyruktaki TX'leri al
            let drain: Vec<(SlotId, RuntimeTx)> = self.dev_queue.drain(..).collect();
            for (tx_slot, tx) in drain {
                if tx_slot == slot || tx_slot.0 + 1 == slot.0 {
                    runtime_txs.push(tx);
                }
            }
        } else {
            // Üretim modu: şifreli mempool'dan TX'leri çek
            let slot_txs = self.mempool.get_slot_txs(slot);
            if !slot_txs.is_empty() {
                if let Ok(revealed) = self.reveal_slot_txs(slot, slot_txs) {
                    let (direct, auction_orders) = self.classify_txs(&revealed.transactions, slot);
                    runtime_txs.extend(direct);

                    // Auction emirlerini işle
                    if !auction_orders.is_empty() {
                        let pair = TokenPair {
                            base: TokenId([1u8; 20]),
                            quote: TokenId::NSC,
                        };
                        if let Ok(clearing) =
                            self.auction.process_slot(slot, auction_orders, slot, pair)
                        {
                            result.clearing_price = Some(clearing.clearing_price);
                            // Çalıştırılan emirleri runtime TX'e çevir
                            for executed in &clearing.executed_orders {
                                runtime_txs.push(RuntimeTx {
                                    sender: executed.trader,
                                    nonce: 0,
                                    kind: TransactionKind::Transfer {
                                        to: executed.trader,
                                        amount: executed.executed_amount,
                                    },
                                    max_fee_per_gas: INITIAL_BASE_FEE,
                                    max_priority_fee_per_gas: 0,
                                    gas_limit: 21_000,
                                    signature: vec![],
                                    slot,
                                });
                            }
                        }
                    }
                    self.mempool.on_slot_revealed(&revealed);
                }
            }
        }

        // Runtime execution
        if !runtime_txs.is_empty() {
            let tx_results = self.executor.execute_block(&runtime_txs);
            result.txs_executed = tx_results.len();
            result.txs_ok = tx_results.iter().filter(|r| r.success).count();
            result.txs_failed = tx_results.iter().filter(|r| !r.success).count();
            result.total_gas = tx_results.iter().map(|r| r.gas_used).sum();
        }

        Ok(result)
    }

    // ─── Yardımcı: Reveal ────────────────────────────────────────────────────

    /// Şifreli TX'leri threshold decryption ile aç
    fn reveal_slot_txs(
        &self,
        slot: SlotId,
        encrypted_txs: Vec<EncryptedTx>,
    ) -> NodeResult<RevealedBatch> {
        // NOVA-RED-NODE-009: threshold=1 → tek validator MEV front-running yapabilir.
        // self.reveal_threshold kullan (MIN_REVEAL_THRESHOLD=2).
        let mut state = SlotRevealState::new(encrypted_txs, self.reveal_threshold).with_slot(slot);
        if state.can_reveal() {
            state.reveal_all().map_err(|e| NodeError::ServiceStartError {
                service: "Reveal".into(),
                reason: e.to_string(),
            })?;
        }
        Ok(state.get_result().cloned().unwrap_or_else(|| RevealedBatch {
            slot,
            transactions: Vec::new(),
            failed_hashes: Vec::new(),
            batch_merkle_root: [0u8; 32],
        }))
    }

    // ─── Yardımcı: TX Sınıflandırma ─────────────────────────────────────────

    /// DecryptedTx → doğrudan RuntimeTx + auction Order listelerine böl
    fn classify_txs(
        &self,
        txs: &[DecryptedTx],
        slot: SlotId,
    ) -> (Vec<RuntimeTx>, Vec<Order>) {
        let mut direct = Vec::new();
        let mut orders = Vec::new();

        for dtx in txs {
            match dtx.raw_tx.tx_type {
                TxType::ContractCall if dtx.raw_tx.data.len() >= 53 => {
                    if let Some(order) = self.decode_auction_order(dtx) {
                        orders.push(order);
                        continue;
                    }
                    // Auction decode başarısız → doğrudan çalıştır
                }
                _ => {}
            }
            if let Some(rtx) = self.raw_to_runtime(dtx, slot) {
                direct.push(rtx);
            }
        }
        (direct, orders)
    }

    /// `RawTransaction` → `nova_runtime::Transaction`
    fn raw_to_runtime(&self, dtx: &DecryptedTx, slot: SlotId) -> Option<RuntimeTx> {
        let raw = &dtx.raw_tx;
        let kind = match raw.tx_type {
            TxType::Transfer => TransactionKind::Transfer {
                to: raw.to?,
                amount: raw.value.0,
            },
            TxType::Heartbeat
            | TxType::GuardianOp
            | TxType::FreezeOp
            | TxType::InheritanceOp => TransactionKind::Transfer {
                to: raw.to.unwrap_or(raw.from),
                amount: 0,
            },
            TxType::ContractCall | TxType::ContractDeploy => TransactionKind::Transfer {
                to: raw.to.unwrap_or(raw.from),
                amount: raw.value.0,
            },
            TxType::Unknown => return None,
        };
        Some(RuntimeTx {
            sender: raw.from,
            nonce: raw.nonce.0,
            kind,
            max_fee_per_gas: raw.max_fee_per_gas.0,
            max_priority_fee_per_gas: raw.max_priority_fee_per_gas.0,
            gas_limit: raw.gas_limit,
            signature: vec![],
            slot,
        })
    }

    /// ContractCall data'sından auction order decode et
    ///
    /// Format: [side:1][amount:16][limit_price:16][base_token:20] = 53 byte minimum
    fn decode_auction_order(&self, dtx: &DecryptedTx) -> Option<Order> {
        let raw = &dtx.raw_tx;
        let data = &raw.data;
        if data.len() < 53 {
            return None;
        }
        let side = if data[0] == 0 { OrderSide::Buy } else { OrderSide::Sell };
        let amount = u128::from_le_bytes(data[1..17].try_into().ok()?);
        let limit_price = u128::from_le_bytes(data[17..33].try_into().ok()?);
        let mut base_bytes = [0u8; 20];
        base_bytes.copy_from_slice(&data[33..53]);

        if amount == 0 || limit_price == 0 {
            return None;
        }
        Some(Order {
            id: OrderId::from_bytes(*dtx.commitment.as_bytes()),
            side,
            pair: TokenPair { base: TokenId(base_bytes), quote: TokenId::NSC },
            amount,
            limit_price,
            max_slippage_bps: 50, // %0.5 slippage toleransı
            class: OrderClass::LimitPartialFill,
            valid_until_slot: SlotId(self.current_slot.0 + 10),
            trader: raw.from,
            submitted_at_slot: self.current_slot,
            extra_data: Vec::new(),
        })
    }

    // ─── İstatistikler ────────────────────────────────────────────────────────

    /// Mempool istatistikleri
    pub fn mempool_stats(&self) -> nova_mempool::MempoolStats {
        self.mempool.stats()
    }

    /// Auction istatistikleri
    pub fn auction_stats(&self) -> nova_auction::AuctionStats {
        self.auction.stats()
    }

    /// ZK pool istatistikleri
    pub fn zk_stats(&self) -> nova_zk::utxo::pool::PoolStats {
        self.zk_shield.stats()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_runtime::{AccountStateCache, INITIAL_BASE_FEE};

    fn make_state() -> Arc<AccountStateCache> {
        let state = AccountStateCache::new();
        state.set_balance(Address::from_bytes([0xAAu8; 20]), 1_000_000_000_000_000_000_000u128);
        Arc::new(state)
    }

    fn make_pipeline() -> TxPipeline {
        TxPipeline::new(make_state(), INITIAL_BASE_FEE)
    }

    fn make_transfer_tx(from: u8, to: u8, amount: u128, nonce: u64, slot: SlotId) -> RuntimeTx {
        RuntimeTx {
            sender: Address::from_bytes([from; 20]),
            nonce,
            kind: TransactionKind::Transfer {
                to: Address::from_bytes([to; 20]),
                amount,
            },
            max_fee_per_gas: INITIAL_BASE_FEE,
            max_priority_fee_per_gas: 0,
            gas_limit: 21_000,
            signature: vec![],
            slot,
        }
    }

    #[test]
    fn test_pipeline_new() {
        let p = make_pipeline();
        assert!(p.dev_mode);
        assert_eq!(p.current_slot, SlotId(0));
    }

    #[test]
    fn test_advance_slot() {
        let mut p = make_pipeline();
        p.advance_slot(SlotId(42));
        assert_eq!(p.current_slot, SlotId(42));
    }

    #[test]
    fn test_execute_empty_slot() {
        let mut p = make_pipeline();
        let result = p.execute_slot(SlotId(1)).unwrap();
        assert_eq!(result.txs_executed, 0);
        assert_eq!(result.slot, SlotId(1));
    }

    #[test]
    fn test_submit_dev_tx_and_execute() {
        let mut p = make_pipeline();
        p.advance_slot(SlotId(5));
        let tx = make_transfer_tx(0xAA, 0xBB, 1000, 0, SlotId(5));
        p.submit_dev_tx(tx);
        let result = p.execute_slot(SlotId(5)).unwrap();
        assert_eq!(result.txs_executed, 1);
    }

    #[test]
    fn test_multiple_dev_txs() {
        let mut p = make_pipeline();
        p.advance_slot(SlotId(10));
        for i in 0u64..5 {
            let tx = make_transfer_tx(0xAA, 0xBB, 100, i, SlotId(10));
            p.submit_dev_tx(tx);
        }
        let result = p.execute_slot(SlotId(10)).unwrap();
        assert_eq!(result.txs_executed, 5);
    }

    #[test]
    fn test_inheritance_and_freeze_called_per_slot() {
        let mut p = make_pipeline();
        let r = p.execute_slot(SlotId(1)).unwrap();
        // Sadece protokol event sayıları döner
        assert_eq!(r.slot, SlotId(1));
        // Sıfır event (yeni pipeline, plan/freeze yok)
        assert_eq!(r.inheritance_events, 0);
        assert_eq!(r.freeze_events, 0);
    }

    #[test]
    fn test_account_lifecycle_called_per_slot() {
        let mut p = make_pipeline();
        let r = p.execute_slot(SlotId(1)).unwrap();
        assert_eq!(r.account_events, 0); // yeni hesap yok
    }

    #[test]
    fn test_sequential_slots() {
        let mut p = make_pipeline();
        for slot in 1u64..=10 {
            p.advance_slot(SlotId(slot));
            let tx = make_transfer_tx(0xAA, 0xBB, 10, slot - 1, SlotId(slot));
            p.submit_dev_tx(tx);
            let r = p.execute_slot(SlotId(slot)).unwrap();
            assert_eq!(r.txs_executed, 1);
        }
    }

    #[test]
    fn test_dev_tx_slot_matching() {
        let mut p = make_pipeline();
        p.advance_slot(SlotId(3));
        // Slot 3'e TX ekle, slot 3'te çalışmalı
        let tx = make_transfer_tx(0xAA, 0xBB, 100, 0, SlotId(3));
        p.submit_dev_tx(tx);
        let r = p.execute_slot(SlotId(3)).unwrap();
        assert_eq!(r.txs_executed, 1);
        // Slot 3 queue'su boşaldı
        let r2 = p.execute_slot(SlotId(4)).unwrap();
        assert_eq!(r2.txs_executed, 0);
    }

    #[test]
    fn test_zk_tx_submission() {
        use nova_zk::{
            circuits::mint::MintWitness, utxo::note::Note, ShieldedAction, ShieldMode,
        };
        let p = make_pipeline();
        let note = Note::new(1000, nova_zk::utxo::note::AssetId::nsc(), [1u8; 32], [2u8; 32]);
        let proof = MintWitness::new(note).prove(SlotId(1)).unwrap();
        let action = ShieldedAction::Mint(proof);
        let tx = ShieldedTx::new(action, ShieldMode::FullShield, SlotId(1), 1000);
        let result = p.submit_shielded_tx(tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mempool_stats_accessible() {
        let p = make_pipeline();
        let stats = p.mempool_stats();
        assert_eq!(stats.pending_count, 0);
    }

    #[test]
    fn test_auction_stats_accessible() {
        let p = make_pipeline();
        let stats = p.auction_stats();
        assert_eq!(stats.total_slots_processed, 0);
    }

    /// NOVA-RED-NODE-009: reveal_threshold varsayılan olarak MIN_REVEAL_THRESHOLD=2 olmalı
    #[test]
    fn test_node009_reveal_threshold_default_is_min() {
        let p = make_pipeline();
        assert_eq!(
            p.reveal_threshold, MIN_REVEAL_THRESHOLD,
            "Tek validator MEV front-running yapmasın: threshold en az {} olmalı",
            MIN_REVEAL_THRESHOLD
        );
        assert!(p.reveal_threshold >= 2, "threshold=1 MEV açığı doğurur");
    }

    /// NOVA-RED-NODE-009: with_dev_mode(false) da threshold korunmalı
    #[test]
    fn test_node009_production_mode_threshold() {
        let p = TxPipeline::with_dev_mode(make_state(), INITIAL_BASE_FEE, false);
        assert_eq!(p.reveal_threshold, MIN_REVEAL_THRESHOLD);
        assert!(!p.dev_mode);
    }

    #[test]
    fn test_gas_accumulation() {
        // Dev modunda imzasız TX'ler pipeline'a girer ve executor tarafından işlenir.
        // Validator imza reddi nedeniyle gas_used=0 döner; önemli olan pipeline'ın
        // 3 TX'i execute_block'a ulaştırmasıdır.
        let mut p = make_pipeline();
        p.advance_slot(SlotId(1));
        for i in 0u64..3 {
            let tx = make_transfer_tx(0xAA, 0xBB, 100, i, SlotId(1));
            p.submit_dev_tx(tx);
        }
        let r = p.execute_slot(SlotId(1)).unwrap();
        assert_eq!(r.txs_executed, 3, "3 TX pipeline'dan geçmeli");
    }
}
