//! Freeze kaldırma işlemleri — §5.5.3
//!
//! Araştırma kaynakları:
//! - TSS-lib signing/finalize.go:
//!   `sumS = Σ(si)`, `ecdsa.Verify(pk, M, rx, sumS)` — threshold proof toplama + doğrulama.
//!   Bizde: `ThresholdProof::verify(account, freeze_id)` — commitment tabanlı.
//! - Multi-party ECDSA blame.rs `GlobalStatePhase5`:
//!   Commitment vs açılmış değer karşılaştırması → kim hata yaptı tespiti.
//!   Bizde: proof'un mesaj hash'ini ve commitment'ını doğrula → kim imzaladı güvenilir mi.
//! - Solana ALT freeze `freeze_lookup_table()`:
//!   Pre-condition guards: "already frozen", "deactivated", "empty" kontrolleri.
//!   Bizde: "not frozen", "wrong freeze_id", "window expired" kontrolleri.
//!
//! Üç unfreeze senaryosu (Whitepaper §5.5.3):
//! 1. Kullanıcı kimliğini kanıtlar → anında açılır
//! 2. Saldırı doğrulanır → yeni guardian seti ile açılır
//! 3. 72 saat geçer → miras protokolü devreye girer

use crate::errors::{FreezeError, FreezeResult};
use crate::types::{FreezeId, ThresholdProof, UnfreezeOutcome};
use nova_primitives::{Address, SlotId};

// ─── UnfreezeProcessor ───────────────────────────────────────────────────────

/// Unfreeze işlemlerini yöneten processor.
///
/// Pattern: Cosmos SDK `Keeper.Unjail()` — koşulları doğrula, state'i güncelle.
/// Stateless: tüm state dışarıdan geçirilir.
pub struct UnfreezeProcessor;

impl UnfreezeProcessor {
    // ─── Mesaj Hash ───────────────────────────────────────────────────────────

    /// Unfreeze işlemi için imzalanması gereken mesajı hesapla.
    ///
    /// Format: blake3(account_bytes || freeze_id || slot_le_bytes || b"nova_unfreeze")
    ///
    /// "nova_unfreeze" domain separator: TSS-lib'in tag-based hash separation pattern'i.
    pub fn build_unfreeze_message(account: &Address, freeze_id: &FreezeId, slot: SlotId) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(account.as_bytes());
        hasher.update(freeze_id.as_slice());
        hasher.update(&slot.0.to_le_bytes());
        hasher.update(b"nova_unfreeze");
        *hasher.finalize().as_bytes()
    }

    // ─── Proof Doğrulama ─────────────────────────────────────────────────────

    /// ThresholdProof doğrulama.
    ///
    /// Kontroller (Solana freeze guard'ları benzeri, cheap→expensive):
    /// 1. İmzalayan sayısı >= required_threshold
    /// 2. proof.verify() — mesaj hash + commitment eşleşmesi
    ///
    /// Pattern: TSS-lib finalize.go `ecdsa.Verify(pk, M, rx, sumS)`.
    pub fn verify_threshold_proof(
        account: &Address,
        freeze_id: &FreezeId,
        proof: &ThresholdProof,
        required_threshold: u8,
    ) -> FreezeResult<()> {
        // 1. İmzalayan sayısı yeterli mi? (cheap check)
        let signer_count = proof.signers.len() as u8;
        if signer_count < required_threshold {
            return Err(FreezeError::InsufficientSigners {
                required: required_threshold,
                provided: signer_count,
            });
        }

        // 2. Proof commitment doğrulaması (expensive check — en sona)
        if !proof.verify(account, freeze_id) {
            return Err(FreezeError::InvalidThresholdProof);
        }

        Ok(())
    }

    // ─── Senaryo 1: Kullanıcı Kimlik Kanıtı ─────────────────────────────────

    /// Kullanıcı MPC threshold imzasıyla kimliğini kanıtlar → hesap açılır.
    ///
    /// §5.5.3: "MPC threshold imzası ile kimlik doğrulama → Hesap anında açılır"
    ///
    /// Proof, kullanıcının kendi hesabında en az `required_threshold` imza
    /// toplamasını gerektiriyor.
    pub fn process_user_unfreeze(
        account: &Address,
        freeze_id: &FreezeId,
        proof: ThresholdProof,
        required_threshold: u8,
        current_slot: SlotId,
    ) -> FreezeResult<UnfreezeOutcome> {
        // Proof doğrulaması
        Self::verify_threshold_proof(account, freeze_id, &proof, required_threshold)?;

        Ok(UnfreezeOutcome::UserIdentityProven {
            unfreeze_slot: current_slot,
        })
    }

    // ─── Senaryo 2: Saldırı Teyidi + Yeni Guardian Seti ─────────────────────

    /// Saldırı doğrulandı, yeni guardian seti atanır ve hesap açılır.
    ///
    /// §5.5.3: "Guardian'lar yeni cihazlara key share dağıtır, eskiler geçersiz kılınır"
    ///
    /// Gereklilikler:
    /// - Proof geçerli (eski guardian'ların onayı)
    /// - Yeni guardian listesi boş olmamalı
    pub fn process_attack_confirmed(
        account: &Address,
        freeze_id: &FreezeId,
        proof: ThresholdProof,
        new_guardians: Vec<Address>,
        required_threshold: u8,
        current_slot: SlotId,
    ) -> FreezeResult<UnfreezeOutcome> {
        if new_guardians.is_empty() {
            return Err(FreezeError::Internal(
                "saldırı teyidi için yeni guardian listesi gerekli".to_string(),
            ));
        }

        // Proof doğrulaması (eski guardian'lardan)
        Self::verify_threshold_proof(account, freeze_id, &proof, required_threshold)?;

        Ok(UnfreezeOutcome::AttackConfirmed {
            new_guardian_set: new_guardians,
            unfreeze_slot: current_slot,
        })
    }

    // ─── Senaryo 3: Timeout → Miras Protokolü ────────────────────────────────

    /// 72 saat geçti, kullanıcıya ulaşılamadı → miras protokolüne handoff.
    ///
    /// §5.5.3: "Kullanıcıya ulaşılamazsa → Miras protokolü devreye girer (5.4 kurallarına göre)"
    ///
    /// Bu senaryo proof gerektirmez; zaman aşımı otomatik tetikler.
    pub fn process_timeout(current_slot: SlotId) -> UnfreezeOutcome {
        UnfreezeOutcome::TimedOutToInheritance {
            handoff_slot: current_slot,
        }
    }

    // ─── Kullanıcı Guardian Freeze İptal ─────────────────────────────────────

    /// Guardian freeze'ini 24 saat içinde kullanıcı iptal eder.
    ///
    /// §5.5.2 Tetikleyici 2: "Bu karar 24 saat içinde kullanıcı tarafından iptal edilebilir"
    ///
    /// Tek kullanıcı imzası yeterli (threshold değil — kullanıcı kendi hesabını iptal eder).
    pub fn process_user_cancellation(
        account: &Address,
        freeze_id: &FreezeId,
        proof: ThresholdProof,
        current_slot: SlotId,
    ) -> FreezeResult<UnfreezeOutcome> {
        // Kullanıcı kendi hesabını iptal ediyor: 1 imza yeterli
        Self::verify_threshold_proof(account, freeze_id, &proof, 1)?;

        Ok(UnfreezeOutcome::UserCancelled {
            cancelled_at_slot: current_slot,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ThresholdProof;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_freeze_id(seed: u8) -> FreezeId {
        [seed; 32]
    }

    fn valid_proof(account: &Address, freeze_id: &FreezeId, signers: Vec<Address>, slot: SlotId) -> ThresholdProof {
        ThresholdProof::mock_valid(account, freeze_id, signers, slot)
    }

    // ─── build_unfreeze_message ───────────────────────────────────────────────

    #[test]
    fn test_build_unfreeze_message_deterministic() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let slot = SlotId(1000);

        let msg1 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, slot);
        let msg2 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, slot);
        assert_eq!(msg1, msg2, "deterministic olmalı");
    }

    #[test]
    fn test_build_unfreeze_message_different_inputs() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);

        let msg1 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, SlotId(1000));
        let msg2 = UnfreezeProcessor::build_unfreeze_message(&account, &freeze_id, SlotId(1001));
        let msg3 = UnfreezeProcessor::build_unfreeze_message(&addr(2), &freeze_id, SlotId(1000));
        assert_ne!(msg1, msg2, "farklı slot → farklı mesaj");
        assert_ne!(msg1, msg3, "farklı hesap → farklı mesaj");
    }

    // ─── verify_threshold_proof ───────────────────────────────────────────────

    #[test]
    fn test_verify_threshold_proof_success() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let signers = vec![addr(10), addr(11), addr(12)];
        let proof = valid_proof(&account, &freeze_id, signers, SlotId(1000));

        let result = UnfreezeProcessor::verify_threshold_proof(&account, &freeze_id, &proof, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_threshold_proof_insufficient_signers() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let proof = valid_proof(&account, &freeze_id, vec![addr(10)], SlotId(1000));

        let err = UnfreezeProcessor::verify_threshold_proof(&account, &freeze_id, &proof, 3).unwrap_err();
        assert!(matches!(err, FreezeError::InsufficientSigners { required: 3, provided: 1 }));
    }

    #[test]
    fn test_verify_threshold_proof_tampered_commitment() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let mut proof = valid_proof(&account, &freeze_id, vec![addr(10), addr(11)], SlotId(1000));
        // Aggregate imzayı boz — BLS doğrulama başarısız olmalı
        let mut corrupted_sig = *proof.aggregate_signature.as_bytes();
        corrupted_sig[0] ^= 0xFF;
        proof.aggregate_signature = nova_primitives::crypto::BlsSignature::from_bytes(corrupted_sig);

        let err = UnfreezeProcessor::verify_threshold_proof(&account, &freeze_id, &proof, 2).unwrap_err();
        assert!(matches!(err, FreezeError::InvalidThresholdProof));
    }

    #[test]
    fn test_verify_threshold_proof_wrong_freeze_id() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let wrong_id = make_freeze_id(99);
        let proof = valid_proof(&account, &freeze_id, vec![addr(10), addr(11)], SlotId(1000));

        let err = UnfreezeProcessor::verify_threshold_proof(&account, &wrong_id, &proof, 2).unwrap_err();
        assert!(matches!(err, FreezeError::InvalidThresholdProof));
    }

    // ─── process_user_unfreeze ────────────────────────────────────────────────

    #[test]
    fn test_user_unfreeze_success() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let proof = valid_proof(&account, &freeze_id, vec![addr(10), addr(11)], SlotId(500));

        let outcome = UnfreezeProcessor::process_user_unfreeze(
            &account, &freeze_id, proof, 2, SlotId(500),
        ).unwrap();

        assert!(matches!(outcome, UnfreezeOutcome::UserIdentityProven { unfreeze_slot: SlotId(500) }));
    }

    #[test]
    fn test_user_unfreeze_invalid_proof() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let mut proof = valid_proof(&account, &freeze_id, vec![addr(10)], SlotId(500));
        // Aggregate imzayı tamamen geçersiz yap
        let mut corrupted_sig = *proof.aggregate_signature.as_bytes();
        corrupted_sig[0] ^= 0xFF;
        proof.aggregate_signature = nova_primitives::crypto::BlsSignature::from_bytes(corrupted_sig);

        let err = UnfreezeProcessor::process_user_unfreeze(
            &account, &freeze_id, proof, 1, SlotId(500),
        ).unwrap_err();
        assert!(matches!(err, FreezeError::InvalidThresholdProof));
    }

    // ─── process_attack_confirmed ─────────────────────────────────────────────

    #[test]
    fn test_attack_confirmed_success() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let old_guardians = vec![addr(10), addr(11)];
        let new_guardians = vec![addr(20), addr(21), addr(22)];
        let proof = valid_proof(&account, &freeze_id, old_guardians, SlotId(600));

        let outcome = UnfreezeProcessor::process_attack_confirmed(
            &account, &freeze_id, proof, new_guardians.clone(), 2, SlotId(600),
        ).unwrap();

        if let UnfreezeOutcome::AttackConfirmed { new_guardian_set, unfreeze_slot } = outcome {
            assert_eq!(new_guardian_set, new_guardians);
            assert_eq!(unfreeze_slot, SlotId(600));
        } else {
            panic!("AttackConfirmed outcome beklendi");
        }
    }

    #[test]
    fn test_attack_confirmed_empty_new_guardians() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let proof = valid_proof(&account, &freeze_id, vec![addr(10), addr(11)], SlotId(600));

        let err = UnfreezeProcessor::process_attack_confirmed(
            &account, &freeze_id, proof, vec![], 2, SlotId(600),
        ).unwrap_err();
        assert!(matches!(err, FreezeError::Internal(_)));
    }

    // ─── process_timeout ──────────────────────────────────────────────────────

    #[test]
    fn test_timeout_produces_inheritance_handoff() {
        let outcome = UnfreezeProcessor::process_timeout(SlotId(648_000));
        assert!(matches!(outcome, UnfreezeOutcome::TimedOutToInheritance { handoff_slot: SlotId(648_000) }));
    }

    // ─── process_user_cancellation ────────────────────────────────────────────

    #[test]
    fn test_user_cancellation_success() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let proof = valid_proof(&account, &freeze_id, vec![addr(1)], SlotId(1000));

        let outcome = UnfreezeProcessor::process_user_cancellation(
            &account, &freeze_id, proof, SlotId(1000),
        ).unwrap();

        assert!(matches!(outcome, UnfreezeOutcome::UserCancelled { cancelled_at_slot: SlotId(1000) }));
    }

    #[test]
    fn test_user_cancellation_requires_at_least_1_signer() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        // Boş signers
        let mut proof = valid_proof(&account, &freeze_id, vec![addr(1)], SlotId(1000));
        proof.signers.clear();
        // Commitment artık eşleşmeyecek → InvalidThresholdProof veya InsufficientSigners
        let err = UnfreezeProcessor::process_user_cancellation(
            &account, &freeze_id, proof, SlotId(1000),
        ).unwrap_err();
        assert!(matches!(
            err,
            FreezeError::InsufficientSigners { required: 1, provided: 0 } | FreezeError::InvalidThresholdProof
        ));
    }
}
