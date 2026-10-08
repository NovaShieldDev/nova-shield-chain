//! nova-freeze hata tipleri — §5.5 Emergency Freeze Protocol
//!
//! Cosmos SDK x/slashing hata pattern'inden ilham alındı:
//! her hata kendi bağlamını taşır, string formatlamaya gerek yok.

use thiserror::Error;

/// FreezeResult — tüm freeze işlemleri bu tip döner.
pub type FreezeResult<T> = Result<T, FreezeError>;

/// nova-freeze modülünün tüm hata varyantları.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FreezeError {
    // ─── Hesap durumu ───────────────────────────────────────────────────────
    /// Hesap bulunamadı.
    #[error("hesap bulunamadı: {address}")]
    AccountNotFound { address: String },

    /// Hesap zaten dondurulmuş.
    #[error("hesap zaten dondurulmuş")]
    AlreadyFrozen,

    /// Hesap dondurulmuş değil.
    #[error("hesap dondurulmuş değil")]
    NotFrozen,

    // ─── Guardian yetkilendirme ─────────────────────────────────────────────
    /// Yetkisiz başlatıcı — freeze_user'da initiator hesap sahibi olmalı.
    ///
    /// NOVA-RED-FREEZE-003: Herhangi biri başkasının hesabını dondurmasını engelle.
    #[error("yetkisiz freeze başlatıcı: initiator {initiator} != hesap {account}")]
    UnauthorizedFreezeInitiator { initiator: String, account: String },

    /// Guardian bu hesabı dondurmaya yetkili değil.
    #[error("guardian dondurma yetkisi yok")]
    GuardianNotAuthorized,

    /// Gerekli threshold karşılanmadı.
    #[error("guardian threshold karşılanmadı: gerekli {required}, mevcut {got}")]
    ThresholdNotMet { required: u8, got: u8 },

    /// Guardian askıya alınmış (jailed).
    /// Pattern: Cosmos SDK slashing `JailedUntil` alanından.
    #[error("guardian jailed — slot {jailed_until_slot}'e kadar")]
    GuardianJailed { jailed_until_slot: u64 },

    // ─── Proof doğrulama ────────────────────────────────────────────────────
    /// ThresholdProof geçersiz.
    #[error("threshold proof geçersiz")]
    InvalidThresholdProof,

    /// Proof için imzalayan sayısı yetersiz.
    #[error("proof için imzalayan yetersiz: gerekli {required}, verilen {provided}")]
    InsufficientSigners { required: u8, provided: u8 },

    // ─── Soruşturma penceresi ───────────────────────────────────────────────
    /// Soruşturma penceresi açık değil.
    #[error("soruşturma penceresi açık değil")]
    NoInvestigationWindow,

    /// Soruşturma penceresi zaten kapandı.
    #[error("soruşturma penceresi kapandı — slot {expired_at_slot}")]
    InvestigationWindowExpired { expired_at_slot: u64 },

    // ─── İşlem reddi ───────────────────────────────────────────────────────
    /// Dondurulmuş hesaptan işlem reddedildi.
    /// Pattern: go-ethereum txpool ErrTxPoolOverflow benzeri.
    #[error("işlem reddedildi — hesap dondurulmuş (freeze_id: {freeze_id})")]
    OperationRejected { freeze_id: String },

    // ─── Anomali ────────────────────────────────────────────────────────────
    /// Anomali skoru eşiğin altında.
    #[error("anomali skoru yetersiz: {score_bps} bps (gerekli: {threshold_bps} bps)")]
    AnomalyScoreTooLow { score_bps: u32, threshold_bps: u32 },

    /// Anomali tespiti için yeterli davranış geçmişi yok.
    /// Pattern: Polkadot im-online `minHeight` check benzeri.
    #[error("anomali tespiti için yeterli işlem geçmişi yok")]
    InsufficientBehaviorHistory,

    // ─── Kayıt bulunamadı ───────────────────────────────────────────────────
    /// Freeze kaydı bulunamadı.
    #[error("freeze kaydı bulunamadı: {freeze_id}")]
    FreezeNotFound { freeze_id: String },

    /// Yanlış freeze bildirimi geçersiz.
    #[error("yanlış freeze bildirimi geçersiz: kayıt bulunamadı veya zaten çözüldü")]
    WrongFreezeReportInvalid,

    /// Yanlış freeze bildirimi yetkisiz — sadece dondurulan hesap sahibi veya yetkili guardian bildirebilir.
    #[error("yanlış freeze bildirimi yetkisiz: reporter={reporter}")]
    UnauthorizedWrongFreezeReporter { reporter: String },

    /// Guardian threshold'da aynı imzacı birden fazla kez gönderildi.
    #[error("guardian threshold: tekrarlı imzacı tespit edildi — her guardian sadece bir kez sayılır")]
    DuplicateGuardianSigner,

    // ─── Yapılandırma ───────────────────────────────────────────────────────
    /// Bu hesap için freeze yetkili guardian listesi tanımlı değil.
    #[error("hesap için freeze yetkili guardian listesi tanımlı değil")]
    NoAuthorizedGuardians,

    // ─── Dahili hata ────────────────────────────────────────────────────────
    /// Dahili hata.
    #[error("dahili hata: {0}")]
    Internal(String),
}

impl FreezeError {
    /// Hata bir konfigürasyon hatasından mı kaynaklanıyor?
    pub fn is_config_error(&self) -> bool {
        matches!(self, Self::NoAuthorizedGuardians | Self::ThresholdNotMet { .. })
    }

    /// Hata bir yetki hatasından mı kaynaklanıyor?
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::UnauthorizedFreezeInitiator { .. }
                | Self::GuardianNotAuthorized
                | Self::GuardianJailed { .. }
                | Self::InvalidThresholdProof
                | Self::InsufficientSigners { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeze_error_display() {
        let e = FreezeError::ThresholdNotMet { required: 3, got: 1 };
        let msg = e.to_string();
        assert!(msg.contains("threshold"), "mesaj threshold içermeli");
        assert!(msg.contains('3'), "gerekli sayı mesajda olmalı");
    }

    #[test]
    fn test_freeze_error_is_auth() {
        assert!(FreezeError::GuardianNotAuthorized.is_auth_error());
        assert!(FreezeError::GuardianJailed { jailed_until_slot: 100 }.is_auth_error());
        assert!(FreezeError::InvalidThresholdProof.is_auth_error());
        assert!(!FreezeError::AlreadyFrozen.is_auth_error());
    }

    #[test]
    fn test_freeze_error_is_config() {
        assert!(FreezeError::NoAuthorizedGuardians.is_config_error());
        assert!(FreezeError::ThresholdNotMet { required: 2, got: 0 }.is_config_error());
        assert!(!FreezeError::AlreadyFrozen.is_config_error());
    }

    #[test]
    fn test_operation_rejected_includes_freeze_id() {
        let e = FreezeError::OperationRejected { freeze_id: "abc123".into() };
        let msg = e.to_string();
        assert!(msg.contains("abc123"));
    }
}
