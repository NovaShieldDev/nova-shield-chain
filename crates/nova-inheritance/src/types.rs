//! nova-inheritance temel veri tipleri.
//!
//! §5.4 — Advanced Native Inheritance Protocol veri yapıları.
//!
//! ## Tasarım İlhamı
//!
//! - **Polkadot Scheduler pallet**: Zaman kilitli işlemler, ertelenmiş çalıştırma
//! - **Cosmos SDK x/authz**: Yetki devri ve koşullu işlemler
//! - **Avalanchego multisig**: Çok imzalı onay mekanizmaları
//! - **Nova Whitepaper §5.4**: Tam protokol spesifikasyonu

use borsh::{BorshDeserialize, BorshSerialize};
use nova_primitives::types::{Address, Balance, Hash32, SlotId};
use serde::{Deserialize, Serialize};

// ─── PLAN ID ─────────────────────────────────────────────────────────────────

/// Miras planı kimliği.
///
/// Her hesabın en fazla 1 aktif planı olabilir.
/// Plan ID = BLAKE3("nova-inheritance-plan-v1" || account_id_bytes || created_at_slot)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct InheritancePlanId(pub [u8; 32]);

impl InheritancePlanId {
    /// Ham byte'lardan oluştur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// BLAKE3 türetme: account_id + slot.
    pub fn derive(account_id: u64, created_at_slot: SlotId) -> Self {
        let mut input = Vec::with_capacity(24);
        input.extend_from_slice(b"nova-inheritance-plan-v1");
        input.extend_from_slice(&account_id.to_le_bytes());
        input.extend_from_slice(&created_at_slot.0.to_le_bytes());
        let hash = blake3::hash(&input);
        Self(*hash.as_bytes())
    }

    /// Hex gösterimi (kısa).
    pub fn short_hex(&self) -> String {
        hex::encode(&self.0[..6])
    }
}

impl std::fmt::Display for InheritancePlanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Plan({}...)", self.short_hex())
    }
}

// ─── TETİKLEYİCİ TİPİ (§5.4.2) ──────────────────────────────────────────────

/// Miras protokolü tetikleyici tipi.
///
/// Whitepaper §5.4.2: "Üç farklı tetikleyiciden biri aktif olduğunda başlar."
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum TriggerType {
    /// Tetikleyici 1: Zaman bazlı inaktivite.
    ///
    /// Kullanıcı belirlenen süre boyunca (işlem/heartbeat olmadan) inaktif kalırsa.
    /// Minimum: 30 gün = 2_592_000 saniye
    Inactivity {
        /// Kaç saniye inaktivite → tetikleyici aktif.
        period_secs: u64,
    },

    /// Tetikleyici 2: Belirli bir slot (tarih/blok bazlı).
    ///
    /// O slot'a ulaşıldıktan sonra inaktivite sayacı başlar.
    /// Örn: "çocuğum 18 yaşına girdiğinde".
    FixedSlot {
        /// Bu slot'tan sonra inaktivite sayacı aktif.
        activation_slot: SlotId,
        /// Aktivasyon sonrası inaktivite süresi.
        inactivity_period_secs: u64,
    },

    /// Tetikleyici 3: Manuel aktivasyon.
    ///
    /// Kullanıcı bilinçli olarak başlatır VEYA
    /// 2 guardian birlikte onaylar (kullanıcı erişimi yoksa).
    ManualActivation {
        /// Guardian onayı gerektiriyor mu?
        requires_guardian_approval: bool,
        /// Gerekli guardian onay sayısı.
        guardian_threshold: u8,
    },
}

impl TriggerType {
    /// Minimum inaktivite süresi (30 gün).
    pub const MIN_INACTIVITY_SECS: u64 = 30 * 24 * 3600; // 2_592_000

    /// Tetikleyici tipi adı.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Inactivity { .. } => "Inactivity",
            Self::FixedSlot { .. } => "FixedSlot",
            Self::ManualActivation { .. } => "ManualActivation",
        }
    }

    /// İnaktivite süresi (saniye cinsinden).
    pub fn inactivity_period_secs(&self) -> Option<u64> {
        match self {
            Self::Inactivity { period_secs } => Some(*period_secs),
            Self::FixedSlot { inactivity_period_secs, .. } => Some(*inactivity_period_secs),
            Self::ManualActivation { .. } => None,
        }
    }

    /// Geçerli mi? (minimum süre kontrolü)
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Inactivity { period_secs } => *period_secs >= Self::MIN_INACTIVITY_SECS,
            Self::FixedSlot { inactivity_period_secs, .. } => {
                *inactivity_period_secs >= Self::MIN_INACTIVITY_SECS
            }
            Self::ManualActivation { guardian_threshold, .. } => *guardian_threshold >= 1,
        }
    }
}

// ─── MİRAS AŞAMASI (§5.4.3) ─────────────────────────────────────────────────

/// Miras protokolünün mevcut aşaması.
///
/// Whitepaper §5.4.3: "Dört aşamalı güvenlik süreci"
///
/// ```text
/// Dormant → PreWarning → FinalWarning → GracePeriod → StagedRelease → Completed
///           ↑                                    ↓
///           └─────────────── Cancelled ──────────┘
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum InheritancePhase {
    /// Plan aktif ama tetikleyici henüz devrede değil.
    Dormant,

    /// Aşama 1: Ön uyarı — tetikleyici süresi dolmadan 30 gün önce.
    ///
    /// §5.4.3: "Guardian'lara zincir üstü bildirim gönderilir."
    /// Kullanıcı tek heartbeat ile sıfırlar.
    PreWarning {
        /// Ön uyarı başladığı slot.
        started_at_slot: SlotId,
        /// Ön uyarının sona ereceği slot.
        ends_at_slot: SlotId,
    },

    /// Aşama 2: Son uyarı — tetikleyici süresi dolmadan 7 gün önce.
    ///
    /// §5.4.3: "Tüm guardian'lara ve varise(lere) ikinci bildirim."
    FinalWarning {
        /// Son uyarı başladığı slot.
        started_at_slot: SlotId,
        /// Son uyarının sona ereceği slot.
        ends_at_slot: SlotId,
    },

    /// Aşama 3: Grace period — tetikleyici süresi doldu, devir BAŞLAMADI.
    ///
    /// §5.4.3: "Kullanıcı tek imzayla iptal edebilir. Varisler erişemez."
    GracePeriod {
        /// Grace period başladığı slot.
        started_at_slot: SlotId,
        /// Grace period'un biteceği slot.
        ends_at_slot: SlotId,
        /// Guardian tarafından kaç kez uzatıldı (maks 3).
        extension_count: u8,
    },

    /// Aşama 4: Kademeli devir başladı.
    ///
    /// §5.4.6: "Kademeli devir planına göre varlıklar aktarılır."
    StagedRelease {
        /// Devir başladığı slot.
        started_at_slot: SlotId,
        /// Tamamlanan adım sayısı.
        completed_steps: u32,
        /// Toplam adım sayısı.
        total_steps: u32,
    },

    /// Devir tamamlandı.
    Completed {
        completed_at_slot: SlotId,
    },

    /// İptal edildi (kullanıcı heartbeat gönderdi veya manuel iptal).
    Cancelled {
        cancelled_at_slot: SlotId,
        reason: CancellationReason,
    },
}

impl InheritancePhase {
    /// Aşama adı (display için).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Dormant => "Dormant",
            Self::PreWarning { .. } => "PreWarning",
            Self::FinalWarning { .. } => "FinalWarning",
            Self::GracePeriod { .. } => "GracePeriod",
            Self::StagedRelease { .. } => "StagedRelease",
            Self::Completed { .. } => "Completed",
            Self::Cancelled { .. } => "Cancelled",
        }
    }

    /// Aktif aşama mı? (Dormant değil, Cancelled değil, Completed değil)
    pub fn is_active_warning(&self) -> bool {
        matches!(self, Self::PreWarning { .. } | Self::FinalWarning { .. } | Self::GracePeriod { .. })
    }

    /// Kullanıcı hâlâ iptal edebilir mi?
    pub fn is_cancellable(&self) -> bool {
        matches!(self,
            Self::Dormant
            | Self::PreWarning { .. }
            | Self::FinalWarning { .. }
            | Self::GracePeriod { .. }
        )
    }

    /// Devir aşamasında mı?
    pub fn is_releasing(&self) -> bool {
        matches!(self, Self::StagedRelease { .. })
    }

    /// Sonlandı mı? (Completed veya Cancelled)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Cancelled { .. })
    }
}

impl std::fmt::Display for InheritancePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// İptal sebebi.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum CancellationReason {
    /// Kullanıcı heartbeat gönderdi → otomatik iptal.
    HeartbeatReceived,
    /// Kullanıcı manuel olarak iptal etti.
    ManualCancellation,
    /// Plan güncellendi (yeni plan ile değiştirildi).
    PlanUpdated,
}

// ─── VARİS KATI (§5.4.5) ─────────────────────────────────────────────────────

/// Varis öncelik katmanı.
///
/// Whitepaper §5.4.5: "Hiyerarşik Varis Sistemi"
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum HeirTier {
    /// 1. Öncelik — birincil varisler.
    Primary = 1,
    /// 2. Öncelik — birincil varisler yanıt vermezse.
    Secondary = 2,
    /// 3. Öncelik — son çare.
    Tertiary = 3,
    /// Hiç varis yanıt vermezse → yakma/hazine.
    TreasuryBurn = 4,
}

impl HeirTier {
    /// Bir sonraki katmana geç.
    pub fn next(&self) -> Option<Self> {
        match self {
            Self::Primary => Some(Self::Secondary),
            Self::Secondary => Some(Self::Tertiary),
            Self::Tertiary => Some(Self::TreasuryBurn),
            Self::TreasuryBurn => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Tertiary => "Tertiary",
            Self::TreasuryBurn => "TreasuryBurn",
        }
    }
}

// ─── VARİS KAYDI ─────────────────────────────────────────────────────────────

/// Tek bir varis kaydı.
///
/// Her varis için: adres, kat, pay yüzdesi, onay süresi.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HeirRecord {
    /// Varis Nova adresi.
    pub address: Address,
    /// Öncelik katmanı.
    pub tier: HeirTier,
    /// Bu varis'e düşen pay (basis points: 10000 = %100).
    /// Aynı kattaki tüm varisler için toplam = 10000.
    pub share_bps: u32,
    /// Bu varis'in onay vermesi için ne kadar slot beklenecek.
    /// (Yanıt vermezse → bir sonraki kata geçilir)
    pub onboarding_timeout_slots: u64,
    /// Ek not (isteğe bağlı, hash olarak saklanır).
    pub note_hash: Option<Hash32>,
}

impl HeirRecord {
    /// Yeni varis kaydı oluştur.
    pub fn new(
        address: Address,
        tier: HeirTier,
        share_bps: u32,
        onboarding_timeout_slots: u64,
    ) -> Self {
        Self {
            address,
            tier,
            share_bps,
            onboarding_timeout_slots,
            note_hash: None,
        }
    }

    /// Bu varise düşen gerçek pay (total_balance üzerinden).
    pub fn allocated_amount(&self, total_balance: u128) -> u128 {
        total_balance
            .saturating_mul(self.share_bps as u128)
            .saturating_div(10_000)
    }
}

// ─── KADEMELİ DEVİR ADIMI (§5.4.6) ─────────────────────────────────────────

/// Kademeli devir planındaki tek bir adım.
///
/// §5.4.6: "Varlıkların tamamı tek seferde devredilmez."
///
/// Örnek:
/// - Adım 1: Anında %25
/// - Adım 2: 30 gün sonra %25 daha
/// - Adım 3: 6 ay sonra %30
/// - Adım 4: 5 yıl sonra %20
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct StagedReleaseStep {
    /// Bu adımın sırası (0-bazlı).
    pub step_index: u32,
    /// Devir tetiklendikten kaç slot sonra bu adım çalışır.
    /// 0 = anında.
    pub delay_slots: u64,
    /// Bu adımda toplam varlığın ne kadarı aktarılır (basis points).
    /// Tüm adımların fraction_bps toplamı = 10000 olmalı.
    pub fraction_bps: u32,
    /// Bu adımın alıcısı (varis adresi).
    pub recipient: Address,
    /// Bu adım çalıştırıldı mı?
    pub executed: bool,
    /// Çalıştırıldıysa hangi slot'ta.
    pub executed_at_slot: Option<SlotId>,
    /// Aktarılan gerçek miktar.
    pub transferred_amount: Option<u128>,
}

impl StagedReleaseStep {
    /// Yeni adım oluştur.
    pub fn new(step_index: u32, delay_slots: u64, fraction_bps: u32, recipient: Address) -> Self {
        Self {
            step_index,
            delay_slots,
            fraction_bps,
            recipient,
            executed: false,
            executed_at_slot: None,
            transferred_amount: None,
        }
    }

    /// Bu adım için gerçek transfer miktarı.
    pub fn amount_for(&self, total_balance: u128) -> u128 {
        total_balance
            .saturating_mul(self.fraction_bps as u128)
            .saturating_div(10_000)
    }

    /// Bu adım hazır mı? (release_slot + delay_slots <= current_slot)
    pub fn is_due(&self, release_started_at: SlotId, current_slot: SlotId) -> bool {
        let due_slot = release_started_at.0.saturating_add(self.delay_slots);
        current_slot.0 >= due_slot
    }
}

// ─── KOŞULLU KURAL (§5.4.7) ──────────────────────────────────────────────────

/// Varlık devirine bağlı koşullar.
///
/// §5.4.7: "Kullanıcı varlık devrine önceden şartlar bağlayabilir."
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum ConditionalRule {
    /// Yaş koşulu: Varis hesabı yeterince eski olmalı.
    ///
    /// §5.4.7: "Hesap yaşı blok timestamp'ini temel alır."
    AgeCondition {
        /// Hangi varise uygulanır.
        heir_address: Address,
        /// Minimum hesap yaşı (slot cinsinden).
        min_account_age_slots: u64,
    },

    /// Sıralı borç ödeme.
    ///
    /// §5.4.7: "Önce [Adres X]'e [miktar] gönder, sonra varisler."
    DebtPayment {
        /// Alacaklı adresi.
        creditor: Address,
        /// Ödenecek miktar (NSC).
        amount: u128,
        /// Ödeme önceliği (1=ilk).
        priority: u8,
    },

    /// Hayır kurumu payı.
    ///
    /// §5.4.7: "Toplam varlığın %5'i → hayır kurumu adresi."
    CharityAllocation {
        /// Hayır kurumu adresi.
        charity_address: Address,
        /// Pay (basis points, maks 5000 = %50).
        share_bps: u32,
    },

    /// Çoklu imza koşulu.
    ///
    /// §5.4.7: "Devir için Varis A VE Varis B birlikte onay vermeli."
    MultiSigRequired {
        /// Kaç varis onaylamalı.
        required_signatures: u8,
        /// Toplam varis sayısı.
        total_heirs: u8,
        /// Ret durumunda yeniden oylama (slot cinsinden gecikme).
        retry_delay_slots: u64,
    },
}

impl ConditionalRule {
    /// Kural tipi adı.
    pub fn rule_type(&self) -> &'static str {
        match self {
            Self::AgeCondition { .. } => "AgeCondition",
            Self::DebtPayment { .. } => "DebtPayment",
            Self::CharityAllocation { .. } => "CharityAllocation",
            Self::MultiSigRequired { .. } => "MultiSigRequired",
        }
    }

    /// Kural geçerli mi?
    pub fn is_valid(&self) -> bool {
        match self {
            Self::AgeCondition { min_account_age_slots, .. } => *min_account_age_slots > 0,
            Self::DebtPayment { amount, .. } => *amount > 0,
            Self::CharityAllocation { share_bps, .. } => *share_bps > 0 && *share_bps <= 5000,
            Self::MultiSigRequired { required_signatures, total_heirs, .. } => {
                *required_signatures >= 2 && *required_signatures <= *total_heirs
            }
        }
    }
}

// ─── MİRAS PLANI ─────────────────────────────────────────────────────────────

/// Tam miras planı — kullanıcının konfigürasyonu.
///
/// §5.4: "Her kullanıcının kendi koşullarını, varislerini, tetikleyicilerini
/// ve devir kurallarını serbestçe programlayabildiği kişisel miras protokolü."
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InheritancePlan {
    /// Plan kimliği.
    pub plan_id: InheritancePlanId,
    /// Bu planın sahibi hesap (u64 account_id).
    pub owner_account_id: u64,
    /// Hangi adresin varlıkları bu planla yönetilir.
    pub owner_address: Address,
    /// Tetikleyici tipi (§5.4.2).
    pub trigger: TriggerType,
    /// Grace period uzunluğu (slot cinsinden).
    /// §5.4.3: "7 gün ile 365 gün arası."
    pub grace_period_slots: u64,
    /// Varisler (hiyerarşik, kat sıralı).
    pub heirs: Vec<HeirRecord>,
    /// Kademeli devir adımları (§5.4.6).
    pub release_steps: Vec<StagedReleaseStep>,
    /// Koşullu kurallar (§5.4.7).
    pub conditions: Vec<ConditionalRule>,
    /// Plan oluşturulduğu slot.
    pub created_at_slot: SlotId,
    /// Son güncelleme slot'u.
    pub updated_at_slot: SlotId,
    /// Plan aktif mi?
    pub is_active: bool,
}

impl InheritancePlan {
    /// Plan geçerli mi? (temel doğrulama)
    pub fn validate(&self) -> Result<(), crate::errors::InheritanceError> {
        use crate::errors::InheritanceError;

        // Tetikleyici geçerliliği
        if !self.trigger.is_valid() {
            return Err(InheritanceError::InvalidParameter(
                format!("geçersiz tetikleyici: {:?}", self.trigger.name()),
            ));
        }

        // Grace period (7 gün minimum = 151_200 slot @ 400ms)
        let min_grace = 7 * 24 * 3600 * 1000 / 400; // slot sayısına çevir
        if self.grace_period_slots < min_grace {
            return Err(InheritanceError::InvalidParameter(
                format!("grace period çok kısa: {} slot (min: {})", self.grace_period_slots, min_grace),
            ));
        }

        // Varis listesi boş değil mi?
        if self.heirs.is_empty() {
            return Err(InheritanceError::EmptyHeirList);
        }

        // Maks varis sayısı
        if self.heirs.len() > 15 {
            return Err(InheritanceError::TooManyHeirs { count: self.heirs.len() });
        }

        // Her kattaki pay toplamı = 10000
        self.validate_heir_shares()?;

        // Kademeli devir adımları toplamı = 10000 (adım varsa)
        // NOVA-RED-INHERITANCE-002: checked_add ile u32 overflow tespiti.
        if !self.release_steps.is_empty() {
            let total: u32 = self.release_steps.iter().try_fold(0u32, |acc, s| {
                acc.checked_add(s.fraction_bps)
            }).ok_or_else(|| InheritanceError::InvalidParameter(
                "kademeli devir adımları fraction_bps toplamı u32 taşması".to_string(),
            ))?;
            if total != 10_000 {
                return Err(InheritanceError::InvalidParameter(
                    format!("kademeli devir adımları toplamı {total} bps (10000 olmalı)"),
                ));
            }
        }

        // Koşullar geçerli mi?
        for rule in &self.conditions {
            if !rule.is_valid() {
                return Err(InheritanceError::InvalidParameter(
                    format!("geçersiz koşul: {}", rule.rule_type()),
                ));
            }
        }

        Ok(())
    }

    /// Her kattaki varis paylarını doğrula.
    fn validate_heir_shares(&self) -> Result<(), crate::errors::InheritanceError> {
        use crate::errors::InheritanceError;
        use std::collections::HashMap;

        let mut tier_totals: HashMap<u8, u32> = HashMap::new();
        for heir in &self.heirs {
            let tier_num = heir.tier as u8;
            *tier_totals.entry(tier_num).or_insert(0) += heir.share_bps;
        }

        for (_, total) in &tier_totals {
            if *total != 10_000 {
                return Err(InheritanceError::InvalidShareTotal { total_bps: *total });
            }
        }

        Ok(())
    }

    /// Belirli kattaki varisler.
    pub fn heirs_at_tier(&self, tier: HeirTier) -> Vec<&HeirRecord> {
        self.heirs.iter().filter(|h| h.tier == tier).collect()
    }

    /// Birincil varisler.
    pub fn primary_heirs(&self) -> Vec<&HeirRecord> {
        self.heirs_at_tier(HeirTier::Primary)
    }

    /// İkincil varisler.
    pub fn secondary_heirs(&self) -> Vec<&HeirRecord> {
        self.heirs_at_tier(HeirTier::Secondary)
    }

    /// Üçüncül varisler.
    pub fn tertiary_heirs(&self) -> Vec<&HeirRecord> {
        self.heirs_at_tier(HeirTier::Tertiary)
    }
}

// ─── MIRAS DURUMU ────────────────────────────────────────────────────────────

/// Çalışma zamanı miras protokol durumu.
///
/// Bir planın anlık çalışma durumunu temsil eder.
/// `InheritancePlan` kullanıcının konfigürasyonu, `InheritanceState` ise
/// protokolün o anki işlem durumudur.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InheritanceState {
    /// Hangi plan.
    pub plan_id: InheritancePlanId,
    /// Mevcut aşama.
    pub phase: InheritancePhase,
    /// Son aktivite slot'u (heartbeat veya TX).
    pub last_activity_slot: SlotId,
    /// Son heartbeat slot'u.
    pub last_heartbeat_slot: SlotId,
    /// Toplam heartbeat sayısı.
    pub heartbeat_count: u64,
    /// Duress heartbeat alındı mı? (gizli alarm)
    pub duress_detected: bool,
    /// Guardian tarafından onaylanan uzatmalar (maks 3).
    pub guardian_extension_count: u8,
    /// Manuel aktivasyon onayları (guardian_id listesi).
    pub manual_activation_approvals: Vec<u64>,
    /// Varis onayları (heir_address → onay_slot).
    pub heir_acknowledgements: Vec<(Address, SlotId)>,
    /// Multi-sig koşulu için varis onayları.
    pub multisig_approvals: Vec<Address>,
    /// Mevcut devir slot'u (StagedRelease başladığında).
    pub release_started_at: Option<SlotId>,
    /// Toplam devredilen miktar.
    pub total_transferred: Balance,
    /// Tetiklenme anındaki kilitli miras değeri (NSC).
    /// `collect_pending_transfers` bu değeri kullanarak adım miktarlarını hesaplar.
    /// 0 = henüz ayarlanmadı (tetiklenme öncesi veya bilinmiyor).
    pub locked_estate: u128,
    /// Son güncelleme slot'u.
    pub updated_at_slot: SlotId,
}

impl InheritanceState {
    /// Yeni state oluştur (Dormant ile başla).
    pub fn new(plan_id: InheritancePlanId, created_at_slot: SlotId) -> Self {
        Self {
            plan_id,
            phase: InheritancePhase::Dormant,
            last_activity_slot: created_at_slot,
            last_heartbeat_slot: created_at_slot,
            heartbeat_count: 0,
            duress_detected: false,
            guardian_extension_count: 0,
            manual_activation_approvals: Vec::new(),
            heir_acknowledgements: Vec::new(),
            multisig_approvals: Vec::new(),
            release_started_at: None,
            total_transferred: Balance(0),
            locked_estate: 0,
            updated_at_slot: created_at_slot,
        }
    }

    /// Son aktiviteden geçen saniye sayısı (slot bazlı, 400ms/slot).
    pub fn inactivity_secs(&self, current_slot: SlotId) -> u64 {
        let elapsed_slots = current_slot.0.saturating_sub(self.last_activity_slot.0);
        elapsed_slots.saturating_mul(400) / 1000 // 400ms/slot → saniye
    }

    /// Heartbeat kaydı.
    pub fn record_heartbeat(&mut self, slot: SlotId, is_duress: bool) {
        self.last_heartbeat_slot = slot;
        self.last_activity_slot = slot;
        self.heartbeat_count = self.heartbeat_count.saturating_add(1);
        if is_duress {
            self.duress_detected = true;
        }
        self.updated_at_slot = slot;
    }

    /// Aktivite kaydı (heartbeat olmayan TX).
    pub fn record_activity(&mut self, slot: SlotId) {
        self.last_activity_slot = slot;
        self.updated_at_slot = slot;
    }

    /// Varis onayı kaydı.
    pub fn record_heir_acknowledgement(&mut self, address: Address, slot: SlotId) {
        // Aynı varis tekrar eklenmez
        if !self.heir_acknowledgements.iter().any(|(a, _)| *a == address) {
            self.heir_acknowledgements.push((address, slot));
        }
        self.updated_at_slot = slot;
    }

    /// Multi-sig onayı kaydı.
    pub fn record_multisig_approval(&mut self, address: Address, slot: SlotId) {
        if !self.multisig_approvals.contains(&address) {
            self.multisig_approvals.push(address);
        }
        self.updated_at_slot = slot;
    }

    /// Manuel aktivasyon onayı kaydı.
    pub fn record_manual_approval(&mut self, guardian_id: u64, slot: SlotId) {
        if !self.manual_activation_approvals.contains(&guardian_id) {
            self.manual_activation_approvals.push(guardian_id);
        }
        self.updated_at_slot = slot;
    }

    /// Guardian uzatma kaydı.
    pub fn record_guardian_extension(&mut self, new_end_slot: SlotId, slot: SlotId) {
        self.guardian_extension_count = self.guardian_extension_count.saturating_add(1);
        if let InheritancePhase::GracePeriod { started_at_slot, extension_count, .. } = &self.phase {
            let started = *started_at_slot;
            let ext_count = self.guardian_extension_count;
            self.phase = InheritancePhase::GracePeriod {
                started_at_slot: started,
                ends_at_slot: new_end_slot,
                extension_count: ext_count,
            };
        }
        self.updated_at_slot = slot;
    }
}

// ─── HEARTBEAT MESAJI (§5.4.4) ───────────────────────────────────────────────

/// Heartbeat tipi — hayatta olduğunu kanıtlama yöntemi.
///
/// §5.4.4: "Kullanıcı hayatta olduğunu kanıtlamak için birden fazla yöntem."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum HeartbeatKind {
    /// Yöntem 1: Normal zincir üstü imza.
    Normal,
    /// Yöntem 2: Duress kodu (baskı koruması) — gizli alarm.
    ///
    /// §5.4.4: "Dışarıdan normal heartbeat gibi görünür.
    ///         İçeride farklı flag. Guardian'lara gizli alarm."
    Duress,
    /// Yöntem 3: Guardian'ın "kullanıcı hayatta" kararı.
    GuardianExtension {
        /// Onay veren guardian sayısı.
        approver_count: u8,
    },
}

/// Heartbeat mesajı — Polkadot Im-Online pallet benzeri.
///
/// §5.4.4 + nova-account HeartbeatRecord'dan ilham alındı.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HeartbeatMsg {
    /// Hangi hesap.
    pub account_id: u64,
    /// Hangi plan için.
    pub plan_id: InheritancePlanId,
    /// Gönderildiği slot.
    pub slot: SlotId,
    /// Heartbeat tipi.
    pub kind: HeartbeatKind,
    /// MPC eşik imzası (threshold imzası — gerçek sahibinden).
    pub mpc_signature: Vec<u8>,
    /// Heartbeat hash'i (doğrulama için).
    pub heartbeat_hash: Hash32,
}

impl HeartbeatMsg {
    /// Heartbeat hash'ini hesapla.
    pub fn compute_hash(account_id: u64, plan_id: &InheritancePlanId, slot: SlotId, kind_byte: u8) -> Hash32 {
        let mut input = Vec::with_capacity(64);
        input.extend_from_slice(b"nova-inheritance-heartbeat-v1");
        input.extend_from_slice(&account_id.to_le_bytes());
        input.extend_from_slice(&plan_id.0);
        input.extend_from_slice(&slot.0.to_le_bytes());
        input.push(kind_byte);
        let hash = blake3::hash(&input);
        Hash32::from_bytes(*hash.as_bytes())
    }

    /// Duress heartbeat mı?
    pub fn is_duress(&self) -> bool {
        matches!(self.kind, HeartbeatKind::Duress)
    }
}

// ─── DEVİR KOMUTU ────────────────────────────────────────────────────────────

/// Bir varlık devir komutu — çalıştırma katmanına gönderilir.
///
/// `nova-runtime` bu komutu alıp gerçek bakiye transferini yapar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferCommand {
    /// Hangi plandan.
    pub plan_id: InheritancePlanId,
    /// Hangi adımdan (StagedRelease adım indeksi).
    pub step_index: u32,
    /// Gönderen (orijinal hesap).
    pub from: Address,
    /// Alıcı (varis).
    pub to: Address,
    /// Transfer miktarı (NSC).
    pub amount: u128,
    /// Bu transfer hangi slot'ta çalıştırılacak.
    pub execute_at_slot: SlotId,
    /// Onay gerektiriyor mu? (MultiSig koşulu).
    pub requires_multisig: bool,
}

// ─── PLAN ÖZETI ──────────────────────────────────────────────────────────────

/// Miras planı anlık özet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InheritanceSummary {
    /// Plan ID.
    pub plan_id: InheritancePlanId,
    /// Sahip hesap.
    pub owner_account_id: u64,
    /// Mevcut aşama.
    pub phase: String,
    /// Tetikleyici tipi.
    pub trigger_type: String,
    /// Son aktivite slot'u.
    pub last_activity_slot: SlotId,
    /// Inaktivite saniyesi.
    pub inactivity_secs: u64,
    /// Varis sayısı (katlara göre).
    pub primary_heir_count: usize,
    pub secondary_heir_count: usize,
    pub tertiary_heir_count: usize,
    /// Heartbeat sayısı.
    pub heartbeat_count: u64,
    /// Duress tespiti var mı?
    pub duress_detected: bool,
    /// Toplam devredilen miktar.
    pub total_transferred: u128,
}

// ─── SABITLER ─────────────────────────────────────────────────────────────────

/// Ön uyarı süresi (tetikleyiciden kaç slot önce).
/// 30 gün = 30 * 24 * 3600 / 0.4 = 6_480_000 slot
pub const PRE_WARNING_SLOTS: u64 = 6_480_000;

/// Son uyarı süresi (tetikleyiciden kaç slot önce).
/// 7 gün = 7 * 24 * 3600 / 0.4 = 1_512_000 slot
pub const FINAL_WARNING_SLOTS: u64 = 1_512_000;

/// Grace period minimum süresi (7 gün slot).
pub const MIN_GRACE_PERIOD_SLOTS: u64 = 1_512_000;

/// Grace period maksimum süresi (365 gün slot).
pub const MAX_GRACE_PERIOD_SLOTS: u64 = 78_840_000;

/// Guardian uzatma limiti.
pub const MAX_GUARDIAN_EXTENSIONS: u8 = 3;

/// Maksimum varis sayısı.
pub const MAX_HEIRS: usize = 15;

/// Maksimum koşullu kural sayısı.
pub const MAX_CONDITIONS: usize = 10;

/// Maksimum kademeli devir adımı.
pub const MAX_RELEASE_STEPS: usize = 20;

/// Slot süresi (milisaniye).
pub const SLOT_DURATION_MS: u64 = 400;
