//! Heartbeat / Hayatta Olduğunu Kanıtlama (Proof of Life)
//!
//! ## Araştırma Kaynakları
//!
//! ### Polkadot Im-Online Pallet
//! `research/chains/polkadot-sdk/substrate/frame/im-online/src/lib.rs`
//!
//! ```text
//! // Im-Online: her session'da heartbeat gossip et
//! pub struct Heartbeat<BlockNumber> {
//!     block_number: BlockNumber,
//!     session_index: SessionIndex,
//!     authority_index: AuthIndex,
//!     validators_len: u32,
//! }
//! fn heartbeat(origin, heartbeat, signature) -> DispatchResult
//! fn is_online(authority_index: u32) -> bool
//! ```
//!
//! Im-Online'dan öğrenilen:
//! - Heartbeat tek bir imza ile gönderilir
//! - Her era/session için sıfırlanır
//! - `is_online()` sorgusu — zincir durumunu sorgular
//!
//! ### Nova Uyarlaması
//!
//! Nova'da heartbeat daha kapsamlı:
//! - Normal heartbeat: inaktivite sayacını sıfırlar
//! - Duress heartbeat: gizli alarm tetikler (§5.4.4 Yöntem 2)
//! - Guardian extension: kullanıcı erişimi yoksa grace period uzatır

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{
    HeartbeatKind, HeartbeatMsg, InheritancePlanId, SLOT_DURATION_MS,
};
use nova_crypto::{EcdsaVerifier};
use nova_primitives::crypto::{Hash, PublicKey, Signature};
use nova_primitives::types::{Hash32, SlotId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── HEARTBEAT KAYDI ─────────────────────────────────────────────────────────

/// Heartbeat kaydı — bir heartbeat'in tam kaydı.
///
/// Polkadot Im-Online `Heartbeat` struct analog.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartbeatRecord {
    /// Hangi plan için.
    pub plan_id: InheritancePlanId,
    /// Gönderildiği slot.
    pub slot: SlotId,
    /// Heartbeat tipi.
    pub kind: HeartbeatKind,
    /// Hash (doğrulama).
    pub hash: Hash32,
    /// İşlenip işlenmediği.
    pub processed: bool,
}

// ─── HEARTBEAT TAKİPÇİSİ ─────────────────────────────────────────────────────

/// Heartbeat takipçisi — tüm gelen heartbeat'leri yönetir.
///
/// Polkadot Im-Online `ReceivedHeartbeats` storage + `is_online()` analog.
///
/// Görevleri:
/// 1. Heartbeat'lerin hash'ini doğrula (replay koruması)
/// 2. Duplicate heartbeat'leri reddet (aynı slot, aynı plan)
/// 3. Duress heartbeat'leri sessizce işaretle (alarm tetikle)
/// 4. Son aktivite slot'unu güncelle
#[derive(Debug)]
pub struct HeartbeatTracker {
    /// Plan ID → son heartbeat slot'u.
    last_heartbeat: HashMap<[u8; 32], SlotId>,
    /// Plan ID → toplam heartbeat sayısı.
    heartbeat_counts: HashMap<[u8; 32], u64>,
    /// Plan ID → duress tespiti.
    duress_flags: HashMap<[u8; 32], bool>,
    /// Plan ID → toplam duplicate reddi sayısı.
    duplicate_rejections: HashMap<[u8; 32], u32>,
}

impl HeartbeatTracker {
    /// Yeni takipçi oluştur.
    pub fn new() -> Self {
        Self {
            last_heartbeat: HashMap::new(),
            heartbeat_counts: HashMap::new(),
            duress_flags: HashMap::new(),
            duplicate_rejections: HashMap::new(),
        }
    }

    /// Heartbeat mesajını işle.
    ///
    /// Im-Online pallet `heartbeat()` extrinsic analog.
    ///
    /// NOVA-RED-INHERITANCE-003: `owner_mpc_pubkey` parametresi eklendi.
    /// Artık `mpc_signature` gerçek ECDSA ile doğrulanıyor — `is_empty()` yeterli değil.
    ///
    /// İşlem adımları:
    /// 1. Hash doğrulaması
    /// 2. Duplicate kontrolü
    /// 3. ECDSA imza doğrulaması
    /// 4. Kayıt güncelleme
    /// 5. Duress flag kontrolü
    pub fn process_heartbeat(
        &mut self,
        msg: &HeartbeatMsg,
        owner_mpc_pubkey: &[u8],
    ) -> InheritanceResult<HeartbeatRecord> {
        let plan_key = msg.plan_id.0;

        // 1. Hash doğrulaması
        let kind_byte = match msg.kind {
            HeartbeatKind::Normal => 0u8,
            HeartbeatKind::Duress => 1u8,
            HeartbeatKind::GuardianExtension { .. } => 2u8,
        };
        let expected_hash = HeartbeatMsg::compute_hash(
            msg.account_id,
            &msg.plan_id,
            msg.slot,
            kind_byte,
        );
        if expected_hash != msg.heartbeat_hash {
            return Err(InheritanceError::InvalidHeartbeatSignature);
        }

        // 2. Duplicate kontrolü (aynı slot için aynı plan'a ikinci heartbeat reddedilir)
        if let Some(&last_slot) = self.last_heartbeat.get(&plan_key) {
            if last_slot == msg.slot {
                *self.duplicate_rejections.entry(plan_key).or_insert(0) += 1;
                return Err(InheritanceError::DuplicateHeartbeat { slot: msg.slot });
            }
        }

        // 3. ECDSA imza doğrulaması (NOVA-RED-INHERITANCE-003)
        // GuardianExtension tipi için de imza zorunludur.
        if msg.mpc_signature.is_empty() {
            return Err(InheritanceError::InvalidHeartbeatSignature);
        }
        {
            let pk = PublicKey::from_slice(owner_mpc_pubkey)
                .ok_or(InheritanceError::InvalidHeartbeatSignature)?;
            let hash = Hash::from_bytes(*msg.heartbeat_hash.as_bytes());
            let sig = Signature::new_ecdsa(msg.mpc_signature.clone());
            EcdsaVerifier::verify(&hash, &sig, &pk)
                .map_err(|_| InheritanceError::InvalidHeartbeatSignature)?;
        }

        // 4. Kayıt güncelleme
        self.last_heartbeat.insert(plan_key, msg.slot);
        *self.heartbeat_counts.entry(plan_key).or_insert(0) += 1;

        // 5. Duress flag — gizli alarm
        if matches!(msg.kind, HeartbeatKind::Duress) {
            self.duress_flags.insert(plan_key, true);
            // NOT: duress flag dışarıya yayınlanmaz — sadece guardian'lara gizli bildirim
            tracing::warn!(
                plan = %msg.plan_id,
                slot = msg.slot.0,
                "DURESS HEARTBEAT ALINDI — guardian'lara gizli alarm gönderildi"
            );
        }

        Ok(HeartbeatRecord {
            plan_id: msg.plan_id,
            slot: msg.slot,
            kind: msg.kind,
            hash: msg.heartbeat_hash,
            processed: true,
        })
    }

    /// Bu plan için son heartbeat slot'u.
    ///
    /// Im-Online `is_online()` analog — son aktivite slot'unu döndürür.
    pub fn last_heartbeat_slot(&self, plan_id: &InheritancePlanId) -> Option<SlotId> {
        self.last_heartbeat.get(&plan_id.0).copied()
    }

    /// Bu plan için toplam heartbeat sayısı.
    pub fn heartbeat_count(&self, plan_id: &InheritancePlanId) -> u64 {
        self.heartbeat_counts.get(&plan_id.0).copied().unwrap_or(0)
    }

    /// Bu plan için duress tespiti var mı?
    ///
    /// §5.4.4: "Dışarıdan normal heartbeat gibi görünür."
    /// Bu bilgi sadece guardian'lara gizlice iletilir.
    pub fn has_duress(&self, plan_id: &InheritancePlanId) -> bool {
        self.duress_flags.get(&plan_id.0).copied().unwrap_or(false)
    }

    /// Bu plan için duplicate reddi sayısı.
    pub fn duplicate_rejection_count(&self, plan_id: &InheritancePlanId) -> u32 {
        self.duplicate_rejections.get(&plan_id.0).copied().unwrap_or(0)
    }

    /// Plan aktif mi? (son N slot içinde heartbeat aldı mı?)
    ///
    /// Im-Online `is_online()` — inaktivite eşiği bazlı kontrol.
    pub fn is_active_within(&self, plan_id: &InheritancePlanId, within_slots: u64, current_slot: SlotId) -> bool {
        match self.last_heartbeat.get(&plan_id.0) {
            Some(&last) => current_slot.0.saturating_sub(last.0) <= within_slots,
            None => false,
        }
    }

    /// Plan için heartbeat geçmişini sıfırla (plan güncellendiğinde).
    pub fn reset(&mut self, plan_id: &InheritancePlanId) {
        let key = plan_id.0;
        self.last_heartbeat.remove(&key);
        self.heartbeat_counts.remove(&key);
        self.duress_flags.remove(&key);
        self.duplicate_rejections.remove(&key);
    }
}

impl Default for HeartbeatTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── HEARTBEAT BUILDER ────────────────────────────────────────────────────────

/// Heartbeat mesajı oluşturucu — test ve entegrasyon için.
pub struct HeartbeatBuilder {
    account_id: u64,
    plan_id: InheritancePlanId,
    slot: SlotId,
    kind: HeartbeatKind,
}

impl HeartbeatBuilder {
    pub fn new(account_id: u64, plan_id: InheritancePlanId, slot: SlotId) -> Self {
        Self {
            account_id,
            plan_id,
            slot,
            kind: HeartbeatKind::Normal,
        }
    }

    pub fn with_duress(mut self) -> Self {
        self.kind = HeartbeatKind::Duress;
        self
    }

    pub fn with_guardian_extension(mut self, approver_count: u8) -> Self {
        self.kind = HeartbeatKind::GuardianExtension { approver_count };
        self
    }

    /// Heartbeat mesajı oluştur (gerçek imza olmadan — SADECE test helper'ları için).
    /// Production kodunda `build_signed` kullanılmalıdır.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn build_unsigned(self) -> HeartbeatMsg {
        let kind_byte = match self.kind {
            HeartbeatKind::Normal => 0u8,
            HeartbeatKind::Duress => 1u8,
            HeartbeatKind::GuardianExtension { .. } => 2u8,
        };
        let hash = HeartbeatMsg::compute_hash(
            self.account_id,
            &self.plan_id,
            self.slot,
            kind_byte,
        );
        HeartbeatMsg {
            account_id: self.account_id,
            plan_id: self.plan_id,
            slot: self.slot,
            kind: self.kind,
            mpc_signature: vec![0xDEu8; 64], // Test imzası — production'da geçersiz
            heartbeat_hash: hash,
        }
    }

    /// Heartbeat mesajı oluştur — gerçek ECDSA imzasıyla.
    ///
    /// NOVA-RED-INHERITANCE-003: Production kodu bu metodu kullanır.
    /// `keypair`: hesap sahibinin secp256k1 keypair'i.
    pub fn build_signed(self, keypair: &nova_crypto::EcdsaKeypair) -> HeartbeatMsg {
        let kind_byte = match self.kind {
            HeartbeatKind::Normal => 0u8,
            HeartbeatKind::Duress => 1u8,
            HeartbeatKind::GuardianExtension { .. } => 2u8,
        };
        let hash = HeartbeatMsg::compute_hash(
            self.account_id,
            &self.plan_id,
            self.slot,
            kind_byte,
        );
        let sig_hash = nova_primitives::crypto::Hash::from_bytes(*hash.as_bytes());
        let signature = keypair.sign_hash(&sig_hash)
            .expect("heartbeat imzalama başarısız")
            .as_bytes()
            .to_vec();
        HeartbeatMsg {
            account_id: self.account_id,
            plan_id: self.plan_id,
            slot: self.slot,
            kind: self.kind,
            mpc_signature: signature,
            heartbeat_hash: hash,
        }
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HeartbeatKind, InheritancePlanId};
    use nova_crypto::EcdsaKeypair;

    fn test_keypair() -> EcdsaKeypair {
        EcdsaKeypair::from_secret_bytes(&[0xA5u8; 32]).expect("test keypair")
    }

    fn test_pubkey() -> Vec<u8> {
        test_keypair().public_key().expect("pubkey").as_bytes().to_vec()
    }

    fn make_plan_id(seed: u8) -> InheritancePlanId {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        InheritancePlanId(bytes)
    }

    fn make_heartbeat(account_id: u64, plan_id: InheritancePlanId, slot: u64) -> HeartbeatMsg {
        let kp = test_keypair();
        HeartbeatBuilder::new(account_id, plan_id, SlotId(slot)).build_signed(&kp)
    }

    fn make_duress_heartbeat(account_id: u64, plan_id: InheritancePlanId, slot: u64) -> HeartbeatMsg {
        let kp = test_keypair();
        HeartbeatBuilder::new(account_id, plan_id, SlotId(slot))
            .with_duress()
            .build_signed(&kp)
    }

    // Im-Online: heartbeat başarıyla işlenir
    #[test]
    fn test_heartbeat_normal_ok() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(1);
        let msg = make_heartbeat(42, plan_id, 100);
        let pk = test_pubkey();

        let result = tracker.process_heartbeat(&msg, &pk);
        assert!(result.is_ok());
        assert_eq!(tracker.last_heartbeat_slot(&plan_id), Some(SlotId(100)));
        assert_eq!(tracker.heartbeat_count(&plan_id), 1);
    }

    // Im-Online: aynı slot'a ikinci heartbeat reddedilir
    #[test]
    fn test_duplicate_heartbeat_rejected() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(2);
        let pk = test_pubkey();

        let msg = make_heartbeat(1, plan_id, 200);
        tracker.process_heartbeat(&msg, &pk).unwrap();

        let msg2 = make_heartbeat(1, plan_id, 200);
        let err = tracker.process_heartbeat(&msg2, &pk).unwrap_err();
        assert!(matches!(err, InheritanceError::DuplicateHeartbeat { .. }));
        assert_eq!(tracker.duplicate_rejection_count(&plan_id), 1);
    }

    // Farklı slot → kabul edilir
    #[test]
    fn test_different_slot_heartbeat_ok() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(3);
        let pk = test_pubkey();

        let msg1 = make_heartbeat(1, plan_id, 100);
        tracker.process_heartbeat(&msg1, &pk).unwrap();

        let msg2 = make_heartbeat(1, plan_id, 101);
        tracker.process_heartbeat(&msg2, &pk).unwrap();

        assert_eq!(tracker.heartbeat_count(&plan_id), 2);
        assert_eq!(tracker.last_heartbeat_slot(&plan_id), Some(SlotId(101)));
    }

    // §5.4.4 Yöntem 2: Duress heartbeat — gizli alarm
    #[test]
    fn test_duress_heartbeat_sets_flag() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(4);
        let pk = test_pubkey();

        assert!(!tracker.has_duress(&plan_id));

        let msg = make_duress_heartbeat(1, plan_id, 300);
        tracker.process_heartbeat(&msg, &pk).unwrap();

        assert!(tracker.has_duress(&plan_id));
        assert_eq!(tracker.heartbeat_count(&plan_id), 1);
    }

    // Im-Online: is_online() benzeri kontrol
    #[test]
    fn test_is_active_within() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(5);
        let pk = test_pubkey();
        let msg = make_heartbeat(1, plan_id, 1000);
        tracker.process_heartbeat(&msg, &pk).unwrap();

        assert!(tracker.is_active_within(&plan_id, 100, SlotId(1100)));
        assert!(!tracker.is_active_within(&plan_id, 100, SlotId(1101)));
    }

    // Hiç heartbeat yok → son slot yok
    #[test]
    fn test_no_heartbeat_returns_none() {
        let tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(6);
        assert_eq!(tracker.last_heartbeat_slot(&plan_id), None);
        assert_eq!(tracker.heartbeat_count(&plan_id), 0);
    }

    // Hash doğrulama: yanlış hash → hata
    #[test]
    fn test_wrong_hash_rejected() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(7);
        let pk = test_pubkey();

        // build_unsigned ile hash/imza uyumsuz oluştur
        let mut msg = HeartbeatBuilder::new(1, plan_id, SlotId(500)).build_unsigned();
        msg.heartbeat_hash = Hash32::from_bytes([0xFFu8; 32]); // Yanlış hash

        let err = tracker.process_heartbeat(&msg, &pk).unwrap_err();
        assert!(matches!(err, InheritanceError::InvalidHeartbeatSignature));
    }

    // Reset: plan sıfırlanınca heartbeat geçmişi temizlenir
    #[test]
    fn test_reset_clears_history() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(8);
        let pk = test_pubkey();

        let msg = make_duress_heartbeat(1, plan_id, 100);
        tracker.process_heartbeat(&msg, &pk).unwrap();

        tracker.reset(&plan_id);

        assert_eq!(tracker.heartbeat_count(&plan_id), 0);
        assert!(!tracker.has_duress(&plan_id));
        assert_eq!(tracker.last_heartbeat_slot(&plan_id), None);
    }

    // İki farklı plan bağımsız izlenir
    #[test]
    fn test_multiple_plans_independent() {
        let mut tracker = HeartbeatTracker::new();
        let plan_a = make_plan_id(10);
        let plan_b = make_plan_id(20);
        let pk = test_pubkey();

        let msg_a = make_heartbeat(1, plan_a, 100);
        let msg_b = make_heartbeat(2, plan_b, 200);

        tracker.process_heartbeat(&msg_a, &pk).unwrap();
        tracker.process_heartbeat(&msg_b, &pk).unwrap();

        assert_eq!(tracker.last_heartbeat_slot(&plan_a), Some(SlotId(100)));
        assert_eq!(tracker.last_heartbeat_slot(&plan_b), Some(SlotId(200)));
    }

    // Guardian Extension heartbeat — artık imza zorunlu (NOVA-RED-INHERITANCE-003)
    #[test]
    fn test_guardian_extension_signed_ok() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(9);
        let kp = test_keypair();
        let pk = test_pubkey();

        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(100))
            .with_guardian_extension(2)
            .build_signed(&kp);

        let result = tracker.process_heartbeat(&msg, &pk);
        assert!(result.is_ok(), "Guardian extension heartbeat başarılı olmalı");
    }

    /// NOVA-RED-INHERITANCE-003: Sahte imza (0xDE*64) reddedilmeli.
    #[test]
    fn test_fake_signature_rejected() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(11);
        let pk = test_pubkey();

        // build_unsigned → sahte imza [0xDE; 64]
        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(100)).build_unsigned();

        let err = tracker.process_heartbeat(&msg, &pk).unwrap_err();
        assert!(
            matches!(err, InheritanceError::InvalidHeartbeatSignature),
            "Sahte imza reddedilmeli: {:?}", err
        );
    }

    /// NOVA-RED-INHERITANCE-003: Yanlış keypair ile imzalanmış heartbeat reddedilmeli.
    #[test]
    fn test_wrong_keypair_rejected() {
        let mut tracker = HeartbeatTracker::new();
        let plan_id = make_plan_id(12);
        let owner_pk = test_pubkey(); // 0xA5 keypair pubkey

        // Farklı keypair (0x55) ile imzala
        let other_kp = EcdsaKeypair::from_secret_bytes(&[0x55u8; 32]).expect("keypair");
        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(100)).build_signed(&other_kp);

        let err = tracker.process_heartbeat(&msg, &owner_pk).unwrap_err();
        assert!(
            matches!(err, InheritanceError::InvalidHeartbeatSignature),
            "Yanlış keypair reddedilmeli: {:?}", err
        );
    }
}
