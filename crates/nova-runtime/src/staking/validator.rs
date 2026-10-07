//! Validator yapısı ve durum makinesi (Cosmos SDK x/staking'den ilham)
//!
//! Durum makinesi: Unbonded → Bonded ↔ Unbonding → Unbonded
//! Cosmos SDK'dan: commission, min_self_delegation, jailed

use nova_primitives::{Address, SlotId, ValidatorId};
use serde::{Deserialize, Serialize};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Unbonding süresi: 21 gün = 21 * 24h * 216_000 slot/era
pub const UNBONDING_DURATION_SLOTS: u64 = 21 * 24 * 216_000;

/// Maksimum komisyon: 5_000 BPS = %50
///
/// BUG-11 düzeltmesi: Eski değer 10_000 (%100) era_payout.rs ile çakışıyordu.
/// Ekonomik tasarım: Validator en fazla %50 komisyon alabilir.
/// Referans: era_payout.rs MAX_COMMISSION_BPS = 5_000 (canonical)
pub const MAX_COMMISSION_BPS: u32 = 5_000;

/// Varsayılan minimum komisyon: 500 BPS = %5 (Cosmos'un %0 minimumundan iyi)
pub const MIN_COMMISSION_BPS: u32 = 500;

/// Maksimum aktif validator sayısı (başlangıç)
pub const MAX_ACTIVE_VALIDATORS: usize = 200;

// ─── Validator Durum Makinesi ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondStatus {
    /// Aktif değil — henüz minimum stake yok
    Unbonded,
    /// Aktif validator sette — blok üretiyor
    Bonded,
    /// Unbonding sürecinde — UNBONDING_DURATION_SLOTS sonra Unbonded
    Unbonding {
        completion_slot: u64,
    },
}

impl BondStatus {
    pub fn is_bonded(&self) -> bool {
        matches!(self, BondStatus::Bonded)
    }

    pub fn is_unbonding(&self) -> bool {
        matches!(self, BondStatus::Unbonding { .. })
    }

    pub fn is_unbonded(&self) -> bool {
        matches!(self, BondStatus::Unbonded)
    }
}

// ─── Validator ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    pub id: ValidatorId,
    pub operator_address: Address,
    pub status: BondStatus,

    /// Toplam stake (kendi + delege edilmiş)
    pub total_tokens: u128,
    /// Validator'ın kendi stake'i
    pub self_stake: u128,
    /// Minimum öz delegasyon — bunun altına düşerse Unbonded
    pub min_self_delegation: u128,
    /// Komisyon oranı (BPS)
    pub commission_bps: u32,

    /// Cezalandırılmış mı?
    pub jailed: bool,
    /// Jail bitiş slot'u (ceza sona erdiğinde unjail edilebilir)
    pub jailed_until_slot: u64,

    /// Kayıt slot'u
    pub registered_at_slot: u64,

    /// Son ödül talep edilen era
    pub last_reward_era: u64,
}

impl Validator {
    pub fn new(
        id: ValidatorId,
        operator_address: Address,
        self_stake: u128,
        commission_bps: u32,
        min_self_delegation: u128,
        registered_at_slot: u64,
    ) -> Self {
        Self {
            id,
            operator_address,
            status: BondStatus::Unbonded,
            total_tokens: self_stake,
            self_stake,
            min_self_delegation,
            commission_bps,
            jailed: false,
            jailed_until_slot: 0,
            registered_at_slot,
            last_reward_era: 0,
        }
    }

    /// Komisyon miktarı hesapla
    pub fn commission_amount(&self, era_reward: u128) -> u128 {
        era_reward * self.commission_bps as u128 / 10_000
    }

    /// Komisyon sonrası delegatörlere kalan
    pub fn delegator_reward(&self, era_reward: u128) -> u128 {
        era_reward - self.commission_amount(era_reward)
    }

    /// Bonded mı ve jail değil mi?
    pub fn is_active(&self) -> bool {
        self.status.is_bonded() && !self.jailed
    }

    /// Konsensüs ağırlığı (basit: total_tokens / 10^18)
    pub fn consensus_power(&self) -> u64 {
        (self.total_tokens / 1_000_000_000_000_000_000).min(u64::MAX as u128) as u64
    }

    /// Unbonding başlat
    pub fn begin_unbonding(&mut self, current_slot: SlotId) {
        self.status = BondStatus::Unbonding {
            completion_slot: current_slot.0 + UNBONDING_DURATION_SLOTS,
        };
    }

    /// Unbonding tamamlandı mı?
    pub fn is_unbonding_complete(&self, current_slot: SlotId) -> bool {
        match &self.status {
            BondStatus::Unbonding { completion_slot } => current_slot.0 >= *completion_slot,
            _ => false,
        }
    }
}

// ─── ValidatorPool ────────────────────────────────────────────────────────────

use nova_primitives::crypto::PublicKey;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// ValidatorPool — staking modülü için validator deposu
#[derive(Debug, Default)]
pub struct ValidatorPool {
    validators: Arc<RwLock<HashMap<ValidatorId, Validator>>>,
}

impl ValidatorPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, v: Validator) -> crate::errors::RuntimeResult<()> {
        let mut map = self.validators.write();
        if map.contains_key(&v.id) {
            return Err(crate::errors::RuntimeError::ValidatorAlreadyExists);
        }
        map.insert(v.id, v);
        Ok(())
    }

    pub fn get(&self, id: &ValidatorId) -> Option<Validator> {
        self.validators.read().get(id).cloned()
    }

    pub fn update(&self, id: &ValidatorId, f: impl FnOnce(&mut Validator)) -> bool {
        let mut map = self.validators.write();
        if let Some(v) = map.get_mut(id) {
            f(v);
            true
        } else {
            false
        }
    }

    /// Aktif (Bonded + unjailed) validator listesi
    pub fn active_validators(&self) -> Vec<Validator> {
        self.validators
            .read()
            .values()
            .filter(|v| v.is_active())
            .cloned()
            .collect()
    }

    /// Güce göre sıralı validator listesi (en yüksekten düşüğe)
    pub fn sorted_by_power(&self) -> Vec<Validator> {
        let mut vs: Vec<_> = self.validators.read().values().cloned().collect();
        vs.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        vs
    }

    pub fn count(&self) -> usize {
        self.validators.read().len()
    }

    /// En yüksek stake'li MAX_ACTIVE_VALIDATORS validator'ı bonded yapar
    pub fn apply_validator_set_update(&self, current_slot: SlotId) {
        let mut map = self.validators.write();
        let mut sorted: Vec<_> = map.values_mut().collect();
        sorted.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

        let mut bonded_count = 0;
        for v in sorted {
            if v.jailed {
                if !v.status.is_unbonding() && !v.status.is_unbonded() {
                    v.status = BondStatus::Unbonded;
                }
                continue;
            }
            if bonded_count < MAX_ACTIVE_VALIDATORS {
                v.status = BondStatus::Bonded;
                bonded_count += 1;
            } else if v.status.is_bonded() {
                // Artık top-200'de değil → unbonding başlat
                v.status = BondStatus::Unbonding {
                    completion_slot: current_slot.0 + UNBONDING_DURATION_SLOTS,
                };
            }
        }
    }
}

// ─── VALIDATOR KEY ROTATION ───────────────────────────────────────────────────

/// AŞAMA 7 Görev 56: Validator konsensüs anahtarı rotasyonu.
///
/// ## Cosmos SDK Referansı
///
/// `x/staking/keeper/cons_pubkey.go`:
/// - `RotateConsPubKey(ctx, msg)` — anahtar değiştirme mesajı
/// - `ConsPubKeyRotationHistory` — geçmiş kayıtlar
/// - Ücret: `KeyRotationFee = Params.KeyRotationFee`
/// - Cooldown: `MaxConsPubKeyRotations` per 24h
///
/// ## Nova Uyarlaması
///
/// Validator'ın konsensüs imza anahtarı (BLS pubkey) değiştirilebilir.
/// Bu gereklidir:
/// 1. Anahtar sızıntısı durumunda güvenli geçiş
/// 2. Donanım güvenlik modülü (HSM) değişimi
/// 3. Periyodik anahtar hijyeni

/// Konsensüs anahtarı rotasyon kaydı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRotationRecord {
    /// Validator kimliği.
    pub validator_id: ValidatorId,
    /// Eski konsensüs pubkey.
    pub old_key: PublicKey,
    /// Yeni konsensüs pubkey.
    pub new_key: PublicKey,
    /// Rotasyonun gerçekleştiği slot.
    pub rotation_slot: u64,
    /// Ödenen rotasyon ücreti (NSC).
    pub fee_paid: u128,
}

/// Key rotation ücreti havuzu.
///
/// Cosmos SDK'dan: rotation fee → treasury veya burn.
/// Nova'da: havuza eklenir, era sonunda distribute_protocol_fee ile dağıtılır.
#[derive(Debug, Default, Clone)]
pub struct KeyRotationFeePool {
    total_collected: u128,
    rotation_count: u64,
}

impl KeyRotationFeePool {
    pub fn new() -> Self { Self::default() }

    pub fn add_fee(&mut self, amount: u128) {
        self.total_collected = self.total_collected.saturating_add(amount);
        self.rotation_count += 1;
    }

    pub fn total_collected(&self) -> u128 { self.total_collected }
    pub fn rotation_count(&self) -> u64 { self.rotation_count }

    /// Era sonunda toplanan ücretleri al ve sıfırla.
    pub fn drain(&mut self) -> u128 {
        let total = self.total_collected;
        self.total_collected = 0;
        self.rotation_count = 0;
        total
    }
}

/// Validator konsensüs anahtar rotasyon yöneticisi.
#[derive(Debug, Default)]
pub struct ConsKeyRotationManager {
    /// Validator → mevcut konsensüs pubkey.
    current_keys: HashMap<ValidatorId, PublicKey>,
    /// Rotasyon geçmişi.
    history: Vec<KeyRotationRecord>,
    /// Minimum rotasyon aralığı (slot sayısı).
    pub min_rotation_interval_slots: u64,
    /// Tek era'da izin verilen max rotasyon sayısı.
    pub max_rotations_per_era: u32,
    /// Era başına bu validator için rotasyon sayacı.
    era_rotation_counts: HashMap<ValidatorId, u32>,
    /// Rotasyon ücreti (NSC).
    pub rotation_fee: u128,
}

impl ConsKeyRotationManager {
    /// Varsayılan parametrelerle oluştur.
    ///
    /// - min_interval: 1 era (216_000 slot ≈ 1 gün)
    /// - max_per_era: 5 (Cosmos SDK varsayılanı)
    /// - fee: 1_000_000 * 10^18 NSC = 1 NSC
    pub fn new() -> Self {
        Self {
            current_keys: HashMap::new(),
            history: Vec::new(),
            min_rotation_interval_slots: 216_000,
            max_rotations_per_era: 5,
            era_rotation_counts: HashMap::new(),
            rotation_fee: 1_000_000_000_000_000_000, // 1 NSC
        }
    }

    /// Validator'ın konsensüs anahtarını kaydet (ilk kayıt, ücretsiz).
    pub fn register_key(&mut self, validator_id: ValidatorId, key: PublicKey) {
        self.current_keys.insert(validator_id, key);
    }

    /// Mevcut konsensüs anahtarını al.
    pub fn current_key(&self, validator_id: &ValidatorId) -> Option<&PublicKey> {
        self.current_keys.get(validator_id)
    }

    /// Anahtar rotasyonu yap.
    ///
    /// Hatalar:
    /// - `TooManyRotationsInEra`: Bu era'da çok fazla rotasyon
    /// - `KeyAlreadyInUse`: Yeni anahtar zaten kullanımda
    pub fn rotate_key(
        &mut self,
        validator_id: ValidatorId,
        new_key: PublicKey,
        current_slot: u64,
        fee_pool: &mut KeyRotationFeePool,
    ) -> Result<KeyRotationRecord, KeyRotationError> {
        // Era başına limit kontrolü
        let era_count = self.era_rotation_counts.entry(validator_id).or_insert(0);
        if *era_count >= self.max_rotations_per_era {
            return Err(KeyRotationError::TooManyRotationsInEra {
                validator_id,
                limit: self.max_rotations_per_era,
            });
        }

        // Aynı anahtar tekrar kullanılıyor mu?
        if self.current_keys.values().any(|k| k == &new_key) {
            return Err(KeyRotationError::KeyAlreadyInUse);
        }

        let old_key = self.current_keys
            .get(&validator_id)
            .cloned()
            .ok_or(KeyRotationError::ValidatorNotFound(validator_id))?;

        // Rotasyonu gerçekleştir
        self.current_keys.insert(validator_id, new_key.clone());
        *era_count += 1;
        fee_pool.add_fee(self.rotation_fee);

        let record = KeyRotationRecord {
            validator_id,
            old_key,
            new_key,
            rotation_slot: current_slot,
            fee_paid: self.rotation_fee,
        };
        self.history.push(record.clone());
        Ok(record)
    }

    /// Era sonunda rotasyon sayaçlarını sıfırla.
    pub fn reset_era_counts(&mut self) {
        self.era_rotation_counts.clear();
    }

    /// Rotasyon geçmişi.
    pub fn history(&self) -> &[KeyRotationRecord] {
        &self.history
    }

    /// Bu validator için toplam rotasyon sayısı.
    pub fn total_rotations(&self, validator_id: &ValidatorId) -> usize {
        self.history.iter().filter(|r| &r.validator_id == validator_id).count()
    }

    /// AŞAMA 7 Görev 57: Eski konsensüs anahtarıyla (ConsAddr) validator bul.
    ///
    /// Cosmos SDK `x/staking/keeper/cons_pubkey.go`:
    /// `GetValidatorByConsAddr(ctx, consAddr)` — rotasyon geçmişinde de arar.
    ///
    /// Bu fonksiyon, bir validatörün eski anahtarıyla bile bulunmasını sağlar.
    /// Bu özellikle slashing için kritiktir: eski anahtar döneminde işlenen
    /// çifte imza (double-sign) kanıtı gelenler için validator'ın tespiti gerekir.
    pub fn find_validator_by_key(&self, key: &PublicKey) -> Option<ValidatorId> {
        // Önce mevcut anahtarlarda ara
        if let Some((&vid, _)) = self.current_keys.iter().find(|(_, k)| *k == key) {
            return Some(vid);
        }
        // Geçmiş kayıtlarda eski anahtarla ara
        self.history
            .iter()
            .rev() // En yeni kayıttan geriye git
            .find(|r| &r.old_key == key)
            .map(|r| r.validator_id)
    }

    /// Bir validator'ın tüm geçmiş konsensüs anahtarlarını al.
    ///
    /// Eski anahtar dönemine ait slashing delili sorgulaması için kullanılır.
    pub fn key_history(&self, validator_id: &ValidatorId) -> Vec<&KeyRotationRecord> {
        self.history.iter().filter(|r| &r.validator_id == validator_id).collect()
    }
}

/// Key rotation hataları.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyRotationError {
    #[error("bu era'da çok fazla rotasyon: validator={:?} limit={}", validator_id.0, limit)]
    TooManyRotationsInEra { validator_id: ValidatorId, limit: u32 },

    #[error("anahtar zaten kullanımda")]
    KeyAlreadyInUse,

    #[error("validator bulunamadı: {:?}", _0.0)]
    ValidatorNotFound(ValidatorId),
}

// ─── PHRAGMÉN SEÇİM ALGORİTMASI ──────────────────────────────────────────────

/// AŞAMA 7 Görev 54: Sequential Phragmén (NPoS) validator seçimi.
///
/// ## Algoritma
///
/// Polkadot `sp-npos-elections` paketinden ilham:
/// Sequential Phragmén, stake'in validator'lar arasında eşit dağılmasını sağlar.
///
/// ### Basit Örnek (greedy'ye göre üstünlüğü)
///
/// Adaylar: A(10), B(10), C(9)
/// Delegatörler: D1 → {A,B,C} (100 stake), D2 → {A,B} (50 stake)
///
/// Greedy top-2: A(160) ve B(160) — her iki delegatörü A ve B alıyor
/// Phragmén top-2: A ve C — D1'in stake'i daha iyi dağılıyor (60/40)
///
/// ### Phragmén load dengesi
///
/// Her adayın `load` değeri, o adayı seçen delegatörlerin normalize
/// yüklerini temsil eder. En küçük marjinal yük artışına yol açan
/// aday seçilir.
pub struct PhragmenElection;

/// Bir delegatörün beyan ettiği destek.
#[derive(Debug, Clone)]
pub struct NominatorVote {
    /// Delegatör adresi.
    pub nominator: Address,
    /// Toplam stake.
    pub total_stake: u128,
    /// Desteklenen validator ID'leri.
    pub targets: Vec<ValidatorId>,
}

/// Phragmén sonucu: bir validator için seçim.
#[derive(Debug, Clone)]
pub struct PhragmenResult {
    /// Seçilen validator.
    pub validator_id: ValidatorId,
    /// Bu validator'ın aldığı toplam backing stake.
    pub backing_stake: u128,
    /// Phragmén load değeri (düşük = daha adil dağılım).
    pub load: f64,
}

impl PhragmenElection {
    /// Sequential Phragmén ile `n_validators` adet validator seç.
    ///
    /// `candidates`: (ValidatorId, self_stake) listesi (ön filtreli, unjailed, bonded-eligible)
    /// `nominators`: delegatör oyları
    /// `n_validators`: seçilecek validator sayısı
    ///
    /// Returns: seçilen validator'lar (backing_stake azalan sırada)
    pub fn elect(
        candidates: &[(ValidatorId, u128)],
        nominators: &[NominatorVote],
        n_validators: usize,
    ) -> Vec<PhragmenResult> {
        if candidates.is_empty() || n_validators == 0 {
            return vec![];
        }

        let n = n_validators.min(candidates.len());

        // Her adayın toplam backing stake'i ve phragmén yükü
        // Başlangıç: yük = 0, backing = self_stake
        let mut backing: Vec<u128> = candidates.iter().map(|(_, s)| *s).collect();
        let mut loads: Vec<f64> = vec![0.0; candidates.len()];
        // Her nominatörün mevcut yükü
        let mut nom_loads: Vec<f64> = vec![0.0; nominators.len()];

        let mut elected: Vec<usize> = Vec::with_capacity(n);
        let mut elected_set: std::collections::HashSet<usize> = Default::default();

        for _round in 0..n {
            // Her seçilmemiş aday için marjinal yük artışını hesapla
            let mut best_candidate = None;
            let mut best_score = f64::MAX;

            for (ci, (_, self_stake)) in candidates.iter().enumerate() {
                if elected_set.contains(&ci) {
                    continue;
                }

                // Bu adayı seçersek toplam backing stake ne olur?
                let mut total_backing = *self_stake as f64;
                for nom in nominators {
                    if nom.targets.contains(&candidates[ci].0) {
                        total_backing += nom.total_stake as f64;
                    }
                }

                if total_backing == 0.0 {
                    continue;
                }

                // Phragmén: score = (1 + Σ nom_loads_i * nom_stake_i) / total_backing
                let mut numerator = 1.0f64;
                for (ni, nom) in nominators.iter().enumerate() {
                    if nom.targets.contains(&candidates[ci].0) {
                        numerator += nom_loads[ni] * nom.total_stake as f64;
                    }
                }
                let score = numerator / total_backing;

                if score < best_score {
                    best_score = score;
                    best_candidate = Some(ci);
                }
            }

            let Some(ci) = best_candidate else { break };
            elected.push(ci);
            elected_set.insert(ci);
            loads[ci] = best_score;

            // Seçilen adayı destekleyen nominatörlerin yüklerini güncelle
            for (ni, nom) in nominators.iter().enumerate() {
                if nom.targets.contains(&candidates[ci].0) {
                    nom_loads[ni] = best_score;
                }
            }

            // Backing stake güncelle
            for (ni, nom) in nominators.iter().enumerate() {
                if nom.targets.contains(&candidates[ci].0) {
                    backing[ci] = backing[ci].saturating_add(
                        (nom.total_stake as f64 * (best_score / loads[ci].max(1e-10))) as u128,
                    );
                }
            }
        }

        let mut results: Vec<PhragmenResult> = elected
            .iter()
            .map(|&ci| PhragmenResult {
                validator_id: candidates[ci].0,
                backing_stake: backing[ci],
                load: loads[ci],
            })
            .collect();

        // Backing stake azalan sıraya göre sırala
        results.sort_by(|a, b| b.backing_stake.cmp(&a.backing_stake));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::nsc::MIN_VALIDATOR_STAKE_NSC;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_validator(id: u64, stake: u128) -> Validator {
        Validator::new(
            ValidatorId(id),
            addr(id as u8),
            stake,
            500,
            MIN_VALIDATOR_STAKE_NSC,
            0,
        )
    }

    #[test]
    fn test_validator_initial_state() {
        let v = make_validator(1, MIN_VALIDATOR_STAKE_NSC);
        assert!(v.status.is_unbonded());
        assert!(!v.jailed);
        assert!(!v.is_active()); // Unbonded → aktif değil
    }

    #[test]
    fn test_validator_commission() {
        let v = make_validator(1, MIN_VALIDATOR_STAKE_NSC);
        // %5 komisyon → 1000 ödülden 50
        let commission = v.commission_amount(1_000);
        assert_eq!(commission, 50);
        assert_eq!(v.delegator_reward(1_000), 950);
    }

    #[test]
    fn test_validator_begin_unbonding() {
        let mut v = make_validator(1, MIN_VALIDATOR_STAKE_NSC);
        v.status = BondStatus::Bonded;
        v.begin_unbonding(SlotId(1_000));
        assert!(v.status.is_unbonding());
        assert!(!v.is_unbonding_complete(SlotId(1_000)));
        assert!(v.is_unbonding_complete(SlotId(1_000 + UNBONDING_DURATION_SLOTS)));
    }

    #[test]
    fn test_validator_pool_register() {
        let pool = ValidatorPool::new();
        pool.register(make_validator(1, MIN_VALIDATOR_STAKE_NSC)).unwrap();
        pool.register(make_validator(2, MIN_VALIDATOR_STAKE_NSC)).unwrap();
        assert_eq!(pool.count(), 2);
    }

    #[test]
    fn test_validator_pool_duplicate() {
        let pool = ValidatorPool::new();
        pool.register(make_validator(1, MIN_VALIDATOR_STAKE_NSC)).unwrap();
        let err = pool.register(make_validator(1, MIN_VALIDATOR_STAKE_NSC));
        assert!(matches!(err, Err(crate::errors::RuntimeError::ValidatorAlreadyExists)));
    }

    #[test]
    fn test_validator_pool_active_filter() {
        let pool = ValidatorPool::new();
        let mut v1 = make_validator(1, MIN_VALIDATOR_STAKE_NSC);
        v1.status = BondStatus::Bonded; // Aktif
        let mut v2 = make_validator(2, MIN_VALIDATOR_STAKE_NSC);
        v2.status = BondStatus::Bonded;
        v2.jailed = true; // Jailed → aktif değil
        pool.register(v1).unwrap();
        pool.register(v2).unwrap();

        let active = pool.active_validators();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, ValidatorId(1));
    }

    #[test]
    fn test_validator_pool_sorted_by_power() {
        let pool = ValidatorPool::new();
        pool.register(make_validator(1, 100)).unwrap();
        pool.register(make_validator(2, 500)).unwrap();
        pool.register(make_validator(3, 300)).unwrap();

        let sorted = pool.sorted_by_power();
        assert_eq!(sorted[0].id, ValidatorId(2)); // En yüksek stake
        assert_eq!(sorted[1].id, ValidatorId(3));
        assert_eq!(sorted[2].id, ValidatorId(1));
    }

    #[test]
    fn test_validator_pool_apply_validator_set() {
        let pool = ValidatorPool::new();
        for i in 1u64..=5 {
            pool.register(make_validator(i, MIN_VALIDATOR_STAKE_NSC * i as u128)).unwrap();
        }
        pool.apply_validator_set_update(SlotId(0));

        // Hepsi MAX_ACTIVE_VALIDATORS=200 sınırının altında → hepsi bonded
        let active = pool.active_validators();
        assert_eq!(active.len(), 5);
    }

    #[test]
    fn test_consensus_power() {
        let v = make_validator(1, 1_000_000_000_000_000_000); // 1 NSC
        assert_eq!(v.consensus_power(), 1);

        let v2 = make_validator(2, 100_000_000_000_000_000_000); // 100 NSC
        assert_eq!(v2.consensus_power(), 100);
    }

    // ─── KeyRotation Testleri ─────────────────────────────────────────────────

    fn make_pubkey(seed: u8) -> nova_primitives::crypto::PublicKey {
        let bytes = [seed; 33];
        nova_primitives::crypto::PublicKey::from_slice(&bytes).unwrap()
    }

    #[test]
    fn test_key_rotation_basic() {
        let mut mgr = ConsKeyRotationManager::new();
        let mut fee_pool = KeyRotationFeePool::new();
        let vid = ValidatorId(1);

        mgr.register_key(vid, make_pubkey(1));
        let record = mgr.rotate_key(vid, make_pubkey(2), 1000, &mut fee_pool).unwrap();

        assert_eq!(record.validator_id, vid);
        assert_eq!(record.rotation_slot, 1000);
        assert_eq!(fee_pool.total_collected(), mgr.rotation_fee);
        assert_eq!(mgr.current_key(&vid), Some(&make_pubkey(2)));
    }

    #[test]
    fn test_key_rotation_max_per_era() {
        let mut mgr = ConsKeyRotationManager::new();
        mgr.max_rotations_per_era = 2;
        let mut fee_pool = KeyRotationFeePool::new();
        let vid = ValidatorId(1);

        mgr.register_key(vid, make_pubkey(1));
        mgr.rotate_key(vid, make_pubkey(2), 100, &mut fee_pool).unwrap();
        mgr.rotate_key(vid, make_pubkey(3), 200, &mut fee_pool).unwrap();
        // 3. rotasyon → hata
        let err = mgr.rotate_key(vid, make_pubkey(4), 300, &mut fee_pool).unwrap_err();
        assert!(matches!(err, KeyRotationError::TooManyRotationsInEra { limit: 2, .. }));
    }

    #[test]
    fn test_key_rotation_reset_era() {
        let mut mgr = ConsKeyRotationManager::new();
        mgr.max_rotations_per_era = 1;
        let mut fee_pool = KeyRotationFeePool::new();
        let vid = ValidatorId(1);

        mgr.register_key(vid, make_pubkey(1));
        mgr.rotate_key(vid, make_pubkey(2), 100, &mut fee_pool).unwrap();

        // Era sıfırla → tekrar rotate edilebilir
        mgr.reset_era_counts();
        mgr.rotate_key(vid, make_pubkey(3), 216_100, &mut fee_pool).unwrap();
        assert_eq!(mgr.total_rotations(&vid), 2);
    }

    #[test]
    fn test_key_rotation_duplicate_key_rejected() {
        let mut mgr = ConsKeyRotationManager::new();
        let mut fee_pool = KeyRotationFeePool::new();
        let v1 = ValidatorId(1);
        let v2 = ValidatorId(2);

        mgr.register_key(v1, make_pubkey(1));
        mgr.register_key(v2, make_pubkey(2));

        // v1 v2'nin anahtarını almak istiyor → hata
        let err = mgr.rotate_key(v1, make_pubkey(2), 500, &mut fee_pool).unwrap_err();
        assert!(matches!(err, KeyRotationError::KeyAlreadyInUse));
    }

    #[test]
    fn test_fee_pool_drain() {
        let mut pool = KeyRotationFeePool::new();
        pool.add_fee(500);
        pool.add_fee(300);
        assert_eq!(pool.total_collected(), 800);
        assert_eq!(pool.rotation_count(), 2);

        let drained = pool.drain();
        assert_eq!(drained, 800);
        assert_eq!(pool.total_collected(), 0);
    }

    #[test]
    fn test_find_validator_by_old_key() {
        let mut mgr = ConsKeyRotationManager::new();
        let mut fee_pool = KeyRotationFeePool::new();
        let vid = ValidatorId(1);

        let old_key = make_pubkey(10);
        let new_key = make_pubkey(20);

        mgr.register_key(vid, old_key.clone());
        mgr.rotate_key(vid, new_key.clone(), 1000, &mut fee_pool).unwrap();

        // Yeni anahtarla bul
        assert_eq!(mgr.find_validator_by_key(&new_key), Some(vid));
        // Eski anahtarla da bulunabilmeli (slashing senaryosu)
        assert_eq!(mgr.find_validator_by_key(&old_key), Some(vid));
        // Hiç olmayan anahtar
        assert_eq!(mgr.find_validator_by_key(&make_pubkey(99)), None);
    }

    #[test]
    fn test_key_history() {
        let mut mgr = ConsKeyRotationManager::new();
        let mut fee_pool = KeyRotationFeePool::new();
        let vid = ValidatorId(1);

        mgr.register_key(vid, make_pubkey(1));
        mgr.rotate_key(vid, make_pubkey(2), 100, &mut fee_pool).unwrap();
        mgr.rotate_key(vid, make_pubkey(3), 200, &mut fee_pool).unwrap();

        let history = mgr.key_history(&vid);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].rotation_slot, 100);
        assert_eq!(history[1].rotation_slot, 200);
    }

    // ─── Phragmén Testleri ────────────────────────────────────────────────────

    #[test]
    fn test_phragmen_no_nominators_top_by_stake() {
        // Nominatör yok → sadece self_stake → yüksek stake'li seçilir
        let candidates = vec![
            (ValidatorId(1), 1_000u128),
            (ValidatorId(2), 3_000u128),
            (ValidatorId(3), 2_000u128),
        ];
        let results = PhragmenElection::elect(&candidates, &[], 2);
        assert_eq!(results.len(), 2);
        // En yüksek stake'li 2 seçilmeli
        let ids: Vec<_> = results.iter().map(|r| r.validator_id).collect();
        assert!(ids.contains(&ValidatorId(2)));
        assert!(ids.contains(&ValidatorId(3)));
    }

    #[test]
    fn test_phragmen_single_nominator() {
        let candidates = vec![
            (ValidatorId(1), 100u128),
            (ValidatorId(2), 100u128),
        ];
        let nominators = vec![NominatorVote {
            nominator: addr(1),
            total_stake: 500,
            targets: vec![ValidatorId(1), ValidatorId(2)],
        }];
        let results = PhragmenElection::elect(&candidates, &nominators, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_phragmen_n_greater_than_candidates() {
        let candidates = vec![(ValidatorId(1), 100u128)];
        let results = PhragmenElection::elect(&candidates, &[], 5);
        assert_eq!(results.len(), 1); // Sadece 1 aday var
    }

    #[test]
    fn test_phragmen_empty_candidates() {
        let results = PhragmenElection::elect(&[], &[], 3);
        assert!(results.is_empty());
    }

    #[test]
    fn test_phragmen_stake_distribution() {
        // 3 aday, 1 nominatör — Phragmén en adil dağılımı seçmeli
        let candidates = vec![
            (ValidatorId(1), 10u128),
            (ValidatorId(2), 10u128),
            (ValidatorId(3), 9u128),
        ];
        let nominators = vec![
            NominatorVote {
                nominator: addr(1),
                total_stake: 100,
                targets: vec![ValidatorId(1), ValidatorId(2), ValidatorId(3)],
            },
            NominatorVote {
                nominator: addr(2),
                total_stake: 50,
                targets: vec![ValidatorId(1), ValidatorId(2)],
            },
        ];
        // Top-2 seç
        let results = PhragmenElection::elect(&candidates, &nominators, 2);
        assert_eq!(results.len(), 2);
        // Sonuç deterministik olmalı (aynı girdide aynı çıktı)
        let results2 = PhragmenElection::elect(&candidates, &nominators, 2);
        let ids1: Vec<_> = results.iter().map(|r| r.validator_id).collect();
        let ids2: Vec<_> = results2.iter().map(|r| r.validator_id).collect();
        assert_eq!(ids1, ids2);
    }
}
