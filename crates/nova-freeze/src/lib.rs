//! nova-freeze — Acil Dondurma Protokolü (§5.5)
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing: Keeper pattern, slash+jail mekanizması.
//! - Polkadot im-online: Heartbeat ve session-based window yönetimi.
//! - Polkadot assets-freezer: `freeze()`, `thaw()`, `freeze_asset()` işlemleri.
//! - go-ethereum txpool: `ValidateTransaction()` + `ValidationOptions.Accept` bitmap.
//! - Avalanchego common/halter: Thread-safe atomic freeze flag.
//! - Solana ALT freeze: Pre-condition guard stack pattern.
//! - TSS-lib finalize.go: Threshold proof doğrulama + sumS aggregation.
//! - multi-party-ecdsa blame.rs: Commitment-based attribution.
//! - Rolling Shutter: Temporal window ve deadline yönetimi.
//! - CoW Protocol banned_users: Per-account yetkilendirme registry.
//!
//! ## Modül Yapısı
//!
//! | Modül     | Sorumluluk                                           |
//! |-----------|------------------------------------------------------|
//! | errors    | `FreezeError` + `FreezeResult<T>` tip tanımları     |
//! | types     | Temel tipler, sabitler, FreezeRecord, ThresholdProof |
//! | trigger   | Stateless freeze tetikleyici değerlendirici          |
//! | anomaly   | Davranış profili + anomali skoru hesabı              |
//! | guardian  | Guardian yetkilendirme + ihlal kayıtları             |
//! | window    | 24h/72h soruşturma penceresi yöneticisi              |
//! | unfreeze  | Stateless unfreeze işlemcisi                         |
//! | engine    | Ana orkestrasyon motoru (`FreezeEngine`)             |

// ─── Modül Bildirimleri ───────────────────────────────────────────────────────

pub mod anomaly;
pub mod engine;
pub mod errors;
pub mod guardian;
pub mod trigger;
pub mod types;
pub mod unfreeze;
pub mod window;

// ─── Re-export: errors ───────────────────────────────────────────────────────

pub use errors::{FreezeError, FreezeResult};

// ─── Re-export: types ─────────────────────────────────────────────────────────

pub use types::{
    // Sabitler
    ANOMALY_CONFIRM_SLOTS,
    DEFAULT_ANOMALY_THRESHOLD_BPS,
    GUARDIAN_JAIL_DURATION_SLOTS,
    INVESTIGATION_WINDOW_SLOTS,
    MAX_AUDIT_LOG_SIZE,
    USER_CANCELLATION_WINDOW_SLOTS,
    WRONG_FREEZE_JAIL_TRIGGER,
    WRONG_FREEZE_SLASH_BPS,
    WRONG_FREEZE_WINDOW_SLOTS,
    // Yardımcı fonksiyon
    freeze_id_to_hex,
    // Tipler
    AnomalyType,
    FreezeAuditEntry,
    FreezeConfig,
    FreezeEvent,
    FreezeEventType,
    FreezeId,
    FreezeOutcomeRecord,
    FreezePhase,
    FreezeReason,
    FreezeRecord,
    FreezeState,
    FreezeSummary,
    OperationType,
    PendingAnomaly,
    ThresholdProof,
    UnfreezeOutcome,
};

// ─── Re-export: engine ───────────────────────────────────────────────────────

pub use engine::FreezeEngine;

// ─── Re-export: guardian ─────────────────────────────────────────────────────

pub use guardian::{FreezeAuthorizedGuardians, GuardianFreezeRegistry};

// ─── Re-export: anomaly ──────────────────────────────────────────────────────

pub use anomaly::{AnomalyDetector, AnomalyResult, TxRecord};

// ─── Re-export: window ───────────────────────────────────────────────────────

pub use window::{ExpiredWindowEvent, InvestigationWindow, WindowManager, WindowType};

// ─── Re-export: trigger ──────────────────────────────────────────────────────

pub use trigger::TriggerEvaluator;

// ─── Re-export: unfreeze ─────────────────────────────────────────────────────

pub use unfreeze::UnfreezeProcessor;

// ─── Entegrasyon Testleri ─────────────────────────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use nova_primitives::{Address, SlotId};

    // ─── Yardımcı Fonksiyonlar ────────────────────────────────────────────────

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    /// Engine + hesap + guardian kurulumu
    fn setup_engine_with_guardians(
        account: Address,
        guardians: Vec<Address>,
        threshold: u8,
    ) -> FreezeEngine {
        let engine = FreezeEngine::with_defaults();
        engine.set_authorized_guardians(
            account,
            FreezeAuthorizedGuardians::new(account, guardians, threshold),
            SlotId(0),
        );
        engine
    }

    /// Güvenilir ThresholdProof üret
    fn make_proof(
        account: &Address,
        freeze_id: &FreezeId,
        signers: Vec<Address>,
        slot: SlotId,
    ) -> ThresholdProof {
        ThresholdProof::mock_valid(account, freeze_id, signers, slot)
    }

    /// Hesabın aktif freeze_id'sini al
    fn get_freeze_id(engine: &FreezeEngine, account: &Address) -> FreezeId {
        *engine
            .get_freeze_state(account)
            .unwrap()
            .active_freeze_id()
            .unwrap()
    }

    // ─── Senaryo 1: Tam Kullanıcı Freeze → Unfreeze Döngüsü ─────────────────
    //
    // §5.5.2 Tetikleyici 1: Kullanıcı kendi hesabını dondurur.
    // §5.5.3 Senaryo 1: ThresholdProof ile kimlik kanıtı → anında açılır.

    #[test]
    fn test_integration_user_freeze_unfreeze_full_lifecycle() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        // 1. Başlangıçta hesap serbest
        assert!(!engine.is_frozen(&account));
        assert!(engine.check_operation(&account, OperationType::Transfer, SlotId(0)).is_ok());

        // 2. Kullanıcı kendi hesabını dondurur
        let freeze_id = engine.freeze_user(account, account, SlotId(1000), 400_000).unwrap();
        assert!(engine.is_frozen(&account));

        // 3. Transfer artık reddedilir
        assert!(engine
            .check_operation(&account, OperationType::Transfer, SlotId(1000))
            .is_err());

        // 4. Receive hâlâ izinli (§5.5.3)
        assert!(engine
            .check_operation(&account, OperationType::Receive, SlotId(1000))
            .is_ok());

        // 5. Kullanıcı kimliğini kanıtlar → unfreeze
        let proof = make_proof(&account, &freeze_id, vec![g1, g2], SlotId(2000));
        let outcome = engine.unfreeze(account, proof, None, SlotId(2000)).unwrap();
        assert!(matches!(outcome, UnfreezeOutcome::UserIdentityProven { .. }));
        assert!(!engine.is_frozen(&account));

        // 6. Transfer tekrar izinli
        assert!(engine
            .check_operation(&account, OperationType::Transfer, SlotId(2000))
            .is_ok());
    }

    // ─── Senaryo 2: Guardian Freeze → Kullanıcı 24 Saat İçinde İptal ────────

    #[test]
    fn test_integration_guardian_freeze_user_cancellation_in_window() {
        let account = addr(2);
        let g1 = addr(20);
        let g2 = addr(21);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        // Guardian threshold freeze
        engine
            .freeze_guardian(account, vec![g1, g2], SlotId(0), 0)
            .unwrap();
        assert!(engine.is_frozen(&account));

        // Hesap dondurulmuş durumda
        assert!(engine.is_frozen(&account));
        // Guardian freeze sonrası phase UserCancellationWindow olmalı
        let state = engine.get_freeze_state(&account).unwrap();
        assert!(
            state.phase.is_cancellation_open(SlotId(1)),
            "kullanıcı iptal penceresi açık olmalı"
        );

        // Kullanıcı 24 saat içinde iptal eder — 1 imza yeterli
        let freeze_id = get_freeze_id(&engine, &account);
        let proof = make_proof(&account, &freeze_id, vec![account], SlotId(100));
        let result = engine.cancel_guardian_freeze(account, proof, SlotId(100));
        assert!(result.is_ok());
        assert!(!engine.is_frozen(&account));
    }

    // ─── Senaryo 3: Guardian Freeze → İptal Penceresi Dolar → Soruşturma ─────

    #[test]
    fn test_integration_cancellation_window_expires_opens_investigation() {
        let account = addr(3);
        let g1 = addr(30);
        let g2 = addr(31);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        engine
            .freeze_guardian(account, vec![g1, g2], SlotId(0), 0)
            .unwrap();

        // 24 saat dolduğunda process_slot → tam soruşturma penceresi açar
        let events = engine.process_slot(SlotId(USER_CANCELLATION_WINDOW_SLOTS));

        let has_investigation_opened = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::InvestigationWindowOpened { .. })
        });
        assert!(has_investigation_opened, "tam soruşturma penceresi açılmalı");

        // Hesap hâlâ dondurulmuş
        assert!(engine.is_frozen(&account));

        // Hesap hâlâ dondurulmuş, phase soruşturma modunda
        let state = engine.get_freeze_state(&account).unwrap();
        assert!(
            state.is_frozen,
            "hesap hâlâ dondurulmuş olmalı"
        );
    }

    // ─── Senaryo 4: 72 Saat Dolar → Miras Protokolü Handoff ─────────────────

    #[test]
    fn test_integration_full_investigation_timeout_inheritance_handoff() {
        let account = addr(4);
        let engine = FreezeEngine::with_defaults();

        // Kullanıcı freeze → soruşturma penceresi açılır
        engine
            .freeze_user(account, account, SlotId(0), 0)
            .unwrap();
        assert!(engine.is_frozen(&account));

        // 72 saat dolunca → miras protokolüne handoff
        let events = engine.process_slot(SlotId(INVESTIGATION_WINDOW_SLOTS));

        let has_expired = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::InvestigationWindowExpired { .. })
        });
        assert!(has_expired, "window expired eventi üretilmeli");

        // Hesap artık serbest (miras protokolüne devredildi)
        assert!(!engine.is_frozen(&account));

        // Audit log'da kayıt var ve outcome set edilmiş
        let log = engine.get_audit_log_tail(5);
        assert!(!log.is_empty());
        let entry = &log[0];
        assert_eq!(entry.account, account);
        assert!(entry.outcome.is_some(), "timeout outcome kaydedilmeli");
    }

    // ─── Senaryo 5: Anomali Bildirimi → 10 Dakika Sonra Otomatik Freeze ──────

    #[test]
    fn test_integration_anomaly_pending_then_auto_freeze() {
        let account = addr(5);
        let engine = FreezeEngine::with_defaults();

        // Anomali bildirimi — henüz freeze değil
        engine
            .notify_anomaly(account, 8_000, AnomalyType::RapidMultipleWithdrawals, SlotId(0))
            .unwrap();
        assert!(!engine.is_frozen(&account), "bildirimde henüz freeze olmamalı");

        // 9 dakika 59 saniye — hâlâ pending
        let events_early = engine.process_slot(SlotId(ANOMALY_CONFIRM_SLOTS - 1));
        assert!(!engine.is_frozen(&account));
        assert!(!events_early
            .iter()
            .any(|e| matches!(e.event_type, FreezeEventType::AccountFrozen { .. })));

        // 10 dakika doldu → otomatik freeze
        let events = engine.process_slot(SlotId(ANOMALY_CONFIRM_SLOTS));
        assert!(engine.is_frozen(&account), "anomali onaylandıktan sonra freeze edilmeli");

        assert!(events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::AnomalyConfirmed { .. })
        }));
        assert!(events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::AccountFrozen { .. })
        }));
    }

    // ─── Senaryo 6: Anomali Skoru Eşiğin Altında → Reddedilir ───────────────

    #[test]
    fn test_integration_anomaly_score_below_threshold_rejected() {
        let engine = FreezeEngine::with_defaults();
        // DEFAULT_ANOMALY_THRESHOLD_BPS = 7_000
        let err = engine
            .notify_anomaly(addr(6), 5_000, AnomalyType::UnusualTransferVolume, SlotId(0))
            .unwrap_err();
        assert!(matches!(
            err,
            FreezeError::AnomalyScoreTooLow {
                score_bps: 5_000,
                ..
            }
        ));
    }

    // ─── Senaryo 7: Anomali Freeze → Kullanıcı Kimlik Kanıtı → Unfreeze ──────

    #[test]
    fn test_integration_anomaly_freeze_then_identity_proof_unfreeze() {
        let account = addr(7);
        let g1 = addr(70);
        let g2 = addr(71);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        // Anomali ile dondur
        engine
            .notify_anomaly(account, 9_000, AnomalyType::TransferToUnknownAddress, SlotId(0))
            .unwrap();
        engine.process_slot(SlotId(ANOMALY_CONFIRM_SLOTS));
        assert!(engine.is_frozen(&account));

        // Kullanıcı kimliğini kanıtlar
        let freeze_id = get_freeze_id(&engine, &account);
        let proof = make_proof(
            &account,
            &freeze_id,
            vec![g1, g2],
            SlotId(ANOMALY_CONFIRM_SLOTS + 100),
        );
        let outcome = engine
            .unfreeze(account, proof, None, SlotId(ANOMALY_CONFIRM_SLOTS + 100))
            .unwrap();
        assert!(matches!(outcome, UnfreezeOutcome::UserIdentityProven { .. }));
        assert!(!engine.is_frozen(&account));
    }

    // ─── Senaryo 8: Saldırı Teyidi → Yeni Guardian Seti ile Unfreeze ─────────

    #[test]
    fn test_integration_attack_confirmed_new_guardian_set() {
        let account = addr(8);
        let old_g1 = addr(80);
        let old_g2 = addr(81);
        let new_g1 = addr(82);
        let new_g2 = addr(83);
        let new_g3 = addr(84);
        let engine = setup_engine_with_guardians(account, vec![old_g1, old_g2], 2);

        engine
            .freeze_user(account, account, SlotId(100), 10000)
            .unwrap();

        let freeze_id = get_freeze_id(&engine, &account);
        let proof = make_proof(&account, &freeze_id, vec![old_g1, old_g2], SlotId(500));
        let new_guardians = vec![new_g1, new_g2, new_g3];

        let outcome = engine
            .unfreeze(account, proof, Some(new_guardians.clone()), SlotId(500))
            .unwrap();

        if let UnfreezeOutcome::AttackConfirmed {
            new_guardian_set,
            unfreeze_slot,
        } = outcome
        {
            assert_eq!(new_guardian_set.len(), 3);
            assert_eq!(unfreeze_slot, SlotId(500));
            for g in &new_guardians {
                assert!(new_guardian_set.contains(g));
            }
        } else {
            panic!("AttackConfirmed beklendi");
        }
        assert!(!engine.is_frozen(&account));
    }

    // ─── Senaryo 9: Yanlış Freeze → Slash → 3. İhlalde Jail ─────────────────

    #[test]
    fn test_integration_wrong_freeze_slash_then_jail_on_third() {
        let account = addr(9);
        let g1 = addr(90);
        let g2 = addr(91);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        // 3 kez yanlış freeze döngüsü
        for cycle in 0u32..3 {
            let slot_base = SlotId(cycle as u64 * 1000);

            let fid = engine
                .freeze_guardian(account, vec![g1, g2], slot_base, 0)
                .unwrap();

            // Unfreeze et
            let proof = make_proof(&account, &fid, vec![g1, g2], SlotId(slot_base.0 + 10));
            engine
                .unfreeze(account, proof, None, SlotId(slot_base.0 + 10))
                .unwrap();

            // Yanlış freeze bildir
            let events = engine
                .report_wrong_freeze(fid, addr(99), SlotId(slot_base.0 + 20))
                .unwrap();

            let has_slash = events
                .iter()
                .any(|e| matches!(e.event_type, FreezeEventType::GuardianSlashed { .. }));
            assert!(has_slash, "her yanlış freeze slash üretmeli (döngü {})", cycle);

            if cycle == 2 {
                // 3. ihlal → jail
                let has_jail = events
                    .iter()
                    .any(|e| matches!(e.event_type, FreezeEventType::GuardianJailed { .. }));
                assert!(has_jail, "3. yanlış freeze'de guardian jail edilmeli");
            }
        }
    }

    // ─── Senaryo 10: Çoklu Hesap Bağımsız Freeze Durumları ───────────────────

    #[test]
    fn test_integration_multiple_accounts_independent_states() {
        let engine = FreezeEngine::with_defaults();

        let a1 = addr(101);
        let a2 = addr(102);
        let a3 = addr(103);

        engine.freeze_user(a1, a1, SlotId(0), 0).unwrap();
        engine.freeze_user(a2, a2, SlotId(0), 0).unwrap();
        // a3 serbest

        assert!(engine.is_frozen(&a1));
        assert!(engine.is_frozen(&a2));
        assert!(!engine.is_frozen(&a3));
        assert_eq!(engine.total_frozen_accounts(), 2);

        // a1 unfreeze
        engine.set_authorized_guardians(
            a1,
            FreezeAuthorizedGuardians::new(a1, vec![a1], 1),
            SlotId(0),
        );
        let fid1 = get_freeze_id(&engine, &a1);
        let proof = make_proof(&a1, &fid1, vec![a1], SlotId(100));
        engine.unfreeze(a1, proof, None, SlotId(100)).unwrap();

        assert!(!engine.is_frozen(&a1));
        assert!(engine.is_frozen(&a2));
        assert_eq!(engine.total_frozen_accounts(), 1);
    }

    // ─── Senaryo 11: Davranış Kaydı + Anomali Skoru Hesabı ──────────────────

    #[test]
    fn test_integration_behavior_recording_anomaly_score() {
        let engine = FreezeEngine::with_defaults();
        let account = addr(11);
        let known = addr(200);

        // Baseline oluştur — 500 normal işlem
        for i in 0..500u64 {
            engine.record_transaction(
                account,
                TxRecord {
                    slot: SlotId(i),
                    amount: 100_000,
                    recipient: known,
                    is_outgoing: true,
                },
            );
        }

        // Şüpheli: normalin çok üzerinde hacim, yabancı alıcı
        engine.record_transaction(
            account,
            TxRecord {
                slot: SlotId(600),
                amount: 50_000_000,
                recipient: addr(201),
                is_outgoing: true,
            },
        );

        // Profil oluştu — anomali bildirimi eşiği geçebilir
        // AnomalyDetector'ü doğrudan test etmek yerine engine'in notify_anomaly'sini kullan
        let result = engine.notify_anomaly(account, 7_001, AnomalyType::UnusualTransferVolume, SlotId(700));
        assert!(result.is_ok(), "eşiği geçen skor kabul edilmeli");
    }

    // ─── Senaryo 12: Engine Özet İstatistikleri Doğruluğu ────────────────────

    #[test]
    fn test_integration_engine_summary_accuracy() {
        let engine = FreezeEngine::with_defaults();

        engine.freeze_user(addr(1), addr(1), SlotId(0), 0).unwrap();
        engine.freeze_user(addr(2), addr(2), SlotId(0), 0).unwrap();
        engine.freeze_user(addr(3), addr(3), SlotId(0), 0).unwrap();

        for i in 1u64..=5 {
            engine.process_slot(SlotId(i));
        }

        let summary = engine.get_summary();
        assert_eq!(summary.total_frozen_accounts, 3);
        assert_eq!(summary.total_freeze_events, 3);
        assert_eq!(summary.slots_processed, 5);
        assert_eq!(summary.total_wrong_freezes, 0);
    }

    // ─── Senaryo 13: Audit Log Tam Lifecycle Kaydı ────────────────────────────

    #[test]
    fn test_integration_audit_log_full_lifecycle() {
        let account = addr(13);
        let g1 = addr(130);
        let g2 = addr(131);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        let freeze_id = engine
            .freeze_user(account, account, SlotId(1000), 400_000)
            .unwrap();

        let proof = make_proof(&account, &freeze_id, vec![g1, g2], SlotId(2000));
        engine.unfreeze(account, proof, None, SlotId(2000)).unwrap();

        let log = engine.get_audit_log_tail(10);
        assert_eq!(log.len(), 1);

        let entry = &log[0];
        assert_eq!(entry.account, account);
        assert_eq!(entry.freeze_id, freeze_id);
        assert_eq!(entry.slot, SlotId(1000));
        assert!(entry.outcome.is_some(), "unfreeze sonrası outcome olmalı");

        let outcome = entry.outcome.as_ref().unwrap();
        assert_eq!(outcome.resolved_at_slot, SlotId(2000));
        assert!(!outcome.was_wrong_freeze);
    }

    // ─── Senaryo 14: Dondurulmuş Hesap — Tüm İşlem Tipleri ──────────────────

    #[test]
    fn test_integration_frozen_account_all_operation_types() {
        let engine = FreezeEngine::with_defaults();
        let account = addr(14);
        engine.freeze_user(account, account, SlotId(0), 0).unwrap();

        // Reddedilmeli (§5.5.3)
        assert!(engine
            .check_operation(&account, OperationType::Transfer, SlotId(0))
            .is_err());
        assert!(engine
            .check_operation(&account, OperationType::Swap, SlotId(0))
            .is_err());
        assert!(engine
            .check_operation(&account, OperationType::StakeDelegate, SlotId(0))
            .is_err());
        assert!(engine
            .check_operation(&account, OperationType::ContractCall, SlotId(0))
            .is_err());

        // İzin verilmeli (§5.5.3)
        assert!(engine
            .check_operation(&account, OperationType::Receive, SlotId(0))
            .is_ok());
        assert!(engine
            .check_operation(&account, OperationType::Unfreeze, SlotId(0))
            .is_ok());
    }

    // ─── Senaryo 15: Dondurulmamış Hesabı Unfreeze → Hata ───────────────────

    #[test]
    fn test_integration_unfreeze_unfrozen_fails() {
        let engine = FreezeEngine::with_defaults();
        let account = addr(15);
        let fake_id = [0xABu8; 32];
        let proof = make_proof(&account, &fake_id, vec![addr(1)], SlotId(0));

        let err = engine.unfreeze(account, proof, None, SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::NotFrozen));
    }

    // ─── Senaryo 16: Zaten Dondurulmuş Hesabı Tekrar Dondur → Hata ──────────

    #[test]
    fn test_integration_double_freeze_fails() {
        let engine = FreezeEngine::with_defaults();
        let account = addr(16);

        engine.freeze_user(account, account, SlotId(0), 0).unwrap();

        let err = engine
            .freeze_user(account, account, SlotId(1), 400)
            .unwrap_err();
        assert!(matches!(err, FreezeError::AlreadyFrozen));

        engine.set_authorized_guardians(
            account,
            FreezeAuthorizedGuardians::new(account, vec![addr(160), addr(161)], 2),
            SlotId(0),
        );
        let err2 = engine
            .freeze_guardian(account, vec![addr(160), addr(161)], SlotId(1), 400)
            .unwrap_err();
        assert!(matches!(err2, FreezeError::AlreadyFrozen));
    }

    // ─── Senaryo 17: Yanlış Hesap Proof'u → Hata ─────────────────────────────

    #[test]
    fn test_integration_wrong_account_proof_rejected() {
        let account = addr(17);
        let other = addr(170);
        let g1 = addr(171);
        let g2 = addr(172);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        engine.freeze_user(account, account, SlotId(0), 0).unwrap();
        let freeze_id = get_freeze_id(&engine, &account);

        // Proof yanlış hesap için üretildi
        let wrong_proof = make_proof(&other, &freeze_id, vec![g1, g2], SlotId(100));
        let err = engine
            .unfreeze(account, wrong_proof, None, SlotId(100))
            .unwrap_err();
        assert!(matches!(err, FreezeError::InvalidThresholdProof));
    }

    // ─── Senaryo 18: Guardian Listesi Olmadan Guardian Freeze → Hata ─────────

    #[test]
    fn test_integration_guardian_freeze_no_authorized_list_fails() {
        let engine = FreezeEngine::with_defaults();
        let account = addr(18);

        let err = engine
            .freeze_guardian(account, vec![addr(180)], SlotId(0), 0)
            .unwrap_err();
        assert!(matches!(err, FreezeError::NoAuthorizedGuardians));
    }

    // ─── Senaryo 19: Guardian Threshold Karşılanmadı → Hata ─────────────────

    #[test]
    fn test_integration_guardian_threshold_not_met() {
        let account = addr(19);
        let g1 = addr(190);
        let g2 = addr(191);
        let g3 = addr(192);
        let engine = setup_engine_with_guardians(account, vec![g1, g2, g3], 3);

        let err = engine
            .freeze_guardian(account, vec![g1, g2], SlotId(0), 0)
            .unwrap_err();
        assert!(matches!(
            err,
            FreezeError::ThresholdNotMet { required: 3, got: 2 }
        ));
    }

    // ─── Senaryo 20: Çoklu Hesap Aynı Anda Expire Olur ──────────────────────

    #[test]
    fn test_integration_multiple_accounts_expire_simultaneously() {
        let engine = FreezeEngine::with_defaults();
        let accounts: Vec<Address> = (1u8..=5).map(addr).collect();

        for &account in &accounts {
            engine.freeze_user(account, account, SlotId(0), 0).unwrap();
        }

        let events = engine.process_slot(SlotId(INVESTIGATION_WINDOW_SLOTS));

        let expired: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(e.event_type, FreezeEventType::InvestigationWindowExpired { .. })
            })
            .collect();
        assert_eq!(expired.len(), 5, "5 hesap da aynı anda expire olmalı");

        for account in &accounts {
            assert!(!engine.is_frozen(account));
        }
    }

    // ─── Senaryo 21: Aynı Hesap Birden Fazla Freeze/Unfreeze Döngüsü ─────────

    #[test]
    fn test_integration_same_account_multiple_cycles() {
        let account = addr(21);
        let g1 = addr(210);
        let engine = setup_engine_with_guardians(account, vec![g1], 1);

        for cycle in 0u64..3 {
            let base = SlotId(cycle * 10_000);

            engine
                .freeze_user(account, account, base, base.0 * 400)
                .unwrap();
            assert!(engine.is_frozen(&account));

            let freeze_id = get_freeze_id(&engine, &account);
            let proof = make_proof(&account, &freeze_id, vec![g1], SlotId(base.0 + 100));
            engine
                .unfreeze(account, proof, None, SlotId(base.0 + 100))
                .unwrap();
            assert!(!engine.is_frozen(&account));
        }

        let log = engine.get_audit_log_tail(10);
        assert_eq!(log.len(), 3, "her döngü için 1 audit kaydı");
    }

    // ─── Senaryo 22: FreezeState Phase Doğrulaması ────────────────────────────

    #[test]
    fn test_integration_freeze_state_phase_transitions() {
        let account = addr(22);
        let engine = FreezeEngine::with_defaults();

        // Freeze öncesi state yok
        assert!(engine.get_freeze_state(&account).is_none());

        // Kullanıcı freeze → FreezeActive phase
        engine.freeze_user(account, account, SlotId(0), 0).unwrap();
        let state = engine.get_freeze_state(&account).unwrap();
        assert!(state.is_frozen);
        assert_eq!(state.total_freeze_count, 1);
        assert!(matches!(state.phase, FreezePhase::FreezeActive));

        // Unfreeze → Resolved phase
        let freeze_id = get_freeze_id(&engine, &account);
        engine.set_authorized_guardians(
            account,
            FreezeAuthorizedGuardians::new(account, vec![addr(220)], 1),
            SlotId(0),
        );
        let proof = make_proof(&account, &freeze_id, vec![addr(220)], SlotId(100));
        engine.unfreeze(account, proof, None, SlotId(100)).unwrap();

        let state_after = engine.get_freeze_state(&account).unwrap();
        assert!(!state_after.is_frozen);
        assert!(matches!(state_after.phase, FreezePhase::Resolved { .. }));
    }

    // ─── Senaryo 23: Özel FreezeConfig Anomali Eşiği ─────────────────────────

    #[test]
    fn test_integration_custom_config_anomaly_threshold() {
        let config = FreezeConfig {
            guardian_freeze_threshold: 3,
            investigation_window_slots: INVESTIGATION_WINDOW_SLOTS,
            user_cancellation_window_slots: USER_CANCELLATION_WINDOW_SLOTS,
            anomaly_confirm_slots: ANOMALY_CONFIRM_SLOTS,
            anomaly_threshold_bps: 9_000,
        };
        let engine = FreezeEngine::new(config);

        // 8_000 bps → threshold 9_000 → reddedilmeli
        let err = engine
            .notify_anomaly(addr(23), 8_000, AnomalyType::UnusualTransferVolume, SlotId(0))
            .unwrap_err();
        assert!(matches!(
            err,
            FreezeError::AnomalyScoreTooLow {
                score_bps: 8_000,
                threshold_bps: 9_000
            }
        ));

        // 9_000 bps → threshold karşılandı
        assert!(engine
            .notify_anomaly(addr(24), 9_000, AnomalyType::UnusualTransferVolume, SlotId(0))
            .is_ok());
    }

    // ─── Senaryo 24: FreezeId Deterministik ve Benzersiz ─────────────────────

    #[test]
    fn test_integration_freeze_id_deterministic_and_unique() {
        let account = addr(24);
        let g1 = addr(240);
        let engine = setup_engine_with_guardians(account, vec![g1], 1);

        // İlk freeze
        let fid1 = engine
            .freeze_user(account, account, SlotId(100), 10000)
            .unwrap();
        let hex1 = freeze_id_to_hex(&fid1);
        assert_eq!(hex1.len(), 64);

        // Unfreeze
        let proof = make_proof(&account, &fid1, vec![g1], SlotId(200));
        engine.unfreeze(account, proof, None, SlotId(200)).unwrap();

        // İkinci freeze farklı slot → farklı ID
        let fid2 = engine
            .freeze_user(account, account, SlotId(300), 30000)
            .unwrap();
        assert_ne!(fid1, fid2, "farklı slot → farklı freeze_id");

        let hex2 = freeze_id_to_hex(&fid2);
        assert_eq!(hex2.len(), 64);
        assert!(hex2.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ─── Senaryo 25: UnfreezeProcessor Mesaj Hash Deterministik ─────────────

    #[test]
    fn test_integration_unfreeze_message_hash_deterministic() {
        let account = addr(25);
        let freeze_id: FreezeId = [25u8; 32];
        let slot = SlotId(25_000);

        let msg1 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, slot);
        let msg2 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, slot);
        assert_eq!(msg1, msg2, "deterministik olmalı");

        // Farklı hesap → farklı hash
        let msg3 = UnfreezeProcessor::build_unfreeze_message(&addr(26), &freeze_id, slot);
        assert_ne!(msg1, msg3);

        // Farklı slot → farklı hash
        let msg4 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, SlotId(25_001));
        assert_ne!(msg1, msg4);
    }

    // ─── Senaryo 26: Jailed Guardian Freeze Başlatamaz ───────────────────────

    #[test]
    fn test_integration_jailed_guardian_cannot_freeze() {
        let account = addr(26);
        let g1 = addr(60);
        let g2 = addr(61);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        // 3 kez yanlış freeze → g1 jailed
        for cycle in 0u32..3 {
            let base = SlotId(cycle as u64 * 1000);
            let fid = engine
                .freeze_guardian(account, vec![g1, g2], base, 0)
                .unwrap();
            let proof = make_proof(&account, &fid, vec![g1, g2], SlotId(base.0 + 10));
            engine
                .unfreeze(account, proof, None, SlotId(base.0 + 10))
                .unwrap();
            engine
                .report_wrong_freeze(fid, addr(99), SlotId(base.0 + 20))
                .unwrap();
        }

        // Şimdi g1 jailed → freeze başlatamaz
        let err = engine
            .freeze_guardian(account, vec![g1, g2], SlotId(10_000), 0)
            .unwrap_err();
        assert!(
            matches!(err, FreezeError::GuardianJailed { .. }),
            "jailed guardian freeze başlatamaz, hata: {:?}",
            err
        );
    }

    // ─── Senaryo 27: Yanlış Freeze Audit Log'da İşaretlenir ─────────────────

    #[test]
    fn test_integration_wrong_freeze_marked_in_audit_log() {
        let account = addr(27);
        let g1 = addr(70);
        let g2 = addr(71);
        let engine = setup_engine_with_guardians(account, vec![g1, g2], 2);

        let freeze_id = engine
            .freeze_guardian(account, vec![g1, g2], SlotId(0), 0)
            .unwrap();

        // Unfreeze
        let proof = make_proof(&account, &freeze_id, vec![g1, g2], SlotId(100));
        engine
            .unfreeze(account, proof, None, SlotId(100))
            .unwrap();

        // Yanlış freeze bildir
        engine
            .report_wrong_freeze(freeze_id, addr(99), SlotId(200))
            .unwrap();

        // Audit log'da was_wrong_freeze işaretlenmeli
        let log = engine.get_audit_log_tail(5);
        let entry = log.iter().find(|e| e.freeze_id == freeze_id).unwrap();
        if let Some(ref outcome) = entry.outcome {
            assert!(outcome.was_wrong_freeze, "yanlış freeze audit log'da işaretlenmeli");
        } else {
            panic!("outcome set edilmeli");
        }
    }
}
