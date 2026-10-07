//! Guardian staking mekanizması — Hold/Freeze, ExistentialDeposit, Hooks, SendRestriction
//!
//! ## AŞAMA 8 Görevleri
//!
//! ### Görev 58: Hold/Freeze Mekanizması (Polkadot pattern)
//!
//! Polkadot `frame/balances/src/lib.rs`:
//! - `hold`: Slash edilebilir ama transfer edilemez (staking lock)
//! - `freeze`: Transfer edilemez ama slash da edilemez (governance lock)
//! - `HoldReason`: Slashing, Bonding, Fee
//!
//! Nova Guardian'da:
//! - `GuardianHold`: Guardian stake'i — slash edilebilir, transfer edilemez
//! - `GuardianFreeze`: Aktif iş var — serbest bırakılamaz
//!
//! ### Görev 59: ExistentialDeposit
//!
//! Polkadot: Hesap varoluşu için minimum bakiye.
//! Guardian'da: Minimum stake olmazsa `Inactive` → hesap silinebilir.
//!
//! ### Görev 60: StakingHooks (Cosmos SDK MultiStakingHooks pattern)
//!
//! Cosmos SDK `x/staking/types/hooks.go`:
//! ```text
//! AfterValidatorCreated, BeforeValidatorModified, AfterValidatorBonded,
//! AfterValidatorBeginUnbonding, BeforeValidatorSlashed
//! ```
//!
//! Nova: Guardian olay kancaları zinciri.
//!
//! ### Görev 61: SendRestriction
//!
//! Polkadot `frame/balances/src/lib.rs`:
//! `RestrictedTransfer`: Transfer öncesi kontrol zinciri.

use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ─── SABITLER ─────────────────────────────────────────────────────────────────

/// AŞAMA 8 Görev 59: ExistentialDeposit — minimum guardian stake.
///
/// Polkadot: 1 DOT = 10^10 Plancks.
/// Nova: 100 NSC — bunun altındaki guardian hesapları "existential" risk altında.
pub const GUARDIAN_EXISTENTIAL_DEPOSIT: u128 = 100 * 1_000_000_000_000_000_000; // 100 NSC

// ─── HOLD MEKANİZMASI ─────────────────────────────────────────────────────────

/// Hold nedeni — neden bu stake kilitli?
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HoldReason {
    /// Aktif staking lock — slash edilebilir.
    GuardianStaking,
    /// Aktif sözleşme teminatı — sözleşme bitmeden açılamaz.
    ActiveContract { contract_id: u64 },
    /// Dispute (ihtilaf) süreci — karar bekleniyor.
    DisputeEscrow { dispute_id: u64 },
}

/// Freeze nedeni — neden transfer engellenmiş?
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FreezeReason {
    /// Slashing süreci başlatıldı — sonuç bekleniyor.
    PendingSlash,
    /// KYC yenileme bekleniyor.
    KycRenewal,
    /// Yönetim oylaması kilitli.
    GovernanceVote,
}

/// Bir guardian için hold kaydı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldEntry {
    pub reason: HoldReason,
    pub amount: u128,
    pub locked_at_slot: u64,
    /// Otomatik serbest bırakma slot'u (None = manuel serbest bırakma)
    pub unlock_at_slot: Option<u64>,
}

/// Bir guardian için freeze kaydı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeEntry {
    pub reason: FreezeReason,
    pub frozen_at_slot: u64,
}

/// AŞAMA 8 Görev 58: Guardian stake hold/freeze yöneticisi.
#[derive(Debug, Default)]
pub struct GuardianStakeManager {
    /// Guardian → hold kayıtları
    holds: HashMap<Address, Vec<HoldEntry>>,
    /// Guardian → freeze kayıtları
    freezes: HashMap<Address, Vec<FreezeEntry>>,
    /// Guardian → toplam stake
    total_stake: HashMap<Address, u128>,
}

impl GuardianStakeManager {
    pub fn new() -> Self { Self::default() }

    /// Stake kaydet.
    pub fn bond(&mut self, guardian: Address, amount: u128, slot: SlotId) {
        *self.total_stake.entry(guardian).or_insert(0) += amount;
        self.holds.entry(guardian).or_default().push(HoldEntry {
            reason: HoldReason::GuardianStaking,
            amount,
            locked_at_slot: slot.0,
            unlock_at_slot: None,
        });
    }

    /// Sözleşme teminatı kilitle.
    ///
    /// Hold'lar overlay çalışır (Polkadot pattern): GuardianStaking hold'u
    /// zaten tüm stake'i kaplıyor, sözleşme lock'u bunun üstüne ek kısıt.
    /// Kontrol: toplam stake >= mevcut sözleşme lock'ları + yeni miktar
    pub fn lock_for_contract(
        &mut self,
        guardian: Address,
        contract_id: u64,
        amount: u128,
        slot: SlotId,
    ) -> Result<(), StakingError> {
        let total = self.total_stake.get(&guardian).copied().unwrap_or(0);
        // Mevcut sözleşme lock'larının toplamı
        let existing_contract_locks: u128 = self.holds
            .get(&guardian)
            .map(|hs| hs.iter()
                .filter(|h| matches!(h.reason, HoldReason::ActiveContract { .. }))
                .map(|h| h.amount)
                .sum())
            .unwrap_or(0);
        if total < existing_contract_locks + amount {
            return Err(StakingError::InsufficientStake {
                required: existing_contract_locks + amount,
                available: total,
            });
        }
        self.holds.entry(guardian).or_default().push(HoldEntry {
            reason: HoldReason::ActiveContract { contract_id },
            amount,
            locked_at_slot: slot.0,
            unlock_at_slot: None,
        });
        Ok(())
    }

    /// Sözleşme teminatını serbest bırak.
    pub fn release_contract_lock(&mut self, guardian: Address, contract_id: u64) -> bool {
        if let Some(holds) = self.holds.get_mut(&guardian) {
            let before = holds.len();
            holds.retain(|h| h.reason != HoldReason::ActiveContract { contract_id });
            return holds.len() < before;
        }
        false
    }

    /// Freeze ekle (transfer engelle).
    pub fn freeze(&mut self, guardian: Address, reason: FreezeReason, slot: SlotId) {
        self.freezes.entry(guardian).or_default().push(FreezeEntry {
            reason,
            frozen_at_slot: slot.0,
        });
    }

    /// Freeze kaldır.
    pub fn unfreeze(&mut self, guardian: Address, reason: &FreezeReason) -> bool {
        if let Some(freezes) = self.freezes.get_mut(&guardian) {
            let before = freezes.len();
            freezes.retain(|f| &f.reason != reason);
            return freezes.len() < before;
        }
        false
    }

    /// Slash: hold miktarından kes.
    ///
    /// Hold edilmiş stake slash edilebilir — bu Polkadot'un temel ilkesi.
    pub fn slash(
        &mut self,
        guardian: Address,
        amount: u128,
        slot: SlotId,
    ) -> Result<u128, StakingError> {
        let total = self.total_stake.get(&guardian).copied().unwrap_or(0);
        if total < amount {
            return Err(StakingError::InsufficientStake {
                required: amount,
                available: total,
            });
        }

        let slashed = amount.min(total);
        *self.total_stake.entry(guardian).or_insert(0) =
            total.saturating_sub(slashed);

        // ExistentialDeposit kontrolü
        let remaining = self.total_stake[&guardian];
        if remaining > 0 && remaining < GUARDIAN_EXISTENTIAL_DEPOSIT {
            // Slash sonrası existential deposit'in altına düştü → sıfırla
            self.total_stake.insert(guardian, 0);
            return Ok(slashed + remaining); // kalan da slash edildi
        }

        // Staking hold'u güncelle
        if let Some(holds) = self.holds.get_mut(&guardian) {
            let mut remaining_slash = slashed;
            for hold in holds.iter_mut() {
                if hold.reason == HoldReason::GuardianStaking && remaining_slash > 0 {
                    let cut = remaining_slash.min(hold.amount);
                    hold.amount -= cut;
                    remaining_slash -= cut;
                }
            }
            holds.retain(|h| h.amount > 0);
        }

        Ok(slashed)
    }

    /// Serbest (transfer edilebilir) bakiye.
    pub fn free_balance(&self, guardian: &Address) -> u128 {
        let total = self.total_stake.get(guardian).copied().unwrap_or(0);
        let held: u128 = self.holds.get(guardian)
            .map(|hs| hs.iter().map(|h| h.amount).sum())
            .unwrap_or(0);
        total.saturating_sub(held)
    }

    /// Toplam stake.
    pub fn total_balance(&self, guardian: &Address) -> u128 {
        self.total_stake.get(guardian).copied().unwrap_or(0)
    }

    /// Transfer engellenmiş mi?
    pub fn is_frozen(&self, guardian: &Address) -> bool {
        self.freezes.get(guardian).map(|fs| !fs.is_empty()).unwrap_or(false)
    }

    /// ExistentialDeposit altında mı?
    pub fn is_below_existential(&self, guardian: &Address) -> bool {
        let bal = self.total_balance(guardian);
        bal > 0 && bal < GUARDIAN_EXISTENTIAL_DEPOSIT
    }

    /// Hold listesi.
    pub fn holds(&self, guardian: &Address) -> &[HoldEntry] {
        self.holds.get(guardian).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Freeze listesi.
    pub fn freezes(&self, guardian: &Address) -> &[FreezeEntry] {
        self.freezes.get(guardian).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

// ─── STAKING HOOKS ────────────────────────────────────────────────────────────

/// AŞAMA 8 Görev 60: Guardian staking olay kancası.
///
/// Cosmos SDK `MultiStakingHooks` pattern: her hook sırayla çağrılır.
pub trait GuardianStakingHook: Send + Sync {
    fn after_guardian_bonded(&self, guardian: Address, amount: u128, slot: SlotId) {}
    fn after_guardian_slashed(&self, guardian: Address, amount: u128, slot: SlotId) {}
    fn before_guardian_unbonded(&self, guardian: Address, slot: SlotId) {}
    fn after_existential_deposit_violated(&self, guardian: Address, slot: SlotId) {}
}

/// Çoklu hook zinciri.
pub struct MultiGuardianStakingHooks {
    hooks: Vec<Arc<dyn GuardianStakingHook>>,
}

impl MultiGuardianStakingHooks {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn add_hook(&mut self, hook: Arc<dyn GuardianStakingHook>) {
        self.hooks.push(hook);
    }

    pub fn after_bonded(&self, guardian: Address, amount: u128, slot: SlotId) {
        for h in &self.hooks { h.after_guardian_bonded(guardian, amount, slot); }
    }

    pub fn after_slashed(&self, guardian: Address, amount: u128, slot: SlotId) {
        for h in &self.hooks { h.after_guardian_slashed(guardian, amount, slot); }
    }

    pub fn before_unbonded(&self, guardian: Address, slot: SlotId) {
        for h in &self.hooks { h.before_guardian_unbonded(guardian, slot); }
    }

    pub fn after_existential_violated(&self, guardian: Address, slot: SlotId) {
        for h in &self.hooks { h.after_existential_deposit_violated(guardian, slot); }
    }
}

impl Default for MultiGuardianStakingHooks {
    fn default() -> Self { Self::new() }
}

// ─── SEND RESTRICTION ─────────────────────────────────────────────────────────

/// AŞAMA 8 Görev 61: Transfer kısıtlama zinciri.
///
/// Polkadot `frame/balances/src/lib.rs` `RestrictedTransfer` pattern.
/// Her restriction kontrol eder — herhangi biri reddederse transfer durur.
pub trait SendRestriction: Send + Sync {
    /// Transfer izni ver ya da reddet.
    fn can_transfer(
        &self,
        from: Address,
        to: Address,
        amount: u128,
        slot: SlotId,
    ) -> Result<(), SendRestrictionError>;

    /// Kısıtlamanın adı (loglama için).
    fn name(&self) -> &'static str;
}

/// Transfer kısıtlama hatası.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendRestrictionError {
    #[error("gönderen dondurulmuş: {0:?}")]
    SenderFrozen(Address),

    #[error("yetersiz serbest bakiye: gerekli={required} mevcut={available}")]
    InsufficientFreeBalance { required: u128, available: u128 },

    #[error("existential deposit ihlali: {0:?}")]
    ExistentialDepositViolation(Address),

    #[error("kısıtlama '{name}' tarafından reddedildi: {reason}")]
    RestrictedBy { name: &'static str, reason: String },
}

/// Kısıtlama zinciri yöneticisi.
pub struct SendRestrictionChain {
    restrictions: Vec<Arc<dyn SendRestriction>>,
}

impl SendRestrictionChain {
    pub fn new() -> Self {
        Self { restrictions: Vec::new() }
    }

    pub fn add(&mut self, r: Arc<dyn SendRestriction>) {
        self.restrictions.push(r);
    }

    /// Tüm kısıtlamaları uygula.
    pub fn check_transfer(
        &self,
        from: Address,
        to: Address,
        amount: u128,
        slot: SlotId,
    ) -> Result<(), SendRestrictionError> {
        for r in &self.restrictions {
            r.can_transfer(from, to, amount, slot)?;
        }
        Ok(())
    }
}

impl Default for SendRestrictionChain {
    fn default() -> Self { Self::new() }
}

// ─── FreezeRestriction: dondurulmuş hesaplar transfer yapamaz ────────────────

/// Dondurulmuş guardian transfer edemez.
pub struct FreezeRestriction {
    manager: Arc<Mutex<GuardianStakeManager>>,
}

impl FreezeRestriction {
    pub fn new(manager: Arc<Mutex<GuardianStakeManager>>) -> Self {
        Self { manager }
    }
}

impl SendRestriction for FreezeRestriction {
    fn can_transfer(
        &self,
        from: Address,
        _to: Address,
        _amount: u128,
        _slot: SlotId,
    ) -> Result<(), SendRestrictionError> {
        let mgr = self.manager.lock().unwrap();
        if mgr.is_frozen(&from) {
            return Err(SendRestrictionError::SenderFrozen(from));
        }
        Ok(())
    }

    fn name(&self) -> &'static str { "FreezeRestriction" }
}

/// ExistentialDeposit kısıtlaması — transfer sonrası ED altına düşemez.
pub struct ExistentialDepositRestriction {
    manager: Arc<Mutex<GuardianStakeManager>>,
}

impl ExistentialDepositRestriction {
    pub fn new(manager: Arc<Mutex<GuardianStakeManager>>) -> Self {
        Self { manager }
    }
}

impl SendRestriction for ExistentialDepositRestriction {
    fn can_transfer(
        &self,
        from: Address,
        _to: Address,
        amount: u128,
        _slot: SlotId,
    ) -> Result<(), SendRestrictionError> {
        let mgr = self.manager.lock().unwrap();
        let free = mgr.free_balance(&from);
        if free < amount {
            return Err(SendRestrictionError::InsufficientFreeBalance {
                required: amount,
                available: free,
            });
        }
        let remaining = mgr.total_balance(&from).saturating_sub(amount);
        if remaining > 0 && remaining < GUARDIAN_EXISTENTIAL_DEPOSIT {
            return Err(SendRestrictionError::ExistentialDepositViolation(from));
        }
        Ok(())
    }

    fn name(&self) -> &'static str { "ExistentialDepositRestriction" }
}

// ─── HATALAR ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StakingError {
    #[error("yetersiz stake: gerekli={required} mevcut={available}")]
    InsufficientStake { required: u128, available: u128 },

    #[error("yetersiz serbest bakiye: gerekli={required} mevcut={available}")]
    InsufficientFreeBalance { required: u128, available: u128 },

    #[error("existential deposit altına düşülemez")]
    ExistentialDepositViolation,
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address { Address::from_bytes([n; 20]) }
    fn slot(n: u64) -> SlotId { SlotId(n) }

    // ─── Hold/Freeze Testleri ─────────────────────────────────────────────────

    #[test]
    fn test_bond_and_hold() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        mgr.bond(g, 5_000 * 1_000_000_000_000_000_000u128, slot(0));
        assert_eq!(mgr.holds(&g).len(), 1);
        assert!(mgr.holds(&g)[0].amount > 0);
    }

    #[test]
    fn test_slash_reduces_stake() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        let stake = 1_000 * 1_000_000_000_000_000_000u128;
        mgr.bond(g, stake, slot(0));

        let slashed = mgr.slash(g, stake / 10, slot(100)).unwrap();
        assert_eq!(slashed, stake / 10);
        assert_eq!(mgr.total_balance(&g), stake - stake / 10);
    }

    #[test]
    fn test_slash_below_existential_zeroes_balance() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        // 110 NSC bond
        let stake = 110 * 1_000_000_000_000_000_000u128;
        mgr.bond(g, stake, slot(0));

        // 50 NSC slash → 60 NSC kalır (ED=100 NSC altı) → tümü slash edilir
        let slashed = mgr.slash(g, 50 * 1_000_000_000_000_000_000u128, slot(100)).unwrap();
        assert!(slashed > 50 * 1_000_000_000_000_000_000u128); // kalan da alındı
        assert_eq!(mgr.total_balance(&g), 0);
    }

    #[test]
    fn test_freeze_blocks_transfer_check() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        mgr.bond(g, 1_000 * 1_000_000_000_000_000_000u128, slot(0));
        assert!(!mgr.is_frozen(&g));

        mgr.freeze(g, FreezeReason::PendingSlash, slot(50));
        assert!(mgr.is_frozen(&g));

        mgr.unfreeze(g, &FreezeReason::PendingSlash);
        assert!(!mgr.is_frozen(&g));
    }

    #[test]
    fn test_contract_lock_and_release() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        let stake = 2_000 * 1_000_000_000_000_000_000u128;
        mgr.bond(g, stake, slot(0));

        // Sözleşme teminatı — overlay hold (toplam stake'e karşı kontrol edilir)
        let lock_amount = 500 * 1_000_000_000_000_000_000u128;
        mgr.lock_for_contract(g, 42, lock_amount, slot(10)).unwrap();

        // Toplam stake değişmedi (overlay), hold sayısı arttı
        assert_eq!(mgr.total_balance(&g), stake);
        assert_eq!(mgr.holds(&g).len(), 2); // staking + contract

        // Kilit açıldı → sözleşme hold'u gitti, staking hold kaldı
        mgr.release_contract_lock(g, 42);
        assert_eq!(mgr.holds(&g).len(), 1);
        assert!(!mgr.holds(&g).iter().any(|h| h.reason == HoldReason::ActiveContract { contract_id: 42 }));
    }

    #[test]
    fn test_existential_deposit_check() {
        let mut mgr = GuardianStakeManager::new();
        let g = addr(1);
        mgr.bond(g, GUARDIAN_EXISTENTIAL_DEPOSIT - 1, slot(0));
        assert!(mgr.is_below_existential(&g));

        mgr.bond(g, 1, slot(1));
        assert!(!mgr.is_below_existential(&g)); // Tam ED'de → ok
    }

    // ─── MultiGuardianStakingHooks Testleri ──────────────────────────────────

    #[test]
    fn test_multi_hooks_call_all() {
        use std::sync::atomic::{AtomicU32, Ordering};

        struct CounterHook(Arc<AtomicU32>);
        impl GuardianStakingHook for CounterHook {
            fn after_guardian_bonded(&self, _: Address, _: u128, _: SlotId) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicU32::new(0));
        let mut hooks = MultiGuardianStakingHooks::new();
        hooks.add_hook(Arc::new(CounterHook(counter.clone())));
        hooks.add_hook(Arc::new(CounterHook(counter.clone())));

        hooks.after_bonded(addr(1), 100, slot(0));
        assert_eq!(counter.load(Ordering::SeqCst), 2); // Her iki hook da çağrıldı
    }

    // ─── SendRestriction Testleri ─────────────────────────────────────────────

    #[test]
    fn test_freeze_restriction_blocks_frozen() {
        let mgr = Arc::new(Mutex::new(GuardianStakeManager::new()));
        let g = addr(1);
        {
            let mut m = mgr.lock().unwrap();
            m.bond(g, 1_000 * 1_000_000_000_000_000_000u128, slot(0));
            m.freeze(g, FreezeReason::PendingSlash, slot(10));
        }

        let restriction = FreezeRestriction::new(mgr.clone());
        let result = restriction.can_transfer(g, addr(2), 100, slot(20));
        assert!(matches!(result, Err(SendRestrictionError::SenderFrozen(_))));
    }

    #[test]
    fn test_send_restriction_chain() {
        let mgr = Arc::new(Mutex::new(GuardianStakeManager::new()));
        let g = addr(1);
        {
            let mut m = mgr.lock().unwrap();
            m.bond(g, 500 * 1_000_000_000_000_000_000u128, slot(0));
        }

        let mut chain = SendRestrictionChain::new();
        chain.add(Arc::new(FreezeRestriction::new(mgr.clone())));
        chain.add(Arc::new(ExistentialDepositRestriction::new(mgr.clone())));

        // Transfer ok (serbest bakiye yok çünkü hepsi hold'da, ama freeze yok)
        let result = chain.check_transfer(g, addr(2), 1, slot(10));
        // Hold nedeniyle free_balance=0 → InsufficientFreeBalance
        assert!(result.is_err());
    }

    #[test]
    fn test_existential_deposit_restriction() {
        let mgr = Arc::new(Mutex::new(GuardianStakeManager::new()));
        let g = addr(1);
        {
            let mut m = mgr.lock().unwrap();
            // 200 NSC bond, hold YOK (direkt total_stake)
            m.total_stake.insert(g, 200 * 1_000_000_000_000_000_000u128);
        }

        let r = ExistentialDepositRestriction::new(mgr.clone());
        // 150 NSC transfer → 50 NSC kalır (ED=100 altı) → engellenmeli
        let result = r.can_transfer(
            g, addr(2),
            150 * 1_000_000_000_000_000_000u128,
            slot(0),
        );
        assert!(matches!(result, Err(SendRestrictionError::ExistentialDepositViolation(_))));

        // 100 NSC transfer → 100 NSC kalır (tam ED) → izin verilir
        let result2 = r.can_transfer(
            g, addr(2),
            100 * 1_000_000_000_000_000_000u128,
            slot(0),
        );
        assert!(result2.is_ok());
    }
}
