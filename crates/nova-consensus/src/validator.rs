//! Validator kayıt ve yönetim sistemi — §5.10 DPoS
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/staking Keeper:
//!   `SetValidator(ctx, validator)`, `GetValidator(ctx, addr)`,
//!   `Jail(ctx, consAddr)`, `Unjail(ctx, consAddr)`,
//!   `Slash(ctx, consAddr, height, power, factor)`.
//!   Bizde: `ValidatorRegistry::register`, `jail`, `unjail`, `slash`.
//! - Cosmos SDK x/slashing `HandleValidatorSignature`:
//!   `MissedBlocksCounter`, `IndexOffset`, bitmap sliding window.
//!   Bizde: `record_block_signature` — her blok için imza takibi.
//! - Avalanchego simplex `validators.Manager`:
//!   `AddStaker`, `GetWeight`, `GetValidators`.
//!   Bizde: `ValidatorRegistry::get_active_validators`.
//! - Polkadot pallet-staking `Exposure { total, own, others }`:
//!   Validator stake dağılımı. Bizde: `ValidatorInfo::stake`.

use crate::errors::{ConsensusError, ConsensusResult};
use crate::types::{
    SlashReason, ValidatorInfo, ValidatorStatus, EPOCH_DURATION_SLOTS, MIN_SIGNED_PER_WINDOW_BPS,
    MIN_VALIDATOR_STAKE, SIGNED_BLOCKS_WINDOW, UNBONDING_PERIOD_SLOTS,
};
use dashmap::DashMap;
use nova_primitives::{Address, Balance, BlsPublicKey, SlotId, ValidatorId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

// ─── ValidatorRegistry ────────────────────────────────────────────────────────

/// Tüm validator'ların kayıt ve durum yöneticisi.
///
/// Pattern: Cosmos SDK x/staking Keeper — per-address state yönetimi.
/// DashMap ile thread-safe, lock-free concurrent erişim.
/// Avalanchego validators.Manager benzeri public API.
pub struct ValidatorRegistry {
    /// Tüm validator'lar (aktif + pasif).
    validators: Arc<DashMap<ValidatorId, ValidatorInfo>>,
}

impl ValidatorRegistry {
    pub fn new() -> Self {
        Self {
            validators: Arc::new(DashMap::new()),
        }
    }

    // ─── Kayıt ────────────────────────────────────────────────────────────

    /// Yeni validator kaydet.
    ///
    /// Pattern: Cosmos SDK x/staking `MsgCreateValidator` handler:
    /// minimum stake kontrolü → validator oluştur → store'a yaz.
    pub fn register(
        &self,
        validator_id: ValidatorId,
        operator: Address,
        stake: Balance,
        commission_bps: u32,
        signing_key: [u8; 32],
        current_slot: SlotId,
    ) -> ConsensusResult<()> {
        if self.validators.contains_key(&validator_id) {
            return Err(ConsensusError::ValidatorAlreadyRegistered {
                validator_id: validator_id.0.to_string(),
            });
        }

        if stake.0 < MIN_VALIDATOR_STAKE {
            return Err(ConsensusError::InsufficientStake {
                required: MIN_VALIDATOR_STAKE,
                provided: stake.0,
            });
        }

        let info = ValidatorInfo::new(
            validator_id,
            operator,
            stake,
            commission_bps,
            current_slot,
            signing_key,
        );

        self.validators.insert(validator_id, info);

        info!(
            target: "nova_consensus",
            validator = %validator_id.0.to_string(),
            stake = stake.0,
            "validator kayıt edildi"
        );

        Ok(())
    }

    // ─── Stake Güncelleme ─────────────────────────────────────────────────

    /// Validator stake'ini artır.
    pub fn add_stake(
        &self,
        validator_id: &ValidatorId,
        amount: u128,
    ) -> ConsensusResult<()> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;
        v.stake = Balance(v.stake.0.saturating_add(amount));
        Ok(())
    }

    /// Unbonding başlat — stake çekme kilidi.
    ///
    /// Cosmos SDK x/staking `UnbondingDelegation` pattern.
    pub fn begin_unbonding(
        &self,
        validator_id: &ValidatorId,
        current_slot: SlotId,
    ) -> ConsensusResult<SlotId> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;

        if matches!(v.status, ValidatorStatus::Tombstoned) {
            return Err(ConsensusError::ValidatorTombstoned);
        }

        // NOVA-RED-CONSENSUS-001: Jailed validator unbonding ile jail'den kaçamaz.
        // Cosmos SDK x/staking: jailed validator unbond isteği reddedilir.
        if let ValidatorStatus::Jailed { jailed_until_slot } = v.status {
            return Err(ConsensusError::ValidatorJailed { jailed_until_slot });
        }

        let unlock_at = SlotId(current_slot.0 + UNBONDING_PERIOD_SLOTS);
        v.status = ValidatorStatus::PendingUnbonding {
            unlock_at_slot: unlock_at.0,
        };

        Ok(unlock_at)
    }

    // ─── Blok İmza Takibi ─────────────────────────────────────────────────

    /// Blok imza kaydını güncelle.
    ///
    /// Pattern: Cosmos SDK x/slashing `HandleValidatorSignature`:
    /// Bitmap sliding window ile missed/signed takibi.
    /// `IndexOffset` modulo `SignedBlocksWindow` → bu slot'taki konum.
    /// `MissedBlocksCounter` → toplam kaçırılan blok.
    ///
    /// Return: downtime slash gerekiyor mu? (min signed eşiği altında)
    pub fn record_block_signature(
        &self,
        validator_id: &ValidatorId,
        signed: bool,
        current_slot: SlotId,
    ) -> ConsensusResult<bool> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;

        // Jaillenmiş validator'ı atla
        if matches!(v.status, ValidatorStatus::Jailed { .. } | ValidatorStatus::Tombstoned) {
            return Ok(false);
        }

        v.index_offset = (v.index_offset + 1) % SIGNED_BLOCKS_WINDOW;

        if signed {
            v.last_active_slot = current_slot;
            // Eğer bu slot daha önce "missed" olarak işaretlendiyse düzelt
            // (Basitleştirilmiş: tam bitmap yok, sadece counter)
            if v.missed_blocks_counter > 0 {
                v.missed_blocks_counter -= 1;
            }
        } else {
            v.missed_blocks_counter += 1;
        }

        // Eşik kontrolü: missed_blocks_counter > maxMissed?
        // maxMissed = window * (1 - min_signed_fraction)
        let max_missed = SIGNED_BLOCKS_WINDOW * (10_000 - MIN_SIGNED_PER_WINDOW_BPS) / 10_000;
        let needs_slash = v.missed_blocks_counter > max_missed
            && !matches!(v.status, ValidatorStatus::Jailed { .. });

        Ok(needs_slash)
    }

    // ─── Jail / Unjail ────────────────────────────────────────────────────

    /// Validator'ı jail'e at.
    ///
    /// Cosmos SDK x/slashing `Jail(ctx, consAddr)` pattern.
    pub fn jail(
        &self,
        validator_id: &ValidatorId,
        jailed_until_slot: SlotId,
    ) -> ConsensusResult<()> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;

        if matches!(v.status, ValidatorStatus::Tombstoned) {
            return Err(ConsensusError::ValidatorTombstoned);
        }

        v.status = ValidatorStatus::Jailed {
            jailed_until_slot: jailed_until_slot.0,
        };
        // Missed counter sıfırla — Cosmos SDK pattern
        v.missed_blocks_counter = 0;
        v.index_offset = 0;

        warn!(
            target: "nova_consensus",
            validator = %validator_id.0.to_string(),
            until_slot = jailed_until_slot.0,
            "validator jail edildi"
        );

        Ok(())
    }

    /// Validator'ı jail'den çıkar.
    ///
    /// Cosmos SDK x/slashing `Unjail(ctx, consAddr)` + unjail.go:
    /// Jail süresi dolmadan unjail edilemez.
    pub fn unjail(
        &self,
        validator_id: &ValidatorId,
        current_slot: SlotId,
    ) -> ConsensusResult<()> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;

        match &v.status {
            ValidatorStatus::Jailed { jailed_until_slot } => {
                if current_slot.0 < *jailed_until_slot {
                    return Err(ConsensusError::ValidatorJailed {
                        jailed_until_slot: *jailed_until_slot,
                    });
                }
                v.status = ValidatorStatus::Active;
                info!(
                    target: "nova_consensus",
                    validator = %validator_id.0.to_string(),
                    "validator jail'den çıktı"
                );
                Ok(())
            }
            ValidatorStatus::Tombstoned => Err(ConsensusError::ValidatorTombstoned),
            _ => Ok(()),
        }
    }

    // ─── Tombstone ────────────────────────────────────────────────────────

    /// Validator'ı kalıcı olarak kaldır (double-sign, mempool leak).
    ///
    /// Cosmos SDK x/evidence `Tombstone(ctx, consAddr)`:
    /// "Once tombstoned, the validator will not be able to recover."
    pub fn tombstone(&self, validator_id: &ValidatorId) -> ConsensusResult<()> {
        let mut v = self.validators
            .get_mut(validator_id)
            .ok_or_else(|| ConsensusError::ValidatorNotFound {
                validator_id: validator_id.0.to_string(),
            })?;

        v.status = ValidatorStatus::Tombstoned;

        warn!(
            target: "nova_consensus",
            validator = %validator_id.0.to_string(),
            "validator tombstone edildi (kalıcı)"
        );

        Ok(())
    }

    // ─── Slash ────────────────────────────────────────────────────────────

    /// Validator'ı slash et.
    ///
    /// Cosmos SDK x/slashing `SlashWithInfractionReason`:
    /// `fraction` bps cinsinden → slashed_amount = stake * fraction / 10_000.
    /// Stake'ten düşürülür, kalan MIN_VALIDATOR_STAKE altına düşerse deaktive.
    pub fn slash(
        &self,
        validator_id: &ValidatorId,
        slash_bps: u32,
        reason: SlashReason,
        current_slot: SlotId,
    ) -> ConsensusResult<u128> {
        let slashed_amount = {
            let v = self.validators
                .get(validator_id)
                .ok_or_else(|| ConsensusError::ValidatorNotFound {
                    validator_id: validator_id.0.to_string(),
                })?;
            v.stake.0 * slash_bps as u128 / 10_000
        };

        {
            let mut v = self.validators.get_mut(validator_id).unwrap();
            v.stake = Balance(v.stake.0.saturating_sub(slashed_amount));

            // Stake minimum altına düştüyse → Inactive
            if v.stake.0 < MIN_VALIDATOR_STAKE
                && !matches!(
                    v.status,
                    ValidatorStatus::Tombstoned | ValidatorStatus::Jailed { .. }
                )
            {
                v.status = ValidatorStatus::Inactive;
            }
        }

        warn!(
            target: "nova_consensus",
            validator = %validator_id.0.to_string(),
            slash_bps = slash_bps,
            slashed = slashed_amount,
            reason = ?reason,
            "validator slash edildi"
        );

        Ok(slashed_amount)
    }

    // ─── BLS Pubkey Kaydı ─────────────────────────────────────────────────

    /// Validator'ın BLS public key'ini kaydet (equivocation raporlama için).
    ///
    /// Polkadot `KeyOwnerProofSystem::insert_authority_key()` pattern.
    /// Validator register edildikten sonra BLS keypair oluşturulup pubkey kayıt edilir.
    /// Equivocation raporunda `bls_pubkey` ile gelen imza, bu pubkey ile doğrulanır.
    pub fn set_bls_pubkey(&self, validator_id: ValidatorId, pubkey: BlsPublicKey) {
        if let Some(mut v) = self.validators.get_mut(&validator_id) {
            v.bls_pubkey = Some(pubkey);
        }
    }

    // ─── Sorgu ────────────────────────────────────────────────────────────

    /// Validator bilgisini al.
    pub fn get(&self, validator_id: &ValidatorId) -> Option<ValidatorInfo> {
        self.validators.get(validator_id).map(|v| v.clone())
    }

    /// Validator var mı?
    pub fn exists(&self, validator_id: &ValidatorId) -> bool {
        self.validators.contains_key(validator_id)
    }

    /// Tüm aktif validator'ları al (stake ağırlıklı sıralı).
    ///
    /// Polkadot pallet-staking `get_elected_validators` → stake azalan sıra.
    pub fn get_active_validators(&self) -> Vec<ValidatorInfo> {
        let mut active: Vec<ValidatorInfo> = self
            .validators
            .iter()
            .filter(|e| matches!(e.status, ValidatorStatus::Active))
            .map(|e| e.clone())
            .collect();
        active.sort_by(|a, b| b.stake.0.cmp(&a.stake.0));
        active
    }

    /// Aktif validator sayısı.
    pub fn active_count(&self) -> usize {
        self.validators
            .iter()
            .filter(|e| matches!(e.status, ValidatorStatus::Active))
            .count()
    }

    /// Toplam kayıtlı validator sayısı.
    pub fn total_count(&self) -> usize {
        self.validators.len()
    }

    /// Validator'ın oy ağırlığı.
    pub fn vote_weight(&self, validator_id: &ValidatorId) -> u128 {
        self.validators
            .get(validator_id)
            .map(|v| v.stake.0)
            .unwrap_or(0)
    }

    /// Toplam aktif stake.
    pub fn total_active_stake(&self) -> u128 {
        self.validators
            .iter()
            .filter(|e| matches!(e.status, ValidatorStatus::Active))
            .map(|e| e.stake.0)
            .sum()
    }

    /// Belirli bir validator setinin toplam ağırlığı.
    pub fn total_weight_of(&self, ids: &[ValidatorId]) -> u128 {
        ids.iter()
            .map(|id| self.vote_weight(id))
            .sum()
    }
}

impl Default for ValidatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{Address, Balance, SlotId, ValidatorId};

    fn vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn enough_stake() -> Balance {
        Balance(MIN_VALIDATOR_STAKE)
    }

    fn register_validator(registry: &ValidatorRegistry, seed: u8) -> ValidatorId {
        let id = vid(seed);
        registry
            .register(id, addr(seed), enough_stake(), 500, [seed; 32], SlotId(0))
            .unwrap();
        id
    }

    // ─── register ─────────────────────────────────────────────────────────

    #[test]
    fn test_register_success() {
        let registry = ValidatorRegistry::new();
        let id = vid(1);
        let result = registry.register(id, addr(1), enough_stake(), 500, [1u8; 32], SlotId(0));
        assert!(result.is_ok());
        assert!(registry.exists(&id));
    }

    #[test]
    fn test_register_duplicate_fails() {
        let registry = ValidatorRegistry::new();
        register_validator(&registry, 1);
        let err = registry
            .register(vid(1), addr(1), enough_stake(), 500, [1u8; 32], SlotId(0))
            .unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorAlreadyRegistered { .. }));
    }

    #[test]
    fn test_register_insufficient_stake_fails() {
        let registry = ValidatorRegistry::new();
        let err = registry
            .register(vid(2), addr(2), Balance(1_000), 500, [2u8; 32], SlotId(0))
            .unwrap_err();
        assert!(matches!(err, ConsensusError::InsufficientStake { .. }));
    }

    #[test]
    fn test_register_sets_active_status() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 3);
        let info = registry.get(&id).unwrap();
        assert!(matches!(info.status, ValidatorStatus::Active));
    }

    // ─── add_stake ────────────────────────────────────────────────────────

    #[test]
    fn test_add_stake_success() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 4);
        let original_stake = registry.get(&id).unwrap().stake.0;

        registry.add_stake(&id, 1_000_000).unwrap();
        let new_stake = registry.get(&id).unwrap().stake.0;
        assert_eq!(new_stake, original_stake + 1_000_000);
    }

    #[test]
    fn test_add_stake_not_found_fails() {
        let registry = ValidatorRegistry::new();
        let err = registry.add_stake(&vid(99), 1_000).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorNotFound { .. }));
    }

    // ─── jail / unjail ────────────────────────────────────────────────────

    #[test]
    fn test_jail_and_unjail_success() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 5);

        registry.jail(&id, SlotId(10_000)).unwrap();
        let info = registry.get(&id).unwrap();
        assert!(matches!(info.status, ValidatorStatus::Jailed { jailed_until_slot: 10_000 }));

        registry.unjail(&id, SlotId(10_000)).unwrap();
        let info2 = registry.get(&id).unwrap();
        assert!(matches!(info2.status, ValidatorStatus::Active));
    }

    #[test]
    fn test_unjail_too_early_fails() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 6);
        registry.jail(&id, SlotId(10_000)).unwrap();

        let err = registry.unjail(&id, SlotId(9_999)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorJailed { jailed_until_slot: 10_000 }));
    }

    #[test]
    fn test_jail_resets_missed_blocks() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 7);

        // Birkaç blok kaçır
        for _ in 0..5 {
            registry.record_block_signature(&id, false, SlotId(100)).unwrap();
        }
        assert!(registry.get(&id).unwrap().missed_blocks_counter > 0);

        // Jail → counter sıfırlanır
        registry.jail(&id, SlotId(1000)).unwrap();
        assert_eq!(registry.get(&id).unwrap().missed_blocks_counter, 0);
    }

    // ─── tombstone ────────────────────────────────────────────────────────

    #[test]
    fn test_tombstone_permanent() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 8);
        registry.tombstone(&id).unwrap();

        let info = registry.get(&id).unwrap();
        assert!(matches!(info.status, ValidatorStatus::Tombstoned));
    }

    #[test]
    fn test_unjail_tombstoned_fails() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 9);
        registry.tombstone(&id).unwrap();

        let err = registry.unjail(&id, SlotId(0)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorTombstoned));
    }

    #[test]
    fn test_jail_tombstoned_fails() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 10);
        registry.tombstone(&id).unwrap();

        let err = registry.jail(&id, SlotId(10_000)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorTombstoned));
    }

    // ─── slash ────────────────────────────────────────────────────────────

    // ─── begin_unbonding / jail escape (NOVA-RED-CONSENSUS-001) ──────────────

    #[test]
    fn test_begin_unbonding_active_validator_ok() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 20);
        let result = registry.begin_unbonding(&id, SlotId(100));
        assert!(result.is_ok(), "Aktif validator unbonding başlatabilmeli");
    }

    /// NOVA-RED-CONSENSUS-001 regresyon: Jailed validator unbonding ile jail'den kaçamaz.
    #[test]
    fn test_jailed_validator_cannot_begin_unbonding() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 21);
        // Önce jail'e at
        registry.jail(&id, SlotId(100_000)).unwrap();

        // Unbonding başlatmaya çalış — reddedilmeli
        let err = registry.begin_unbonding(&id, SlotId(500)).unwrap_err();
        assert!(
            matches!(err, ConsensusError::ValidatorJailed { .. }),
            "Jailed validator unbonding başlatamaz: {err:?}"
        );
    }

    #[test]
    fn test_tombstoned_validator_cannot_begin_unbonding() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 22);
        registry.tombstone(&id).unwrap();

        let err = registry.begin_unbonding(&id, SlotId(500)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorTombstoned));
    }

    #[test]
    fn test_slash_reduces_stake() {
        let registry = ValidatorRegistry::new();
        let id = vid(11);
        // Çok büyük stake — slash sonrası minimumun üzerinde kalacak
        registry
            .register(id, addr(11), Balance(1_000_000 * 10u128.pow(18)), 500, [11; 32], SlotId(0))
            .unwrap();

        let original_stake = registry.get(&id).unwrap().stake.0;
        let slashed = registry
            .slash(&id, 2_500, SlashReason::WrongBatchClearing, SlotId(100))
            .unwrap();

        assert_eq!(slashed, original_stake * 2_500 / 10_000);
        let new_stake = registry.get(&id).unwrap().stake.0;
        assert_eq!(new_stake, original_stake - slashed);
    }

    #[test]
    fn test_slash_100pct_sets_inactive() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 12);

        // %100 slash → stake = 0 < minimum → Inactive
        registry
            .slash(&id, 10_000, SlashReason::DoubleSign, SlotId(100))
            .unwrap();

        let info = registry.get(&id).unwrap();
        assert!(matches!(info.status, ValidatorStatus::Inactive));
    }

    // ─── record_block_signature ───────────────────────────────────────────

    #[test]
    fn test_record_signature_signed_updates_last_active() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 13);

        registry
            .record_block_signature(&id, true, SlotId(500))
            .unwrap();

        assert_eq!(registry.get(&id).unwrap().last_active_slot, SlotId(500));
    }

    #[test]
    fn test_record_signature_missed_increments_counter() {
        let registry = ValidatorRegistry::new();
        let id = register_validator(&registry, 14);

        registry
            .record_block_signature(&id, false, SlotId(100))
            .unwrap();

        assert_eq!(registry.get(&id).unwrap().missed_blocks_counter, 1);
    }

    #[test]
    fn test_record_signature_too_many_missed_triggers_downtime() {
        let registry = ValidatorRegistry::new();
        let id = vid(15);
        registry
            .register(id, addr(15), enough_stake(), 0, [15; 32], SlotId(0))
            .unwrap();

        // MAX_MISSED = window * (1 - 0.5) = 216_000 * 0.5 = 108_000
        // Daha fazlasını simüle etmek için counter'ı doğrudan set edemeyiz
        // Bunun yerine missed threshold değerini hesaplayalım
        let max_missed =
            SIGNED_BLOCKS_WINDOW * (10_000 - MIN_SIGNED_PER_WINDOW_BPS) / 10_000;

        // Counter'ı max_missed değerine getir (kaçırılan blok kayıtla)
        for i in 0..=max_missed {
            let _ = registry.record_block_signature(&id, false, SlotId(i));
        }

        let info = registry.get(&id).unwrap();
        assert!(info.missed_blocks_counter > max_missed);
    }

    // ─── get_active_validators ────────────────────────────────────────────

    #[test]
    fn test_get_active_validators_sorted_by_stake() {
        let registry = ValidatorRegistry::new();

        let v1 = vid(20);
        let v2 = vid(21);
        let v3 = vid(22);

        registry
            .register(v1, addr(20), Balance(MIN_VALIDATOR_STAKE * 3), 0, [20; 32], SlotId(0))
            .unwrap();
        registry
            .register(v2, addr(21), Balance(MIN_VALIDATOR_STAKE * 1), 0, [21; 32], SlotId(0))
            .unwrap();
        registry
            .register(v3, addr(22), Balance(MIN_VALIDATOR_STAKE * 2), 0, [22; 32], SlotId(0))
            .unwrap();

        let active = registry.get_active_validators();
        assert_eq!(active.len(), 3);
        // Azalan sıra: v1 > v3 > v2
        assert_eq!(active[0].validator_id, v1);
        assert_eq!(active[1].validator_id, v3);
        assert_eq!(active[2].validator_id, v2);
    }

    #[test]
    fn test_get_active_excludes_jailed_tombstoned() {
        let registry = ValidatorRegistry::new();
        let v1 = register_validator(&registry, 23);
        let v2 = register_validator(&registry, 24);
        let v3 = register_validator(&registry, 25);

        registry.jail(&v2, SlotId(10_000)).unwrap();
        registry.tombstone(&v3).unwrap();

        let active = registry.get_active_validators();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].validator_id, v1);
    }

    #[test]
    fn test_total_active_stake() {
        let registry = ValidatorRegistry::new();
        let v1 = vid(30);
        let v2 = vid(31);
        registry
            .register(v1, addr(30), Balance(MIN_VALIDATOR_STAKE * 2), 0, [30; 32], SlotId(0))
            .unwrap();
        registry
            .register(v2, addr(31), Balance(MIN_VALIDATOR_STAKE * 3), 0, [31; 32], SlotId(0))
            .unwrap();

        assert_eq!(
            registry.total_active_stake(),
            MIN_VALIDATOR_STAKE * 5
        );
    }
}
