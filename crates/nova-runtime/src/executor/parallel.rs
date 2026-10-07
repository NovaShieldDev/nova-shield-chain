//! Paralel TX yürütme motoru (Sealevel ilkesi, Solana'dan ilham)
//!
//! Bağımsız TX'ler Rayon thread pool ile paralel çalıştırılır.
//! Çakışan TX'ler sıralı işlenir.
//!
//! Güvenlik katmanları:
//! 1. `build_parallel_groups` — statik analiz: çakışan TX'leri ayrı gruplara ayırır
//! 2. `AccountLocks` — runtime kilit: her TX execution öncesi write/read kilitleri alır
//!    (DependencyGraph'ın kaçırdığı edge-case'lere karşı güvenlik ağı)

use crate::executor::scheduler::{build_parallel_groups, derive_account_keys};
use crate::state::cache::AccountStateCache;
use crate::state::locks::AccountLocks;
use crate::tx::types::{Transaction, TransactionKind};
use crate::tx::validation::TxValidator;
use crate::errors::{RuntimeError, RuntimeResult};
use crate::virtual_account::{STAKING_MODULE_ADDR, GOVERNANCE_MODULE_ADDR};
use nova_primitives::Address;
use rayon::prelude::*;
use std::sync::Arc;

/// TX yürütme sonucu
#[derive(Debug, Clone)]
pub struct TxResult {
    pub tx_hash: [u8; 32],
    pub success: bool,
    pub error: Option<String>,
    pub gas_used: u64,
    pub fee_paid: u128,
}

/// Paralel yürütme motoru
pub struct ParallelExecutor {
    state: Arc<AccountStateCache>,
    base_fee: u128,
    /// Runtime hesap kilitleri — statik scheduler'ın kaçırdığı çakışmalara karşı güvenlik ağı
    account_locks: Arc<AccountLocks>,
}

impl ParallelExecutor {
    pub fn new(state: Arc<AccountStateCache>, base_fee: u128) -> Self {
        Self { state, base_fee, account_locks: Arc::new(AccountLocks::new()) }
    }

    /// AccountLocks ile executor oluştur (paylaşımlı kilit yönetimi için)
    pub fn new_with_locks(
        state: Arc<AccountStateCache>,
        base_fee: u128,
        account_locks: Arc<AccountLocks>,
    ) -> Self {
        Self { state, base_fee, account_locks }
    }

    /// Aktif hesap kilidi sayısı (monitoring için)
    pub fn active_lock_count(&self) -> usize {
        self.account_locks.active_lock_count()
    }

    /// Bir TX bloğunu paralel gruplarda yürüt
    pub fn execute_block(&self, txs: &[Transaction]) -> Vec<TxResult> {
        let groups = build_parallel_groups(txs);
        let mut results = vec![
            TxResult {
                tx_hash: [0u8; 32],
                success: false,
                error: None,
                gas_used: 0,
                fee_paid: 0,
            };
            txs.len()
        ];

        for group in groups {
            // Gruptaki TX'leri paralel yürüt
            let group_results: Vec<(usize, TxResult)> = group
                .par_iter()
                .map(|&idx| {
                    let tx = &txs[idx];
                    let result = self.execute_single(tx);
                    (idx, result)
                })
                .collect();

            for (idx, result) in group_results {
                results[idx] = result;
            }
        }

        results
    }

    /// Tek TX yürüt
    fn execute_single(&self, tx: &Transaction) -> TxResult {
        use crate::tx::types::compute_tx_hash;
        let tx_hash = compute_tx_hash(tx);

        let validator = TxValidator::new(self.base_fee);
        if let Err(e) = validator.validate(tx, &self.state) {
            return TxResult {
                tx_hash,
                success: false,
                error: Some(e.to_string()),
                gas_used: 0,
                fee_paid: 0,
            };
        }

        // ── AccountLocks: runtime write/read kilitleri al ──────────────────
        // Statik DependencyGraph scheduler'ın üstünde ikinci güvenlik katmanı.
        // Kilitler TX yürütmesi bitince `_lock_guard` drop'ta otomatik serbest kalır.
        let account_keys = derive_account_keys(tx);
        let writes: Vec<Address> = account_keys.writes().iter().copied().collect();
        let reads: Vec<Address> = account_keys.reads().iter().copied().collect();

        let _lock_guard = match self.account_locks.try_lock(&writes, &reads) {
            Ok(guard) => guard,
            Err(e) => {
                return TxResult {
                    tx_hash,
                    success: false,
                    error: Some(format!("hesap kilidi alınamadı: {e}")),
                    gas_used: 0,
                    fee_paid: 0,
                };
            }
        };
        // _lock_guard bu scope'ta yaşar → TX bitene kadar kilitler aktif

        // Nonce artır
        self.state.increment_nonce(&tx.sender);

        // İşleme göre state değiştir
        let staking_addr = Address::from_bytes(STAKING_MODULE_ADDR);
        let gov_addr = Address::from_bytes(GOVERNANCE_MODULE_ADDR);

        let (gas_used, err) = match &tx.kind {
            TransactionKind::Transfer { to, amount } => {
                if self.state.transfer(&tx.sender, to, *amount) {
                    (crate::tx::fee::TRANSFER_GAS_COST, None)
                } else {
                    (crate::tx::fee::TRANSFER_GAS_COST, Some("transfer failed: insufficient balance".to_string()))
                }
            }
            TransactionKind::Stake { amount, .. } => {
                // Cosmos bonding: sender bakiyesi → staking modülü
                if self.state.transfer(&tx.sender, &staking_addr, *amount) {
                    (crate::tx::fee::STAKING_GAS_COST, None)
                } else {
                    (crate::tx::fee::STAKING_GAS_COST, Some("stake failed: insufficient balance".to_string()))
                }
            }
            TransactionKind::Unstake { amount, .. } => {
                // Unbonding başlat: staking modülü → sender (basitleştirilmiş — tam impl unbonding period gerekir)
                if self.state.transfer(&staking_addr, &tx.sender, *amount) {
                    (crate::tx::fee::STAKING_GAS_COST, None)
                } else {
                    (crate::tx::fee::STAKING_GAS_COST, Some("unstake failed: insufficient staked balance".to_string()))
                }
            }
            TransactionKind::GovernanceVote { .. } => {
                // Oy kayıt edilir — token hareketi yok (governance modülü state'i ayrı)
                (crate::tx::fee::GOVERNANCE_GAS_COST, None)
            }
            TransactionKind::GovernancePropose { deposit, .. } => {
                // Öneri depozitosu governance modülüne transfer edilir (Cosmos gov deposit)
                if self.state.transfer(&tx.sender, &gov_addr, *deposit) {
                    (crate::tx::fee::GOVERNANCE_GAS_COST, None)
                } else {
                    (crate::tx::fee::GOVERNANCE_GAS_COST, Some("propose failed: insufficient deposit balance".to_string()))
                }
            }
            TransactionKind::RegisterValidator { min_self_delegation, .. } => {
                // Validator self-delegation: sender → staking modülü
                if self.state.transfer(&tx.sender, &staking_addr, *min_self_delegation) {
                    (crate::tx::fee::STAKING_GAS_COST, None)
                } else {
                    (crate::tx::fee::STAKING_GAS_COST, Some("register validator failed: insufficient self-delegation balance".to_string()))
                }
            }
            TransactionKind::WithdrawUnbonded { .. } => {
                // Unbonding tamamlandıktan sonra çekim — staking modülü tarafından yönetilir.
                // Executor seviyesinde sadece TX kaydedilir; gerçek ödeme staking::reward modülünde.
                (crate::tx::fee::STAKING_GAS_COST, None)
            }
            TransactionKind::RuntimeUpgrade { .. } => {
                // Governance onaylı runtime upgrade — token hareketi yok
                (crate::tx::fee::GOVERNANCE_GAS_COST, None)
            }
        };

        // Fee hesapla ve öde
        let fee = self.base_fee * gas_used as u128;
        let fee_collector = Address::from_bytes([0xFEu8; 20]); // Fee collector
        let fee_deducted = self.state.transfer(
            &tx.sender,
            &fee_collector,
            fee,
        );

        let success = err.is_none();
        TxResult {
            tx_hash,
            success,
            error: err,
            gas_used,
            fee_paid: if fee_deducted { fee } else { 0 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::nsc::NSC_DECIMALS;

    /// Deterministik test adresi (alıcı için, imza doğrulaması yapılmaz)
    fn dest_addr(n: u8) -> Address {
        Address::from_bytes([0xDDu8, n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, n])
    }

    /// Signed TX + sender adresi üret (seed_byte: 1..255)
    fn signed_transfer(seed: u8, to: Address, amount: u128, nonce: u64) -> (Transaction, Address) {
        Transaction::signed_for_test(seed, to, amount, nonce)
    }

    #[test]
    fn test_execute_transfer() {
        let (tx, sender) = signed_transfer(1, dest_addr(2), 10 * NSC_DECIMALS, 0);

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 100 * NSC_DECIMALS);
        let executor = ParallelExecutor::new(Arc::clone(&state), 0);

        let results = executor.execute_block(&[tx]);
        assert_eq!(results.len(), 1);
        assert!(results[0].success, "transfer başarılı olmalı: {:?}", results[0].error);
        assert_eq!(state.balance(&dest_addr(2)), 10 * NSC_DECIMALS);
    }

    #[test]
    fn test_execute_invalid_nonce() {
        let (mut tx, sender) = signed_transfer(2, dest_addr(3), 10 * NSC_DECIMALS, 0);

        // Nonce = 0 ile imzalı TX ama state nonce = 5
        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 100 * NSC_DECIMALS);
        state.set_nonce(sender, 5); // beklenen nonce = 5

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx]);
        // Nonce uyumsuz → başarısız (InvalidNonce veya InvalidSignature çünkü nonce farklı → hash farklı)
        assert!(!results[0].success, "yanlış nonce reddedilmeli");
    }

    #[test]
    fn test_execute_parallel_independent_txs() {
        let (tx1, s1) = signed_transfer(10, dest_addr(100), 10 * NSC_DECIMALS, 0);
        let (tx2, s2) = signed_transfer(11, dest_addr(101), 10 * NSC_DECIMALS, 0);
        let (tx3, s3) = signed_transfer(12, dest_addr(102), 10 * NSC_DECIMALS, 0);

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(s1, 100 * NSC_DECIMALS);
        state.set_balance(s2, 100 * NSC_DECIMALS);
        state.set_balance(s3, 100 * NSC_DECIMALS);

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx1, tx2, tx3]);

        assert!(results.iter().all(|r| r.success),
            "tüm paralel TX'ler başarılı olmalı: {:?}", results.iter().filter(|r| !r.success).collect::<Vec<_>>());
        assert_eq!(state.balance(&dest_addr(100)), 10 * NSC_DECIMALS);
        assert_eq!(state.balance(&dest_addr(101)), 10 * NSC_DECIMALS);
        assert_eq!(state.balance(&dest_addr(102)), 10 * NSC_DECIMALS);
    }

    #[test]
    fn test_execute_nonce_increments() {
        let (tx, sender) = signed_transfer(20, dest_addr(21), 1, 0);

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 100 * NSC_DECIMALS);

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        executor.execute_block(&[tx]);
        assert_eq!(state.nonce(&sender), 1, "nonce 1'e yükselmeli");
    }

    #[test]
    fn test_stake_moves_tokens_to_staking_module() {
        use crate::virtual_account::STAKING_MODULE_ADDR;
        use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
        use nova_primitives::crypto::Hash;
        use crate::tx::types::compute_tx_hash;
        use nova_primitives::ValidatorId;

        let mut seed = [0u8; 32];
        seed[0] = 50;
        let kp = EcdsaKeypair::from_secret_bytes(&seed).unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);
        let validator_id = ValidatorId(1);
        let stake_amount = 1_000 * NSC_DECIMALS;

        let mut tx = Transaction {
            sender,
            nonce: 0,
            kind: TransactionKind::Stake { validator_id, amount: stake_amount },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 50_000,
            signature: Vec::new(),
            slot: nova_primitives::SlotId(0),
        };
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 10_000 * NSC_DECIMALS);

        let staking_addr = Address::from_bytes(STAKING_MODULE_ADDR);
        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx]);

        assert!(results[0].success, "stake başarılı olmalı: {:?}", results[0].error);
        assert_eq!(state.balance(&staking_addr), stake_amount, "stake tutarı staking modülüne gitmiş olmalı");
        assert!(state.balance(&sender) < 10_000 * NSC_DECIMALS, "gönderenin bakiyesi azalmış olmalı");
    }

    #[test]
    fn test_governance_propose_transfers_deposit() {
        use crate::virtual_account::GOVERNANCE_MODULE_ADDR;
        use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
        use nova_primitives::crypto::Hash;
        use crate::tx::types::{compute_tx_hash, ProposalPayload};

        let mut seed = [0u8; 32];
        seed[0] = 51;
        let kp = EcdsaKeypair::from_secret_bytes(&seed).unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);
        let deposit = 500 * NSC_DECIMALS;

        let mut tx = Transaction {
            sender,
            nonce: 0,
            kind: TransactionKind::GovernancePropose {
                title: "Test".to_string(),
                description: "Test proposal".to_string(),
                payload: ProposalPayload::Text { content: "test".to_string() },
                deposit,
            },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 100_000,
            signature: Vec::new(),
            slot: nova_primitives::SlotId(0),
        };
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 10_000 * NSC_DECIMALS);

        let gov_addr = Address::from_bytes(GOVERNANCE_MODULE_ADDR);
        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx]);

        assert!(results[0].success, "propose başarılı olmalı: {:?}", results[0].error);
        assert_eq!(state.balance(&gov_addr), deposit, "depozito governance modülüne gitmiş olmalı");
    }

    #[test]
    fn test_stake_fails_insufficient_balance() {
        use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
        use nova_primitives::crypto::Hash;
        use crate::tx::types::compute_tx_hash;
        use nova_primitives::ValidatorId;

        let mut seed = [0u8; 32];
        seed[0] = 52;
        let kp = EcdsaKeypair::from_secret_bytes(&seed).unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = Transaction {
            sender,
            nonce: 0,
            kind: TransactionKind::Stake { validator_id: ValidatorId(1), amount: 1_000 * NSC_DECIMALS },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 50_000,
            signature: Vec::new(),
            slot: nova_primitives::SlotId(0),
        };
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(sender, 1); // yetersiz bakiye

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx]);
        // Validation aşamasında InsufficientBalance hatası verir
        assert!(!results[0].success, "yetersiz bakiye ile stake reddedilmeli");
    }

    #[test]
    fn test_forged_sender_rejected() {
        // Saldırgan kendi keypair ile imzalar ama sender'ı başkasına atar
        let (mut tx, _real_sender) = signed_transfer(30, dest_addr(31), 1, 0);
        let victim = Address::from_bytes([0xABu8; 20]);
        tx.sender = victim; // imzayı bozmadan sender değiştir

        let state = Arc::new(AccountStateCache::new());
        state.set_balance(victim, 100 * NSC_DECIMALS);

        let executor = ParallelExecutor::new(Arc::clone(&state), 0);
        let results = executor.execute_block(&[tx]);
        assert!(!results[0].success, "sahte sender reddedilmeli");
    }
}
