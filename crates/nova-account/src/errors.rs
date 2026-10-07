//! nova-account — Hata Tipleri
//!
//! Whitepaper §5.1 — MPC-Native Hesap Modeli
//!
//! Go-Ethereum'dan öğrenilen desen: Her alt sistem kendi `Error` enum'ını tanımlar;
//! üst katman `From<SubError>` ile dönüştürür. Cosmos-SDK'nın modül hata kodlama
//! yaklaşımı burada `AccountError` içinde hata kategorileriyle yansıtılmaktadır.

use thiserror::Error;

/// nova-account tüm hata tipleri.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AccountError {
    // ─── Hesap Bulunamadı / Var Olmayan Hesap ───────────────────────────────
    /// Verilen adrese karşılık hesap bulunamadı.
    #[error("hesap bulunamadı: {address}")]
    AccountNotFound { address: String },

    /// Hesap zaten mevcut, tekrar oluşturulamaz.
    #[error("hesap zaten mevcut: {address}")]
    AccountAlreadyExists { address: String },

    // ─── Eşik (Threshold) Hataları ──────────────────────────────────────────
    /// Geçersiz threshold parametresi — t > n veya t < 1.
    #[error("geçersiz threshold: t={threshold}, n={total} — t ≤ n ve t ≥ 1 olmalıdır")]
    InvalidThreshold { threshold: u8, total: u8 },

    /// Threshold asgari imza sayısı karşılanamadı.
    #[error("yetersiz imza: {provided} imza var, {required} gerekli")]
    InsufficientSignatures { provided: usize, required: usize },

    /// Minimum threshold 2/3 olmalı (§5.1.2 uyum kontrolü).
    #[error("eşik çok düşük: minimum 2/3 guardian gerekli, verildi {threshold}/{total}")]
    ThresholdTooLow { threshold: u8, total: u8 },

    /// Maksimum threshold aşıldı — §5.1.2: maks 10/15.
    #[error("guardian sayısı aşıldı: maksimum {max} guardian, verildi {given}")]
    TooManyGuardians { max: u8, given: u8 },

    // ─── Guardian Hataları ──────────────────────────────────────────────────
    /// Guardian bulunamadı (index veya adres bazlı).
    #[error("guardian bulunamadı: {guardian_id}")]
    GuardianNotFound { guardian_id: String },

    /// Guardian zaten var, tekrar eklenemez.
    #[error("guardian zaten kayıtlı: {guardian_id}")]
    GuardianAlreadyRegistered { guardian_id: String },

    /// Guardian değişikliği bekleme süresi dolmadı (§5.1.5: 7 gün).
    #[error("guardian değişikliği bekleme süresi dolmadı: {remaining_seconds} saniye kaldı")]
    GuardianChangePending { remaining_seconds: u64 },

    /// Guardian aktivasyon koşulu sağlanmadı.
    #[error("guardian aktif değil: {guardian_id} — durum: {status}")]
    GuardianNotActive { guardian_id: String, status: String },

    /// Guardian kötü niyetli eylem tespit edildi (stake kesintisi uygulanacak).
    #[error("guardian kötü niyet tespit edildi: {guardian_id} — sebep: {reason}")]
    GuardianMaliciousBehavior { guardian_id: String, reason: String },

    // ─── Hesap Durumu Hataları ──────────────────────────────────────────────
    /// Hesap dondurulmuş — tüm çıkış işlemleri engellendi.
    #[error("hesap dondurulmuş: {address} — sebep: {reason}")]
    AccountFrozen { address: String, reason: String },

    /// Hesap kurtarma modunda — normal işlem yapılamaz.
    #[error("hesap kurtarma modunda: {address}")]
    AccountInRecovery { address: String },

    /// Hesap miras protokolünde — normal işlem yapılamaz.
    #[error("hesap miras protokolünde: {address}")]
    AccountInInheritance { address: String },

    /// Hesap kapalı / silinmiş — işlem yapılamaz.
    #[error("hesap kapalı: {address}")]
    AccountClosed { address: String },

    /// Hesap zaten aktif, yeniden aktivasyon gereksiz.
    #[error("hesap zaten aktif: {address}")]
    AccountAlreadyActive { address: String },

    // ─── Kurtarma Hataları ──────────────────────────────────────────────────
    /// Kurtarma talebi bulunamadı.
    #[error("kurtarma talebi bulunamadı: {request_id}")]
    RecoveryRequestNotFound { request_id: String },

    /// Kurtarma bekleme süresi dolmadı (§5.1.4: 48 saat).
    #[error("kurtarma bekleme süresi dolmadı: {remaining_seconds} saniye kaldı")]
    RecoveryPeriodNotElapsed { remaining_seconds: u64 },

    /// Kurtarma süresi geçti — talep iptal edildi.
    #[error("kurtarma süresi geçti: talep {request_id} sona erdi")]
    RecoveryExpired { request_id: String },

    /// Kurtarma talebi iptal edildi kullanıcı tarafından.
    #[error("kurtarma talebi kullanıcı tarafından iptal edildi: {request_id}")]
    RecoveryCancelledByUser { request_id: String },

    /// Kurtarma talebi onay threshold'u aşılamadı.
    #[error("kurtarma onayı yetersiz: {approvals} onay var, {required} gerekli")]
    RecoveryInsufficientApprovals { approvals: usize, required: usize },

    /// Yetkisiz kurtarma iptali — sadece hesap sahibi iptal edebilir.
    ///
    /// NOVA-RED-ACCOUNT-002: canceller != hesap sahibi ise reddedilmeli.
    #[error("yetkisiz kurtarma iptali: canceller {canceller} hesap sahibi {owner} değil")]
    UnauthorizedRecoveryCanceller { canceller: String, owner: String },

    // ─── Heartbeat Hataları ─────────────────────────────────────────────────
    /// Heartbeat imzası geçersiz.
    #[error("geçersiz heartbeat imzası: {address}")]
    InvalidHeartbeatSignature { address: String },

    /// Heartbeat çok sık gönderildi (spam koruması).
    #[error("heartbeat çok sık: son gönderimden {seconds_since_last} saniye geçti, minimum {minimum_interval} saniye")]
    HeartbeatTooFrequent {
        seconds_since_last: u64,
        minimum_interval: u64,
    },

    // ─── İmza Hataları ──────────────────────────────────────────────────────
    /// MPC imzası doğrulama başarısız.
    #[error("MPC imzası doğrulama başarısız: {reason}")]
    SignatureVerificationFailed { reason: String },

    /// Geçersiz imzacı — bu taraf bu hesap için imzalama yetkisi yok.
    #[error("yetkisiz imzacı: {signer}")]
    UnauthorizedSigner { signer: String },

    // ─── Yapılandırma Hataları ──────────────────────────────────────────────
    /// Geçersiz hesap yapılandırması.
    #[error("geçersiz hesap yapılandırması: {reason}")]
    InvalidAccountConfig { reason: String },

    /// Inaktivite süresi geçersiz — §5.4.2: min 30 gün.
    #[error("geçersiz inaktivite süresi: {seconds} saniye — minimum {min_seconds} saniye")]
    InvalidInactivityPeriod { seconds: u64, min_seconds: u64 },

    // ─── Depolama Hataları ──────────────────────────────────────────────────
    /// Depolama okuma hatası.
    #[error("depolama okuma hatası: {reason}")]
    StorageReadError { reason: String },

    /// Depolama yazma hatası.
    #[error("depolama yazma hatası: {reason}")]
    StorageWriteError { reason: String },

    /// Serializasyon hatası.
    #[error("serializasyon hatası: {reason}")]
    SerializationError { reason: String },

    // ─── Kriptografi Hataları ────────────────────────────────────────────────
    /// Genel kriptografi hatası.
    #[error("kriptografi hatası: {reason}")]
    CryptoError { reason: String },

    /// Key share eksik veya geçersiz.
    #[error("key share hatası: parti {party_index} — {reason}")]
    KeyShareError { party_index: usize, reason: String },

    // ─── Genel Hatalar ───────────────────────────────────────────────────────
    /// Genel dahili hata.
    #[error("dahili hata: {reason}")]
    InternalError { reason: String },

    /// Özellik henüz desteklenmiyor.
    #[error("desteklenmiyor: {feature}")]
    Unsupported { feature: String },

    /// Geçersiz parametre.
    #[error("geçersiz parametre '{param}': {reason}")]
    InvalidParameter { param: String, reason: String },
}

/// nova-account sonuç tipi.
pub type AccountResult<T> = Result<T, AccountError>;

impl AccountError {
    // ─── Yapıcı Yardımcılar ────────────────────────────────────────────────

    /// Hesap bulunamadı hatası oluştur.
    pub fn not_found(address: impl Into<String>) -> Self {
        Self::AccountNotFound {
            address: address.into(),
        }
    }

    /// Geçersiz threshold hatası oluştur.
    pub fn invalid_threshold(threshold: u8, total: u8) -> Self {
        Self::InvalidThreshold { threshold, total }
    }

    /// Guardian bulunamadı hatası oluştur.
    pub fn guardian_not_found(guardian_id: impl Into<String>) -> Self {
        Self::GuardianNotFound {
            guardian_id: guardian_id.into(),
        }
    }

    /// Hesap dondurulmuş hatası oluştur.
    pub fn frozen(address: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::AccountFrozen {
            address: address.into(),
            reason: reason.into(),
        }
    }

    /// Dahili hata oluştur.
    pub fn internal(reason: impl Into<String>) -> Self {
        Self::InternalError {
            reason: reason.into(),
        }
    }

    /// Kriptografi hatası oluştur.
    pub fn crypto(reason: impl Into<String>) -> Self {
        Self::CryptoError {
            reason: reason.into(),
        }
    }

    /// Serializasyon hatası oluştur.
    pub fn serialization(reason: impl Into<String>) -> Self {
        Self::SerializationError {
            reason: reason.into(),
        }
    }

    // ─── Sınıflandırma ────────────────────────────────────────────────────

    /// Bu hata geri kurtarılabilir mi? (Yeniden deneme mantıklı mı?)
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::GuardianChangePending { .. }
                | Self::RecoveryPeriodNotElapsed { .. }
                | Self::HeartbeatTooFrequent { .. }
                | Self::InsufficientSignatures { .. }
        )
    }

    /// Bu hata kullanıcı kaynaklı mı? (Dışarıya detay bildirilebilir mi?)
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidThreshold { .. }
                | Self::TooManyGuardians { .. }
                | Self::InvalidAccountConfig { .. }
                | Self::InvalidParameter { .. }
                | Self::InvalidInactivityPeriod { .. }
        )
    }

    /// Bu hata güvenlik olayı tetiklemeli mi?
    pub fn is_security_event(&self) -> bool {
        matches!(
            self,
            Self::GuardianMaliciousBehavior { .. }
                | Self::UnauthorizedSigner { .. }
                | Self::SignatureVerificationFailed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hata_mesajlari_dogru_formatlanir() {
        let err = AccountError::not_found("0xabcdef");
        assert!(err.to_string().contains("0xabcdef"));
        assert!(err.to_string().contains("hesap bulunamadı"));
    }

    #[test]
    fn gecersiz_threshold_hata_olusturulur() {
        let err = AccountError::invalid_threshold(4, 3);
        assert!(err.to_string().contains("t=4"));
        assert!(err.to_string().contains("n=3"));
    }

    #[test]
    fn guardian_not_found_hata_olusturulur() {
        let err = AccountError::guardian_not_found("guardian-01");
        assert!(err.to_string().contains("guardian-01"));
    }

    #[test]
    fn frozen_hata_detay_iceriyor() {
        let err = AccountError::frozen("0xabc", "şüpheli işlem");
        assert!(err.to_string().contains("0xabc"));
        assert!(err.to_string().contains("şüpheli işlem"));
    }

    #[test]
    fn hatalar_kurtarilabilirlik_siniflandirilir() {
        let pending = AccountError::GuardianChangePending {
            remaining_seconds: 100,
        };
        assert!(pending.is_recoverable());

        let not_found = AccountError::not_found("x");
        assert!(!not_found.is_recoverable());
    }

    #[test]
    fn guvenlik_olaylari_tespiti_dogru() {
        let malicious = AccountError::GuardianMaliciousBehavior {
            guardian_id: "g1".into(),
            reason: "erken dondurma girişimi".into(),
        };
        assert!(malicious.is_security_event());

        let internal = AccountError::internal("disk hata");
        assert!(!internal.is_security_event());
    }

    #[test]
    fn kullanici_hatalari_siniflandirilir() {
        let threshold_err = AccountError::InvalidThreshold {
            threshold: 5,
            total: 3,
        };
        assert!(threshold_err.is_user_error());

        let frozen_err = AccountError::frozen("0x0", "sebep");
        assert!(!frozen_err.is_user_error());
    }

    #[test]
    fn result_tipi_kullanilabilir() {
        let ok: AccountResult<u32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);

        let err: AccountResult<u32> = Err(AccountError::internal("test"));
        assert!(err.is_err());
    }

    #[test]
    fn too_many_guardians_hata_olusturulur() {
        let err = AccountError::TooManyGuardians { max: 15, given: 20 };
        assert!(err.to_string().contains("15"));
        assert!(err.to_string().contains("20"));
    }

    #[test]
    fn recovery_period_not_elapsed_hata_olusturulur() {
        let err = AccountError::RecoveryPeriodNotElapsed {
            remaining_seconds: 3600,
        };
        assert!(err.to_string().contains("3600"));
        assert!(err.is_recoverable());
    }

    #[test]
    fn insufficient_signatures_hata_olusturulur() {
        let err = AccountError::InsufficientSignatures {
            provided: 2,
            required: 3,
        };
        assert!(err.to_string().contains("2"));
        assert!(err.to_string().contains("3"));
        assert!(err.is_recoverable());
    }

    #[test]
    fn unauthorized_signer_guvenlik_olayidir() {
        let err = AccountError::UnauthorizedSigner {
            signer: "0xmalicious".into(),
        };
        assert!(err.is_security_event());
        assert!(!err.is_user_error());
        assert!(!err.is_recoverable());
    }
}
