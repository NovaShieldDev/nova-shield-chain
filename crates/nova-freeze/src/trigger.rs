//! Freeze tetikleyicileri — §5.5.2 üç tetikleyici
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing infractions.go: `HandleValidatorSignature()` — koşul değerlendirme
//!   kalıbı: önce erken çıkış (jailed check), sonra bilgi güncelleme, sonra ceza.
//! - Polkadot im-online: `authorize_heartbeat_call()` — cheap checks first, expensive last.
//! - Rolling Shutter keyper voting: guardian quorum oylama mantığı.
//!
//! Her tetikleyici şu akışı izler:
//! 1. Ön koşul kontrolleri (erken hata)
//! 2. Guardian yetki doğrulama
//! 3. FreezeRecord oluşturma
//! 4. FreezeId türetme

use crate::errors::{FreezeError, FreezeResult};
use crate::guardian::FreezeAuthorizedGuardians;
use crate::types::{AnomalyType, FreezeId, FreezeReason, FreezeRecord};
use nova_primitives::{Address, SlotId};

/// Üç tetikleyiciyi yöneten değerlendirici.
///
/// Pattern: Cosmos SDK'nın Keeper pattern'i — tüm state bilgisi dışarıdan
/// geçirilir, TriggerEvaluator kendi state'i tutmaz (stateless evaluator).
pub struct TriggerEvaluator;

impl TriggerEvaluator {
    // ─── Tetikleyici 1: Kullanıcı dondurma ───────────────────────────────────

    /// Kullanıcı kendi hesabını dondurur.
    ///
    /// §5.5.2 Tetikleyici 1: "Kullanıcı tek MPC imzasıyla hesabını anında
    /// dondurabilir — threshold gerekmez."
    ///
    /// Pattern: Cosmos SDK unjail.go'nun tersi — kullanıcı `MsgUnjail` yerine
    /// `MsgFreeze` gönderir; tek imza yeterli.
    pub fn evaluate_user_freeze(
        account: Address,
        initiator: Address,
        current_slot: SlotId,
        timestamp_secs: u64,
    ) -> FreezeResult<FreezeRecord> {
        // Kullanıcı dondurmasında initiator = account sahibi veya yetkili device
        let freeze_id = Self::generate_freeze_id(&account, current_slot, 0x01);
        Ok(FreezeRecord {
            freeze_id,
            account,
            reason: FreezeReason::UserInitiated,
            initiator,
            initiated_at_slot: current_slot,
            initiated_at_timestamp: timestamp_secs,
        })
    }

    // ─── Tetikleyici 2: Guardian threshold dondurma ───────────────────────────

    /// Guardian'lar threshold sayıda imzayla hesabı dondurur.
    ///
    /// §5.5.2 Tetikleyici 2: "belirlenen threshold sayıda guardian birleşerek
    /// dondurabilir."
    ///
    /// Pattern: Rolling Shutter keyper voting — `keypers.len() >= threshold`
    /// koşulu karşılanmadan işlem geçersiz.
    ///
    /// Kontroller (Cosmos SDK infractions.go cheap→expensive sırası):
    /// 1. Guardian listesi tanımlı mı?
    /// 2. Her signer yetkili mi?
    /// 3. Threshold karşılandı mı?
    /// 4. Jailed signer var mı?
    pub fn evaluate_guardian_threshold(
        account: Address,
        signers: Vec<Address>,
        authorized: &FreezeAuthorizedGuardians,
        jailed_check: impl Fn(&Address) -> bool,
        current_slot: SlotId,
        timestamp_secs: u64,
    ) -> FreezeResult<FreezeRecord> {
        // 1. Yetkili listesi boş mu?
        if authorized.authorized.is_empty() {
            return Err(FreezeError::NoAuthorizedGuardians);
        }

        // 2. NOVA-RED-FREEZE-006: Tekrarlı imzacı kontrolü — dedup önce, threshold sonra
        {
            let mut sorted = signers.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != signers.len() {
                return Err(FreezeError::DuplicateGuardianSigner);
            }
        }

        // 3. Her imzalayanın yetkili olup olmadığını kontrol et
        for signer in &signers {
            if !authorized.is_authorized(signer) {
                return Err(FreezeError::GuardianNotAuthorized);
            }
        }

        // 4. Threshold karşılandı mı?
        let signer_count = signers.len() as u8;
        if signer_count < authorized.required_threshold {
            return Err(FreezeError::ThresholdNotMet {
                required: authorized.required_threshold,
                got: signer_count,
            });
        }

        // 5. Jailed signer var mı? (expensive check — son)
        for signer in &signers {
            if jailed_check(signer) {
                return Err(FreezeError::GuardianJailed {
                    jailed_until_slot: current_slot.0,
                });
            }
        }

        let freeze_id = Self::generate_freeze_id(&account, current_slot, 0x02);
        Ok(FreezeRecord {
            freeze_id,
            account,
            reason: FreezeReason::GuardianThreshold {
                signers: signers.clone(),
            },
            // NOVA-RED-FREEZE-004: initiator = ilk gerçek imzacı (authorized[0] değil)
            initiator: signers[0],
            initiated_at_slot: current_slot,
            initiated_at_timestamp: timestamp_secs,
        })
    }

    // ─── Tetikleyici 3: Anomali tespiti ──────────────────────────────────────

    /// Anomali tespiti sonucu dondurma.
    ///
    /// §5.5.2 Tetikleyici 3: "Protokol, hesabın geçmiş davranış örüntüsünden
    /// sapma tespit ederse uyarı başlatır."
    ///
    /// Pattern: Polkadot im-online `on_before_session_ending()` — session sonunda
    /// offline validator'ları tara → offence raporla. Burada: slot işleminde
    /// anomali tespiti → onay süresi → dondur.
    ///
    /// Gerekli: `score_bps >= min_threshold_bps`
    pub fn evaluate_anomaly_trigger(
        account: Address,
        score_bps: u32,
        anomaly_type: AnomalyType,
        current_slot: SlotId,
        timestamp_secs: u64,
        min_threshold_bps: u32,
    ) -> FreezeResult<FreezeRecord> {
        if score_bps < min_threshold_bps {
            return Err(FreezeError::AnomalyScoreTooLow {
                score_bps,
                threshold_bps: min_threshold_bps,
            });
        }

        let freeze_id = Self::generate_freeze_id(&account, current_slot, 0x03);
        Ok(FreezeRecord {
            freeze_id,
            account,
            reason: FreezeReason::AnomalyDetected {
                anomaly_score_bps: score_bps,
                anomaly_type,
            },
            initiator: Address::ZERO, // Protokol tarafından başlatıldı
            initiated_at_slot: current_slot,
            initiated_at_timestamp: timestamp_secs,
        })
    }

    // ─── Yardımcı: FreezeId türetme ───────────────────────────────────────────

    /// FreezeId = blake3(account || slot_le_bytes || reason_tag).
    ///
    /// Pattern: nova-inheritance `InheritancePlanId::derive(account_id, slot)` —
    /// deterministik ID türetme, çakışma riski sıfır.
    ///
    /// `reason_tag`: 0x01=user, 0x02=guardian, 0x03=anomaly
    pub fn generate_freeze_id(account: &Address, slot: SlotId, reason_tag: u8) -> FreezeId {
        let mut hasher = blake3::Hasher::new();
        hasher.update(account.as_bytes());
        hasher.update(&slot.0.to_le_bytes());
        hasher.update(&[reason_tag]);
        *hasher.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardian::FreezeAuthorizedGuardians;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_authorized(account: Address, guardians: Vec<Address>, threshold: u8) -> FreezeAuthorizedGuardians {
        FreezeAuthorizedGuardians {
            account,
            authorized: guardians,
            required_threshold: threshold,
        }
    }

    // ─── Tetikleyici 1: Kullanıcı dondurma ───────────────────────────────────

    #[test]
    fn test_user_freeze_creates_valid_record() {
        let account = addr(1);
        let initiator = addr(1);
        let slot = SlotId(1000);

        let record = TriggerEvaluator::evaluate_user_freeze(account, initiator, slot, 100_000)
            .expect("user freeze başarılı olmalı");

        assert_eq!(record.account, account);
        assert_eq!(record.initiator, initiator);
        assert!(record.is_user_initiated());
        assert_eq!(record.initiated_at_slot, slot);
        assert_eq!(record.initiated_at_timestamp, 100_000);
        // freeze_id deterministic olmalı
        let expected_id = TriggerEvaluator::generate_freeze_id(&account, slot, 0x01);
        assert_eq!(record.freeze_id, expected_id);
    }

    #[test]
    fn test_user_freeze_different_slots_different_ids() {
        let account = addr(1);
        let id1 = TriggerEvaluator::generate_freeze_id(&account, SlotId(100), 0x01);
        let id2 = TriggerEvaluator::generate_freeze_id(&account, SlotId(101), 0x01);
        assert_ne!(id1, id2, "farklı slot → farklı freeze_id");
    }

    #[test]
    fn test_user_freeze_reason_discriminant_different() {
        let account = addr(1);
        let slot = SlotId(100);
        let id_user = TriggerEvaluator::generate_freeze_id(&account, slot, 0x01);
        let id_guardian = TriggerEvaluator::generate_freeze_id(&account, slot, 0x02);
        let id_anomaly = TriggerEvaluator::generate_freeze_id(&account, slot, 0x03);
        assert_ne!(id_user, id_guardian);
        assert_ne!(id_user, id_anomaly);
        assert_ne!(id_guardian, id_anomaly);
    }

    // ─── Tetikleyici 2: Guardian threshold ───────────────────────────────────

    #[test]
    fn test_guardian_threshold_freeze_success() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let g3 = addr(12);

        let authorized = make_authorized(account, vec![g1, g2, g3], 2);
        let signers = vec![g1, g2];
        let no_jail = |_: &Address| false;

        let record = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers.clone(), &authorized, no_jail, SlotId(500), 50_000,
        ).expect("2-of-3 threshold karşılandı");

        assert!(record.is_guardian_freeze());
        assert_eq!(record.guardian_signers().unwrap().len(), 2);
    }

    #[test]
    fn test_guardian_threshold_not_met() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);

        let authorized = make_authorized(account, vec![g1, g2], 2);
        let signers = vec![g1]; // sadece 1, gerekli 2
        let no_jail = |_: &Address| false;

        let err = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).unwrap_err();

        assert!(matches!(err, FreezeError::ThresholdNotMet { required: 2, got: 1 }));
    }

    #[test]
    fn test_guardian_not_authorized() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let unauthorized = addr(99);

        let authorized = make_authorized(account, vec![g1, g2], 2);
        let signers = vec![g1, unauthorized]; // unauthorized yetkisiz
        let no_jail = |_: &Address| false;

        let err = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).unwrap_err();

        assert!(matches!(err, FreezeError::GuardianNotAuthorized));
    }

    #[test]
    fn test_guardian_jailed_rejected() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);

        let authorized = make_authorized(account, vec![g1, g2], 2);
        let signers = vec![g1, g2];
        let g1_clone = g1;
        let jailed_check = move |a: &Address| *a == g1_clone; // g1 jailed

        let err = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, jailed_check, SlotId(500), 50_000,
        ).unwrap_err();

        assert!(matches!(err, FreezeError::GuardianJailed { .. }));
    }

    /// NOVA-RED-FREEZE-004: initiator = ilk gerçek imzacı, authorized[0] değil
    #[test]
    fn test_guardian_initiator_is_first_signer_not_authorized_zero() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let g3 = addr(12); // authorized[0] ama signer değil

        let authorized = make_authorized(account, vec![g3, g1, g2], 2);
        let signers = vec![g1, g2]; // g3 authorized[0] ama imzalamadı
        let no_jail = |_: &Address| false;

        let record = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).expect("başarılı olmalı");

        // initiator authorized[0]=g3 değil, signers[0]=g1 olmalı
        assert_eq!(record.initiator, g1, "initiator ilk gerçek imzacı olmalı");
    }

    /// NOVA-RED-FREEZE-006: tekrarlı imzacı threshold bypass'ı engellenmeli
    #[test]
    fn test_duplicate_signer_rejected() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);

        let authorized = make_authorized(account, vec![g1, g2], 2);
        // g1 iki kez gönderildi — threshold bypass denemesi
        let signers = vec![g1, g1];
        let no_jail = |_: &Address| false;

        let err = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).unwrap_err();

        assert!(matches!(err, FreezeError::DuplicateGuardianSigner),
            "tekrarlı imzacı reddedilmeli, aldık: {:?}", err);
    }

    /// NOVA-RED-FREEZE-006: 3 farklı imzacı → geçerli
    #[test]
    fn test_no_duplicate_signers_passes() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let g3 = addr(12);

        let authorized = make_authorized(account, vec![g1, g2, g3], 2);
        let signers = vec![g1, g2]; // farklı, threshold = 2 → geçerli
        let no_jail = |_: &Address| false;

        assert!(TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).is_ok());
    }

    #[test]
    fn test_guardian_no_authorized_list() {
        let account = addr(1);
        let authorized = make_authorized(account, vec![], 2); // boş liste
        let signers = vec![addr(10), addr(11)];
        let no_jail = |_: &Address| false;

        let err = TriggerEvaluator::evaluate_guardian_threshold(
            account, signers, &authorized, no_jail, SlotId(500), 50_000,
        ).unwrap_err();

        assert!(matches!(err, FreezeError::NoAuthorizedGuardians));
    }

    // ─── Tetikleyici 3: Anomali ───────────────────────────────────────────────

    #[test]
    fn test_anomaly_trigger_above_threshold() {
        let account = addr(1);
        let record = TriggerEvaluator::evaluate_anomaly_trigger(
            account,
            8_500, // 85% — eşik üzerinde
            AnomalyType::RapidMultipleWithdrawals,
            SlotId(1000),
            100_000,
            7_000, // %70 eşik
        ).expect("yüksek anomali skoru freeze tetiklemeli");

        assert!(record.is_anomaly_freeze());
        if let FreezeReason::AnomalyDetected { anomaly_score_bps, .. } = &record.reason {
            assert_eq!(*anomaly_score_bps, 8_500);
        }
    }

    #[test]
    fn test_anomaly_trigger_below_threshold_rejected() {
        let account = addr(1);
        let err = TriggerEvaluator::evaluate_anomaly_trigger(
            account,
            5_000, // 50% — eşiğin altında
            AnomalyType::UnusualTransferVolume,
            SlotId(1000),
            100_000,
            7_000, // %70 eşik
        ).unwrap_err();

        assert!(matches!(
            err,
            FreezeError::AnomalyScoreTooLow { score_bps: 5_000, threshold_bps: 7_000 }
        ));
    }

    #[test]
    fn test_anomaly_trigger_exact_threshold() {
        let account = addr(1);
        // Tam eşikte → başarılı
        let result = TriggerEvaluator::evaluate_anomaly_trigger(
            account,
            7_000, // tam eşikte
            AnomalyType::GuardianReport,
            SlotId(1000),
            100_000,
            7_000,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_anomaly_initiator_is_zero_address() {
        let record = TriggerEvaluator::evaluate_anomaly_trigger(
            addr(1), 8_000, AnomalyType::GeographicAnomaliy, SlotId(100), 1000, 7_000,
        ).unwrap();
        // Anomali protokol tarafından başlatıldı → zero address
        assert_eq!(record.initiator, Address::ZERO);
    }
}
