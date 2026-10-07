//! Transaction doğrulama — imza, nonce, bakiye, fee, gas limiti, vesting kilit

use nova_crypto::signatures::ecdsa::{EcdsaVerifier, public_key_to_address};
use nova_crypto::hash::blake3_hash;
use nova_primitives::crypto::{Hash, Signature, SignatureType};

use crate::errors::{RuntimeError, RuntimeResult};
use crate::state::cache::AccountStateCache;
use crate::token::vesting::VestingState;
use crate::tx::fee::effective_gas_price;
use crate::tx::types::{Transaction, TransactionKind, compute_tx_hash};

/// Maksimum TX boyutu: 256 KB
pub const MAX_TX_SIZE_BYTES: usize = 256 * 1024;

/// Maksimum TX yaşı: ~600 saniye @ 400ms/slot ≈ 1500 slot (Polkadot CheckMortality + Solana MAX_PROCESSING_AGE)
pub const MAX_TX_AGE_SLOTS: u64 = 1_500;

/// Maksimum ileri tarih toleransı: 10 slot (~4 saniye) clock drift toleransı
pub const MAX_TX_FUTURE_SLOTS: u64 = 10;

/// Compute unit limiti: Solana'nın 200k CU'su ile hizalanmış
pub const DEFAULT_COMPUTE_UNITS: u64 = 200_000;

/// Compute unit maliyetleri (Solana benchmarkları temel alındı)
pub mod compute_costs {
    pub const TRANSFER_CU: u64 = 2_100;        // basit transfer
    pub const STAKE_CU: u64 = 5_000;            // staking op
    pub const UNSTAKE_CU: u64 = 5_000;          // unstake
    pub const GOVERNANCE_VOTE_CU: u64 = 3_000;  // oy kullanma
    pub const GOVERNANCE_PROPOSE_CU: u64 = 10_000; // öneri oluşturma
    pub const REGISTER_VALIDATOR_CU: u64 = 15_000; // validator kayıt
    pub const WITHDRAW_UNBONDED_CU: u64 = 5_000;   // unbonded çekim
    pub const RUNTIME_UPGRADE_CU: u64 = 50_000;    // runtime upgrade — en pahalı
    pub const PER_BYTE_CU: u64 = 4;            // payload byte başına CU
}

/// TxValidator — işlem doğrulama motoru
pub struct TxValidator {
    current_base_fee: u128,
    current_slot: u64,
}

impl TxValidator {
    pub fn new(current_base_fee: u128) -> Self {
        Self { current_base_fee, current_slot: 0 }
    }

    /// Slot bilgisi ile doğrulayıcı oluştur (mortality kontrolü için)
    pub fn with_slot(current_base_fee: u128, current_slot: u64) -> Self {
        Self { current_base_fee, current_slot }
    }

    /// Tam TX doğrulama: imza + mortality + nonce + bakiye + fee + gas + compute budget
    ///
    /// BUG-4 düzeltmesi: İmza doğrulaması eklendi.
    /// Ethereum ecrecover yöntemiyle sender adresi doğrulanır:
    /// 1. TX hash hesapla (signature hariç tüm alanlar)
    /// 2. ECDSA recovery → public key
    /// 3. keccak256(pubkey[1..]) son 20 byte → address
    /// 4. tx.sender ile karşılaştır
    pub fn validate(&self, tx: &Transaction, state: &AccountStateCache) -> RuntimeResult<()> {
        self.check_size(tx)?;
        self.check_signature(tx)?;
        self.check_mortality(tx)?;
        self.check_compute_budget(tx)?;
        self.check_fee(tx)?;
        self.check_nonce(tx, state)?;
        self.check_balance(tx, state)?;
        Ok(())
    }

    /// Vesting kilit kontrolü dahil tam TX doğrulama.
    ///
    /// Genesis tokenomics adresleri için kilitli token transferini engeller.
    /// Vesting state None ise standart `validate()` davranışıyla aynıdır.
    pub fn validate_with_vesting(
        &self,
        tx: &Transaction,
        state: &AccountStateCache,
        vesting: Option<&VestingState>,
    ) -> RuntimeResult<()> {
        // Standart kontroller
        self.check_size(tx)?;
        self.check_signature(tx)?;
        self.check_mortality(tx)?;
        self.check_compute_budget(tx)?;
        self.check_fee(tx)?;
        self.check_nonce(tx, state)?;
        self.check_balance(tx, state)?;

        // Vesting kilit kontrolü
        if let Some(vs) = vesting {
            self.check_vesting(tx, state, vs)?;
        }

        Ok(())
    }

    /// Transaction mortality kontrolü (Polkadot CheckMortality + Solana MAX_PROCESSING_AGE)
    ///
    /// - TX'in `slot` alanı geçerli pencere içinde olmalı
    /// - Çok eski TX → replay saldırısı riski → TransactionExpired
    /// - Çok ileri tarihli TX → clock manipülasyon → TransactionTooFuture
    /// - current_slot=0 ise (genesis/test) kontrolü atla
    fn check_mortality(&self, tx: &Transaction) -> RuntimeResult<()> {
        // current_slot=0 → validator slot bilgisi yok (geriye dönük uyumluluk)
        if self.current_slot == 0 {
            return Ok(());
        }

        let tx_slot = tx.slot.0;
        let current = self.current_slot;

        // Çok eski TX: tx_slot + MAX_AGE < current → süresi dolmuş
        if tx_slot + MAX_TX_AGE_SLOTS < current {
            return Err(RuntimeError::TransactionExpired {
                tx_slot,
                current_slot: current,
                max_age: MAX_TX_AGE_SLOTS,
            });
        }

        // Çok ileri tarihli TX: tx_slot > current + MAX_FUTURE → gelecek TX
        if tx_slot > current + MAX_TX_FUTURE_SLOTS {
            return Err(RuntimeError::TransactionTooFuture {
                tx_slot,
                current_slot: current,
                max_future: MAX_TX_FUTURE_SLOTS,
            });
        }

        Ok(())
    }

    /// Compute budget kontrolü (Solana ComputeBudget)
    ///
    /// Her TX türü için sabit CU maliyeti + payload byte başına ek CU.
    /// gas_limit bu limiti aşamaz.
    fn check_compute_budget(&self, tx: &Transaction) -> RuntimeResult<()> {
        use compute_costs::*;

        let base_cu = match &tx.kind {
            TransactionKind::Transfer { .. } => TRANSFER_CU,
            TransactionKind::Stake { .. } => STAKE_CU,
            TransactionKind::Unstake { .. } => UNSTAKE_CU,
            TransactionKind::GovernanceVote { .. } => GOVERNANCE_VOTE_CU,
            TransactionKind::GovernancePropose { description, .. } => {
                GOVERNANCE_PROPOSE_CU + description.len() as u64 * PER_BYTE_CU
            }
            TransactionKind::RegisterValidator { .. } => REGISTER_VALIDATOR_CU,
            TransactionKind::WithdrawUnbonded { .. } => WITHDRAW_UNBONDED_CU,
            TransactionKind::RuntimeUpgrade { .. } => RUNTIME_UPGRADE_CU,
        };

        if base_cu > DEFAULT_COMPUTE_UNITS {
            return Err(RuntimeError::ComputeBudgetExceeded {
                used: base_cu,
                limit: DEFAULT_COMPUTE_UNITS,
            });
        }

        // gas_limit CU sınırını aşamaz
        if tx.gas_limit as u64 > DEFAULT_COMPUTE_UNITS {
            return Err(RuntimeError::ComputeBudgetExceeded {
                used: tx.gas_limit as u64,
                limit: DEFAULT_COMPUTE_UNITS,
            });
        }

        Ok(())
    }

    /// Vesting kilit kontrolü
    ///
    /// Sender'ın kilitli bakiyesini hesapla.
    /// Transfer + fee > (total_balance - locked) ise reddet.
    fn check_vesting(
        &self,
        tx: &Transaction,
        state: &AccountStateCache,
        vs: &VestingState,
    ) -> RuntimeResult<()> {
        let locked = vs.locked(&tx.sender);

        // Kilitsiz adresler için hızlı çıkış
        if locked == 0 {
            return Ok(());
        }

        let total_balance = state.balance(&tx.sender);
        let max_fee = tx.max_fee_per_gas * tx.gas_limit as u128;

        let transfer_amount = match &tx.kind {
            TransactionKind::Transfer { amount, .. } => *amount,
            TransactionKind::Stake { amount, .. } => *amount,
            TransactionKind::GovernancePropose { deposit, .. } => *deposit,
            _ => 0,
        };

        let available = total_balance.saturating_sub(locked);
        let requested = transfer_amount + max_fee;

        if requested > available {
            return Err(RuntimeError::VestingLockedTransfer {
                locked,
                available,
                requested,
            });
        }

        Ok(())
    }

    /// İmza doğrulaması — Ethereum ecrecover yöntemi.
    ///
    /// TX hash (signature hariç) üzerinden sender'ın imzayı attığını kanıtla.
    /// Boş imza → test/genesis TX'ler için atla (production'da strict).
    fn check_signature(&self, tx: &Transaction) -> RuntimeResult<()> {
        // Boş imza: sadece genesis veya test ortamında kabul
        if tx.signature.is_empty() {
            return Err(RuntimeError::InvalidSignature(
                "imza boş — TX imzalanmalı".into()
            ));
        }

        // Signature formatı: r(32) || s(32) || v(1) = 65 byte ECDSA
        if tx.signature.len() != 65 {
            return Err(RuntimeError::InvalidSignature(format!(
                "imza boyutu yanlış: beklenen 65, gelen {}",
                tx.signature.len()
            )));
        }

        // TX hash: signature dahil değil (compute_tx_hash zaten signature'ı dışlar)
        let tx_hash_bytes = compute_tx_hash(tx);
        let tx_hash = Hash::from_bytes(tx_hash_bytes);

        // ECDSA signature wrap et
        let sig = Signature::new_ecdsa(tx.signature.clone());

        // Recover public key from hash + signature
        let recovered_pubkey = EcdsaVerifier::recover_public_key(&tx_hash, &sig)
            .map_err(|e| RuntimeError::InvalidSignature(format!(
                "public key recover başarısız: {e}"
            )))?;

        // Public key'den adres türet (Ethereum keccak256 yöntemi)
        let derived_address = public_key_to_address(&recovered_pubkey);

        // Sender adresiyle karşılaştır
        if derived_address != tx.sender {
            return Err(RuntimeError::InvalidSignature(format!(
                "imza sender ile uyuşmuyor: beklenen {:?}, recover edilen {:?}",
                tx.sender, derived_address
            )));
        }

        Ok(())
    }

    /// TX boyut kontrolü
    fn check_size(&self, tx: &Transaction) -> RuntimeResult<()> {
        // Basit heuristic: kind + base alanlar
        let estimated_size = 200 + match &tx.kind {
            TransactionKind::GovernancePropose { description, .. } => description.len(),
            _ => 0,
        };
        if estimated_size > MAX_TX_SIZE_BYTES {
            return Err(RuntimeError::TransactionTooLarge {
                size: estimated_size,
                max: MAX_TX_SIZE_BYTES,
            });
        }
        Ok(())
    }

    /// Fee kontrolü: max_fee >= current_base_fee
    fn check_fee(&self, tx: &Transaction) -> RuntimeResult<()> {
        match effective_gas_price(
            tx.max_fee_per_gas,
            tx.max_priority_fee_per_gas,
            self.current_base_fee,
        ) {
            None => Err(RuntimeError::BaseFeeTooLow {
                min_fee: self.current_base_fee,
                provided: tx.max_fee_per_gas,
            }),
            Some(_) => Ok(()),
        }
    }

    /// Nonce kontrolü
    fn check_nonce(&self, tx: &Transaction, state: &AccountStateCache) -> RuntimeResult<()> {
        let expected = state.nonce(&tx.sender);
        if tx.nonce != expected {
            return Err(RuntimeError::InvalidNonce { expected, got: tx.nonce });
        }
        Ok(())
    }

    /// Bakiye kontrolü: gönderenin max olası fee + transfer miktarı için yeterli bakiyesi var mı?
    fn check_balance(&self, tx: &Transaction, state: &AccountStateCache) -> RuntimeResult<()> {
        let balance = state.balance(&tx.sender);
        let max_fee = tx.max_fee_per_gas * tx.gas_limit as u128;

        let transfer_amount = match &tx.kind {
            TransactionKind::Transfer { amount, .. } => *amount,
            TransactionKind::Stake { amount, .. } => *amount,
            TransactionKind::GovernancePropose { deposit, .. } => *deposit,
            _ => 0,
        };

        let required = max_fee + transfer_amount;
        if balance < required {
            return Err(RuntimeError::InsufficientBalance {
                required,
                available: balance,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::types::{Transaction, TransactionKind, compute_tx_hash};
    use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
    use nova_primitives::{Address, SlotId, ValidatorId};
    use nova_primitives::crypto::Hash;

    /// Test için ECDSA keypair + signed transaction oluştur.
    /// Sender adresi keypair'den türetilir → imza doğrulaması geçer.
    fn make_signed_transfer(
        keypair: &EcdsaKeypair,
        to: Address,
        amount: u128,
        nonce: u64,
    ) -> Transaction {
        let pk = keypair.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = Transaction {
            sender,
            nonce,
            kind: TransactionKind::Transfer { to, amount },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 21_000,
            signature: Vec::new(), // önce boş
            slot: SlotId(0),
        };

        // TX hash'i imzala (signature boşken hash al)
        let hash_bytes = compute_tx_hash(&tx);
        let hash = Hash::from_bytes(hash_bytes);
        let sig = keypair.sign_hash(&hash).unwrap();
        tx.signature = sig.as_bytes().to_vec();
        tx
    }

    fn make_signed_stake(
        keypair: &EcdsaKeypair,
        validator_id: ValidatorId,
        amount: u128,
        nonce: u64,
    ) -> Transaction {
        let pk = keypair.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = Transaction {
            sender,
            nonce,
            kind: TransactionKind::Stake { validator_id, amount },
            max_fee_per_gas: 1_000_000_000,
            max_priority_fee_per_gas: 100_000_000,
            gas_limit: 50_000,
            signature: Vec::new(),
            slot: SlotId(0),
        };

        let hash_bytes = compute_tx_hash(&tx);
        let hash = Hash::from_bytes(hash_bytes);
        let sig = keypair.sign_hash(&hash).unwrap();
        tx.signature = sig.as_bytes().to_vec();
        tx
    }

    fn funded_state(sender: Address, balance: u128, nonce: u64) -> AccountStateCache {
        let state = AccountStateCache::new();
        state.set_balance(sender, balance);
        state.set_nonce(sender, nonce);
        state
    }

    fn dummy_addr() -> Address {
        Address::from_bytes([0xABu8; 20])
    }

    // ─── İMZA DOĞRULAMA TESTLERİ ──────────────────────────────────────────────

    #[test]
    fn test_valid_transfer_with_real_signature() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        validator.validate(&tx, &state).unwrap();
    }

    #[test]
    fn test_empty_signature_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.signature.clear(); // imzayı sil
        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::InvalidSignature(_))));
    }

    #[test]
    fn test_wrong_signature_rejected() {
        // Farklı bir keypair ile imzalı tx, başka sender'a aitmiş gibi sunulursa reddedilmeli
        let kp_real = EcdsaKeypair::generate().unwrap();
        let kp_attacker = EcdsaKeypair::generate().unwrap();

        let pk_real = kp_real.public_key().unwrap();
        let real_sender = public_key_to_address(&pk_real);

        // Saldırgan kendi keypair'ıyla imzalıyor
        let attacker_tx = make_signed_transfer(&kp_attacker, dummy_addr(), 100, 0);
        // Ama sender'ı gerçek kurbanın adresi olarak değiştiriyor
        let mut tx = attacker_tx;
        tx.sender = real_sender;

        let state = funded_state(real_sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::InvalidSignature(_))),
            "yanlış imza reddedilmeli");
    }

    #[test]
    fn test_tampered_tx_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        // TX imzalandıktan sonra amount'u değiştir
        if let TransactionKind::Transfer { ref mut amount, .. } = tx.kind {
            *amount = 999_999_999_999; // imzalanmış miktardan farklı
        }

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::InvalidSignature(_))),
            "değiştirilmiş tx reddedilmeli");
    }

    // ─── NONCE TESTLERİ ────────────────────────────────────────────────────────

    #[test]
    fn test_invalid_nonce() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 5); // nonce=5 ama 0 bekleniyor
        let state = funded_state(sender, 10_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::InvalidNonce { expected: 0, got: 5 })));
    }

    // ─── BAKİYE TESTLERİ ──────────────────────────────────────────────────────

    #[test]
    fn test_insufficient_balance() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, 1, 0); // Neredeyse hiç bakiye yok
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::InsufficientBalance { .. })));
    }

    // ─── FEE TESTLERİ ─────────────────────────────────────────────────────────

    #[test]
    fn test_base_fee_too_low() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        // max_fee çok düşük tx oluştur
        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        // İmzayı bozmadan önce: düşük fee ile yeniden imzala
        tx.max_fee_per_gas = 100_000;
        let hash_bytes = compute_tx_hash(&tx);
        let hash = Hash::from_bytes(hash_bytes);
        let sig = kp.sign_hash(&hash).unwrap();
        tx.signature = sig.as_bytes().to_vec();

        let state = funded_state(sender, 10_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000); // base fee = 1 Gwei
        let err = validator.validate(&tx, &state);
        assert!(matches!(err, Err(RuntimeError::BaseFeeTooLow { .. })));
    }

    // ─── STAKE TESTLERİ ───────────────────────────────────────────────────────

    #[test]
    fn test_valid_stake_tx() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_stake(&kp, ValidatorId(1), 1_000_000, 0);
        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        validator.validate(&tx, &state).unwrap();
    }

    // ─── VESTİNG KİLİT TESTLERİ ──────────────────────────────────────────────

    use crate::token::vesting::{VestingSchedule, VestingState};

    fn make_vesting_state_with_lock(addr: Address, locked_amount: u128) -> VestingState {
        let mut vs = VestingState::new();
        // Linear vesting: başlangıçta tamamen kilitli
        vs.register(addr, VestingSchedule::linear(locked_amount, 48));
        vs
    }

    #[test]
    fn test_no_vesting_state_passes_through() {
        // vesting=None → standart validate ile aynı davranış
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        validator.validate_with_vesting(&tx, &state, None).unwrap();
    }

    #[test]
    fn test_unlocked_address_not_affected_by_vesting() {
        // Vesting state var ama sender için kilit yok
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, 100_000_000_000_000, 0);

        // Farklı bir adrese kilit koy, sender'a değil
        let vs = make_vesting_state_with_lock(dummy_addr(), 1_000_000);

        let validator = TxValidator::new(1_000_000_000);
        validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap();
    }

    #[test]
    fn test_vesting_blocks_locked_transfer() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        // Sender'ın 1M token'ı var ama hepsi kilitli (epoch=0, linear 48 ay)
        let balance = 1_000_000_000_000_000_000u128; // 1 NOVA
        let locked = balance; // tamamı kilitli (epoch 0'da vested = 0)
        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, balance, 0);
        let vs = make_vesting_state_with_lock(sender, locked);

        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap_err();
        assert!(
            matches!(err, RuntimeError::VestingLockedTransfer { .. }),
            "kilitli transfer reddedilmeli, gelen hata: {err:?}"
        );
    }

    #[test]
    fn test_vesting_allows_transfer_from_unlocked_portion() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        // 1000 token, 500 kilitli, 500 serbest
        let total = 1_000_000_000_000u128;
        let locked = 500_000_000_000u128;
        let available = total - locked;

        // Fee: 1_000_000_000 * 21_000 = 21_000_000_000_000
        // Bu available'dan büyük olabilir, küçük transfer deneyelim
        let fee_cost = 1_000_000_000u128 * 21_000u128; // 21 * 10^12

        // Balance fee_cost + available'dan fazla olsun
        let balance = locked + fee_cost + 100;
        let transfer_amount = 50u128;

        // available = balance - locked = fee_cost + 100
        // requested = transfer_amount + fee_cost = fee_cost + 50 ≤ available
        let state = funded_state(sender, balance, 0);
        let mut vs = VestingState::new();
        // Locked = locked (epoch 0'da tüm linear kilitli)
        vs.register(sender, VestingSchedule::linear(locked, 48));

        let tx = make_signed_transfer(&kp, dummy_addr(), transfer_amount, 0);
        let validator = TxValidator::new(1_000_000_000);
        // available = fee_cost + 100, requested = transfer_amount + fee_cost = fee_cost + 50
        // fee_cost + 50 ≤ fee_cost + 100 → izin verilmeli
        validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap();
    }

    #[test]
    fn test_vesting_blocks_stake_with_locked_tokens() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let balance = 2_000_000_000_000_000_000u128; // 2 NOVA
        let locked = balance; // tamamı kilitli

        let tx = make_signed_stake(&kp, ValidatorId(1), 1_000_000, 0);
        let state = funded_state(sender, balance, 0);
        let vs = make_vesting_state_with_lock(sender, locked);

        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap_err();
        assert!(
            matches!(err, RuntimeError::VestingLockedTransfer { .. }),
            "kilitli stake reddedilmeli"
        );
    }

    #[test]
    fn test_vesting_fully_vested_allows_transfer() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let total = 1_000_000_000_000u128;
        let balance = total + 1_000_000_000_000_000u128; // fazladan bakiye (fee için)

        // 48 ay geçtikten sonra tümü açık
        let mut vs = VestingState::new();
        vs.register(sender, VestingSchedule::linear(total, 48));
        vs.advance_epoch(48 * 30); // 48 ay ilerlet

        // locked = 0 artık
        assert_eq!(vs.locked(&sender), 0, "48 ay sonra kilit 0 olmalı");

        let tx = make_signed_transfer(&kp, dummy_addr(), total / 2, 0);
        let state = funded_state(sender, balance, 0);
        let validator = TxValidator::new(1_000_000_000);
        validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap();
    }

    // ─── MORTALITY (SLOT PENCERE) TESTLERİ ───────────────────────────────────

    #[test]
    fn test_mortality_valid_slot_passes() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let current_slot = 1000u64;
        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.slot = SlotId(current_slot); // tam geçerli slot
        // imzayı yenile (slot değişti)
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::with_slot(1_000_000_000, current_slot);
        validator.validate(&tx, &state).unwrap();
    }

    #[test]
    fn test_mortality_expired_tx_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let current_slot = 5000u64;
        // tx_slot çok eski: current - MAX_AGE - 1
        let old_slot = current_slot - MAX_TX_AGE_SLOTS - 1;
        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.slot = SlotId(old_slot);
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::with_slot(1_000_000_000, current_slot);
        let err = validator.validate(&tx, &state);
        assert!(
            matches!(err, Err(RuntimeError::TransactionExpired { .. })),
            "süresi dolmuş TX reddedilmeli: {err:?}"
        );
    }

    #[test]
    fn test_mortality_future_tx_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let current_slot = 1000u64;
        let future_slot = current_slot + MAX_TX_FUTURE_SLOTS + 1;
        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.slot = SlotId(future_slot);
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::with_slot(1_000_000_000, current_slot);
        let err = validator.validate(&tx, &state);
        assert!(
            matches!(err, Err(RuntimeError::TransactionTooFuture { .. })),
            "gelecek TX reddedilmeli: {err:?}"
        );
    }

    #[test]
    fn test_mortality_slot_zero_skips_check() {
        // current_slot=0 → mortality kontrolü atlanır (genesis/test uyumluluğu)
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.slot = SlotId(0);
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000); // current_slot=0
        validator.validate(&tx, &state).unwrap();
    }

    // ─── COMPUTE BUDGET TESTLERİ ─────────────────────────────────────────────

    #[test]
    fn test_compute_budget_transfer_within_limit() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        // Transfer CU = 2100 < 200_000 → geçmeli
        validator.validate(&tx, &state).unwrap();
    }

    #[test]
    fn test_compute_budget_gas_limit_over_cu_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        let mut tx = make_signed_transfer(&kp, dummy_addr(), 100, 0);
        tx.gas_limit = DEFAULT_COMPUTE_UNITS + 1; // CU limitini aşıyor
        let hash = Hash::from_bytes(compute_tx_hash(&tx));
        tx.signature = kp.sign_hash(&hash).unwrap().as_bytes().to_vec();

        let state = funded_state(sender, 100_000_000_000_000, 0);
        let validator = TxValidator::new(1_000_000_000);
        let err = validator.validate(&tx, &state);
        assert!(
            matches!(err, Err(RuntimeError::ComputeBudgetExceeded { .. })),
            "gas_limit > CU_limit reddedilmeli: {err:?}"
        );
    }

    #[test]
    fn test_vesting_locked_error_contains_correct_values() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let sender = public_key_to_address(&pk);

        // Fee: 1_000_000_000 * 21_000 = 21_000_000_000_000
        // balance > fee (check_balance geçer), ama tümü kilitli → vesting check tetiklenir
        let fee_cost = 1_000_000_000u128 * 21_000u128; // 21 * 10^12
        let balance = fee_cost * 10; // fee_cost'tan büyük, check_balance geçer
        let locked = balance; // tamamı kilitli → available = 0

        let transfer = 100u128;
        let tx = make_signed_transfer(&kp, dummy_addr(), transfer, 0);
        let state = funded_state(sender, balance, 0);
        let vs = make_vesting_state_with_lock(sender, locked);

        let validator = TxValidator::new(1_000_000_000);
        match validator.validate_with_vesting(&tx, &state, Some(&vs)).unwrap_err() {
            RuntimeError::VestingLockedTransfer { locked: l, available: a, requested: r } => {
                assert_eq!(l, locked, "locked doğru olmalı");
                assert_eq!(a, 0, "available=balance-locked=0 olmalı");
                assert!(r > 0, "requested > 0 olmalı");
            }
            err => panic!("yanlış hata türü: {err:?}"),
        }
    }
}
