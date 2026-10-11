//! nova-inheritance hata tipleri.
//!
//! §5.4 — Advanced Native Inheritance Protocol hata senaryoları.
//!
//! ## Tasarım İlkesi
//!
//! Her hata varyantı, protokolün hangi güvenlik katmanında sorun
//! çıktığını açıkça belirtir. Bu şekilde:
//! - Guardian suistimali ile teknik hata ayrışır
//! - Kullanıcı hatası ile protokol hatası ayrışır
//! - Denetim izleri (audit log) hata tipine göre filtrelenebilir

use nova_primitives::types::SlotId;
use thiserror::Error;

/// Inheritance protokol hataları.
#[derive(Debug, Error)]
pub enum InheritanceError {
    // ─── Plan Hataları ────────────────────────────────────────────────────────

    /// Plan bulunamadı.
    #[error("miras planı bulunamadı: plan_id={}", hex::encode(plan_id))]
    PlanNotFound { plan_id: [u8; 32] },

    /// Plan zaten mevcut.
    #[error("miras planı zaten var: hesap için sadece 1 plan olabilir")]
    PlanAlreadyExists,

    /// Plan aktif değil (iptal edilmiş veya tamamlanmış).
    #[error("miras planı aktif değil: mevcut durum={phase}")]
    PlanNotActive { phase: String },

    /// Plan zaten tamamlanmış.
    #[error("miras planı tamamlandı: devir gerçekleşti")]
    PlanAlreadyCompleted,

    /// Plan iptal edildi.
    #[error("miras planı iptal edildi")]
    PlanCancelled,

    // ─── Tetikleyici Hataları ─────────────────────────────────────────────────

    /// İnaktivite süresi henüz dolmadı.
    #[error("inaktivite süresi dolmadı: kalan={remaining_secs} saniye")]
    InactivityPeriodNotElapsed { remaining_secs: u64 },

    /// Hedef slot henüz gelmedi.
    #[error("hedef slot bekleniyor: hedef={target}, şimdiki={current}")]
    TargetSlotNotReached { target: SlotId, current: SlotId },

    /// Manuel aktivasyon için guardian onayı eksik.
    #[error("manuel aktivasyon: yetersiz guardian onayı ({got}/{required})")]
    InsufficientGuardianApproval { got: u8, required: u8 },

    // ─── Varis Hataları ───────────────────────────────────────────────────────

    /// Varis listesi boş.
    #[error("varis listesi boş: en az 1 varis gerekli")]
    EmptyHeirList,

    /// Varis pay toplamı %100 değil.
    #[error("varis pay toplamı hatalı: toplam={total_bps} bps (10000 olmalı)")]
    InvalidShareTotal { total_bps: u32 },

    /// Varis bulunamadı.
    #[error("varis bulunamadı: heir_id={}", hex::encode(heir_id))]
    HeirNotFound { heir_id: [u8; 20] },

    /// Varis zaten listede.
    #[error("varis zaten eklenmiş: {}", hex::encode(heir_address))]
    HeirAlreadyExists { heir_address: [u8; 20] },

    /// Fazla varis (maksimum 15).
    #[error("varis sayısı sınırı aşıldı: maks=15, mevcut={count}")]
    TooManyHeirs { count: usize },

    // ─── Aşama (Phase) Hataları ───────────────────────────────────────────────

    /// Yanlış aşamada işlem.
    #[error("geçersiz aşama: beklenen={expected} mevcut={current}")]
    WrongPhase { expected: String, current: String },

    /// Aşama geçişi henüz mümkün değil.
    #[error("erken aşama geçişi: slot={slot} için hazır değil")]
    PrematurePhaseTransition { slot: SlotId },

    // ─── Heartbeat Hataları ───────────────────────────────────────────────────

    /// Heartbeat imzası geçersiz.
    #[error("heartbeat imzası geçersiz")]
    InvalidHeartbeatSignature,

    /// Heartbeat zaten kayıtlı bu slot için.
    #[error("bu slot için heartbeat zaten gönderildi: slot={slot}")]
    DuplicateHeartbeat { slot: SlotId },

    // ─── Grace Period Hataları ────────────────────────────────────────────────

    /// Grace period süresi dolmadı, devir başlatılamaz.
    #[error("grace period süresi dolmadı: kalan={remaining_slots} slot")]
    GracePeriodNotElapsed { remaining_slots: u64 },

    /// Grace period uzatma limiti aşıldı (maks 3 kez guardian uzatabilir).
    #[error("grace period uzatma limiti aşıldı: maks 3 kez")]
    GracePeriodExtensionLimitExceeded,

    // ─── Kademeli Devir Hataları ──────────────────────────────────────────────

    /// Kademeli devir adımı henüz zamanlanmadı.
    #[error("kademeli devir adımı zamanlanmadı: adım={step} için slot={required_slot}")]
    ReleaseStepNotDue { step: u32, required_slot: SlotId },

    /// Kademeli devir adımı zaten çalıştırıldı.
    #[error("kademeli devir adımı zaten çalıştırıldı: adım={step}")]
    ReleaseStepAlreadyExecuted { step: u32 },

    /// Kademeli devir adımı eksik (önceki adım tamamlanmadı).
    #[error("önceki kademeli devir adımı tamamlanmadı: adım={step}")]
    PreviousReleaseStepPending { step: u32 },

    // ─── Koşullu Kural Hataları ───────────────────────────────────────────────

    /// Yaş koşulu sağlanmadı.
    #[error("yaş koşulu karşılanmadı: hesap yaşı={account_age_slots} slot")]
    AgeConditionNotMet { account_age_slots: u64 },

    /// Borç ödeme koşulu tamamlanmadı.
    #[error("borç ödeme gerekiyor: {debt_amount} NSC → {}", hex::encode(creditor))]
    DebtPaymentRequired { debt_amount: u128, creditor: [u8; 20] },

    /// Çoklu imza koşulu karşılanmadı.
    #[error("çoklu imza koşulu: {got}/{required} varis onayladı")]
    MultiSigConditionNotMet { got: u8, required: u8 },

    // ─── Guardian Suistimal Koruması ──────────────────────────────────────────

    /// Guardian'lar zaman kilidini atlayamaz.
    #[error("guardian yetkisi yok: inaktivite süresi dolmadan devir başlatılamaz")]
    GuardianCannotOverrideTrigger,

    /// Guardian değişikliği bekleme penceresi aktif.
    #[error("guardian değişikliği bekleme: {remaining_slots} slot kaldı")]
    GuardianChangePending { remaining_slots: u64 },

    // ─── Genel Hatalar ────────────────────────────────────────────────────────

    /// Geçersiz parametre.
    #[error("geçersiz parametre: {0}")]
    InvalidParameter(String),

    /// Serializasyon hatası.
    #[error("serializasyon hatası: {0}")]
    SerializationError(String),

    /// İç hata (bug göstergesi).
    #[error("iç hata: {0}")]
    InternalError(String),

    /// NOVA-RED-INHERIT-002/004: Yetkisiz erişim
    #[error("yetkisiz erişim: bu işlem için yetki yok")]
    Unauthorized,
}

/// Inheritance sonuç tipi.
pub type InheritanceResult<T> = Result<T, InheritanceError>;
