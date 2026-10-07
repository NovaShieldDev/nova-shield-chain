//! nova-account — Temel Veri Yapıları
//!
//! Whitepaper §5.1 — MPC-Native Hesap Modeli
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **Cosmos-SDK** (`x/auth/types/account.go`):
//!   BaseAccount {Address, PubKey, AccountNumber, Sequence} deseninden ilham alındı.
//!   Nova Shield'de Sequence → `nonce`, AccountNumber → `account_id`.
//!
//! **TSS-Lib** (`tss/party_id.go`):
//!   PartyID {id, moniker, key, index} yapısından `GuardianPartyId` türetildi.
//!   SortPartyIDs → `GuardianSet::sorted()` metodu.
//!   ReSharingParameters → `GuardianSetChange {old, new, threshold}`.
//!
//! **Multi-party ECDSA GG20** (`src/protocols/multi_party_ecdsa/gg_2020/party_i.rs`):
//!   Parameters {threshold, share_count} → `ThresholdConfig`.
//!   Keys {u_i, y_i, ek, dk, party_index} → `KeyShareInfo`.
//!   SharedKeys {y, x_i} → ortak public key + private share ayrımı.
//!
//! **Penumbra** (`crates/core/keys`):
//!   SpendKey → FullViewingKey → IncomingViewingKey → Address hiyerarşisi.
//!   Diversifier → tek key'den sonsuz adres üretimi (nova'da gizlilik seçeneği).
//!
//! **Polkadot-SDK** (staking pallet):
//!   ValidatorPrefs {commission, blocked} → `GuardianPreferences`.
//!   StakerStatus (Idle/Validator/Nominator) → `GuardianStatus`.
//!   SlashDeferDuration → `GUARDIAN_CHANGE_DELAY_SECS` (7 gün).
//!
//! **Solana** (`sdk/program/src/vote/state/mod.rs`):
//!   VoteState.epoch_credits → guardian responsiveness score.
//!   Bank lifecycle (genesis→open→freeze→root) → `AccountLifecycle`.
//!
//! **Avalanchego** (uptime tracking):
//!   Validator uptime hesabı → guardian responsiveness yüzdesi.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use nova_primitives::types::{Address, Hash32, SlotId};

// ─── SABITLER ─────────────────────────────────────────────────────────────────

/// Minimum threshold oranı: >1/2 (çoğunluk quorum) (§5.1.2)
/// Whitepaper varsayılanı 3/5; BFT güvenliği için threshold > n/2 zorunludur.
pub const MIN_THRESHOLD_RATIO_NUM: u8 = 1;
pub const MIN_THRESHOLD_RATIO_DEN: u8 = 2;

/// Maksimum guardian sayısı: 15 (§5.1.2: maks 10/15)
pub const MAX_GUARDIANS: u8 = 15;

/// Minimum guardian sayısı: 3 (en az 2/3 için gerekli minimum)
pub const MIN_GUARDIANS: u8 = 3;

/// Guardian değişikliği bekleme süresi: 7 gün (§5.1.5)
/// Polkadot-SDK'nın SlashDeferDuration benzeri güvenlik penceresi.
pub const GUARDIAN_CHANGE_DELAY_SECS: u64 = 7 * 24 * 3600;

/// Sosyal kurtarma bekleme süresi: 48 saat (§5.1.4)
/// Rolling Shutter'ın epoch reveal window'undan ilham alındı.
pub const RECOVERY_DELAY_SECS: u64 = 48 * 3600;

/// Kurtarma talebinin geçerlilik süresi: 30 gün
pub const RECOVERY_EXPIRY_SECS: u64 = 30 * 24 * 3600;

/// Heartbeat minimum aralık: 1 dakika (spam koruması)
/// Polkadot Im-Online pallet heartbeat interval benzeri.
pub const HEARTBEAT_MIN_INTERVAL_SECS: u64 = 60;

/// Hesap inaktivite minimum süresi: 30 gün (§5.4.2)
pub const MIN_INACTIVITY_PERIOD_SECS: u64 = 30 * 24 * 3600;

/// Secp256k1 compressed public key boyutu: 33 byte
pub const COMPRESSED_PUBKEY_SIZE: usize = 33;

/// MPC ortak public key boyutu (secp256k1 compressed): 33 byte
pub const MPC_PUBKEY_SIZE: usize = COMPRESSED_PUBKEY_SIZE;

/// VSS commitment boyutu (threshold + 1 nokta × 33 byte)
pub const MAX_VSS_COMMITMENT_SIZE: usize = 16 * COMPRESSED_PUBKEY_SIZE;

/// Hesap nonce başlangıcı (Cosmos-SDK BaseAccount.Sequence karşılığı)
pub const INITIAL_NONCE: u64 = 0;

// ─── HESAP KİMLİĞİ VE ADRES ──────────────────────────────────────────────────

/// Nova Shield hesap kimliği — monoton artan sayaç.
/// Cosmos-SDK'nın AccountNumber'ından ilham alındı.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
)]
pub struct AccountId(pub u64);

impl AccountId {
    pub const ZERO: Self = Self(0);

    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "account#{}", self.0)
    }
}

// ─── THRESHOLD YAPILANDIRMASI ─────────────────────────────────────────────────

/// MPC Threshold yapılandırması — GG20'nin Parameters{threshold, share_count}
/// yapısından türetildi. §5.1.2: varsayılan 3/5, min 2/3, maks 10/15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Kaç guardian imzası gerekli (t).
    pub threshold: u8,
    /// Toplam guardian sayısı (n).
    pub total: u8,
}

impl ThresholdConfig {
    /// Varsayılan 3/5 threshold (§5.1.2) — whitepaper varsayılanı.
    pub const DEFAULT: Self = Self {
        threshold: 3,
        total: 5,
    };

    /// Yeni threshold yapılandırması oluştur.
    pub fn new(threshold: u8, total: u8) -> Result<Self, &'static str> {
        if threshold == 0 {
            return Err("threshold sıfır olamaz");
        }
        if threshold > total {
            return Err("threshold toplam guardian sayısından büyük olamaz");
        }
        if total < MIN_GUARDIANS {
            return Err("minimum guardian sayısı 3");
        }
        if total > MAX_GUARDIANS {
            return Err("maksimum guardian sayısı 15");
        }
        // Minimum 2/3 oranı kontrolü
        // threshold/total >= 2/3 → 3*threshold >= 2*total
        let ratio_ok = (threshold as u32) * (MIN_THRESHOLD_RATIO_DEN as u32)
            >= (total as u32) * (MIN_THRESHOLD_RATIO_NUM as u32);
        if !ratio_ok {
            return Err("threshold oranı minimum 2/3 olmalıdır");
        }
        Ok(Self { threshold, total })
    }

    /// Bu threshold yapılandırması geçerli mi?
    pub fn is_valid(&self) -> bool {
        Self::new(self.threshold, self.total).is_ok()
    }

    /// Maksimum kaç guardian offline olabilir?
    pub fn max_offline(&self) -> u8 {
        self.total - self.threshold
    }

    /// İmza oranını hesapla (yüzde × 100, örn: 3/5 → 6000)
    pub fn ratio_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.threshold as u32) * 10_000 / (self.total as u32)
    }
}

impl fmt::Display for ThresholdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.threshold, self.total)
    }
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ─── GUARDIAN PARTİ KİMLİĞİ ──────────────────────────────────────────────────

/// Guardian parti kimliği — TSS-Lib'in PartyID{id, moniker, key, index}
/// yapısından ilham alındı. Her guardian protokol oturumunda bu kimlikle tanınır.
///
/// TSS-Lib'de `SortPartyIDs` deterministik sıralama yapar; biz de
/// `GuardianSet::sorted()` ile aynı prensibi uygularız.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuardianPartyId {
    /// Benzersiz string kimlik (TSS-Lib: `id`).
    pub id: String,
    /// İnsan okunabilir isim (TSS-Lib: `moniker`).
    pub moniker: String,
    /// Protokol içi sıralama için anahtar (TSS-Lib: `key` — big.Int bytes).
    /// Secp256k1 compressed public key bytes kullanılır.
    pub sort_key: Vec<u8>,
    /// Sıralanmış indeks — SortPartyIDs sonrası atanır (TSS-Lib: `Index`).
    pub index: usize,
}

impl GuardianPartyId {
    pub fn new(id: impl Into<String>, moniker: impl Into<String>, pubkey: &[u8]) -> Self {
        Self {
            id: id.into(),
            moniker: moniker.into(),
            sort_key: pubkey.to_vec(),
            index: usize::MAX, // SortPartyIDs'den önce geçersiz
        }
    }
}

impl fmt::Display for GuardianPartyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guardian[{}:{}]", self.index, self.moniker)
    }
}

// ─── GUARDIAN KATEGORİSİ ──────────────────────────────────────────────────────

/// Guardian kategorisi — §5.1.5
///
/// Polkadot-SDK'nın StakerStatus (Idle/Validator/Nominator) enum'undan
/// ilham alındı; ancak Nova Shield'e özgü kategoriler kullanıldı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianCategory {
    /// Kişisel guardian — başka bir Nova Shield hesabı.
    Personal {
        /// Guardian'ın Nova Shield adresi.
        address: Address,
    },
    /// Kurumsal guardian — lisanslı saklama hizmeti sağlayıcısı.
    Institutional {
        /// Kurum kimliği (on-chain kayıtlı).
        institution_id: String,
        /// Kurum doğrulama adresi.
        address: Address,
    },
    /// Akıllı kontrat guardian — DAO veya multi-sig kontrat.
    SmartContract {
        /// Kontrat adresi.
        contract_address: Address,
        /// Kontratın kullandığı threshold.
        contract_threshold: ThresholdConfig,
    },
    /// Zaman kilitli vault — belirli süre sonra aktif olan guardian (K5).
    TimeLock {
        /// Vault adresi.
        vault_address: Address,
        /// Aktivasyon için gerekli minimum blok yüksekliği.
        activate_after_slot: SlotId,
    },
}

impl GuardianCategory {
    /// Bu guardian kategorisinin birincil adresi.
    pub fn primary_address(&self) -> &Address {
        match self {
            Self::Personal { address } => address,
            Self::Institutional { address, .. } => address,
            Self::SmartContract { contract_address, .. } => contract_address,
            Self::TimeLock { vault_address, .. } => vault_address,
        }
    }

    /// Kategori açıklaması.
    pub fn category_name(&self) -> &'static str {
        match self {
            Self::Personal { .. } => "kişisel",
            Self::Institutional { .. } => "kurumsal",
            Self::SmartContract { .. } => "akıllı kontrat",
            Self::TimeLock { .. } => "zaman kilidi",
        }
    }

    /// Zaman kilitli vault mu? (Yalnızca slot koşulu sağlandığında aktif)
    pub fn is_time_locked(&self) -> bool {
        matches!(self, Self::TimeLock { .. })
    }

    /// Bu guardian belirli slotta aktif mi?
    pub fn is_active_at_slot(&self, current_slot: SlotId) -> bool {
        match self {
            Self::TimeLock { activate_after_slot, .. } => current_slot >= *activate_after_slot,
            _ => true,
        }
    }
}

// ─── GUARDIAN DURUMU ──────────────────────────────────────────────────────────

/// Guardian'ın mevcut durumu.
///
/// Polkadot-SDK staking paletinin StakerStatus enum'undan ilham alındı.
/// Avalanchego'nun uptime tracking yapısından responsiveness puanlama alındı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianStatus {
    /// Aktif ve imza üretebilir.
    Active,
    /// Eklenmeyi bekliyor — 7 günlük bekleme süresi (§5.1.5).
    /// Polkadot-SDK'nın SlashDeferDuration benzeri güvenlik penceresi.
    PendingActivation {
        /// Aktivasyon gerçekleşecek slot.
        activates_at_slot: SlotId,
        /// Bekleme süresinin kaç saniyesi kaldı.
        remaining_secs: u64,
    },
    /// Kaldırılmayı bekliyor — güvenlik penceresi açık.
    PendingRemoval {
        /// Kaldırma gerçekleşecek slot.
        removes_at_slot: SlotId,
        remaining_secs: u64,
    },
    /// Geçici olarak askıya alındı — kötü davranış tespiti.
    Suspended {
        reason: String,
        suspended_until_slot: SlotId,
    },
    /// Kalıcı olarak devre dışı — yetersiz performans veya kötü niyet.
    Deactivated {
        reason: String,
    },
}

impl GuardianStatus {
    /// Bu durum imzalama için geçerli mi?
    pub fn can_sign(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Bu durum aktif mi (genel anlamda)?
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Durum adı (string).
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::Active => "aktif",
            Self::PendingActivation { .. } => "aktivasyon bekleniyor",
            Self::PendingRemoval { .. } => "kaldırma bekleniyor",
            Self::Suspended { .. } => "askıya alındı",
            Self::Deactivated { .. } => "devre dışı",
        }
    }
}

// ─── GUARDIAN KAYDI ──────────────────────────────────────────────────────────

/// Tek bir guardian'ın tam kaydı.
///
/// Bir hesabın guardian ağı bu kayıtlardan oluşur.
/// Polkadot-SDK ValidatorPrefs {commission, blocked} → GuardianPreferences.
/// TSS-Lib PartyID → GuardianPartyId alanı.
/// Avalanchego uptime → responsiveness_score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRecord {
    /// TSS-Lib benzeri parti kimliği.
    pub party_id: GuardianPartyId,
    /// Guardian kategorisi (§5.1.5).
    pub category: GuardianCategory,
    /// Guardian'ın secp256k1 compressed public key'i (33 byte).
    pub signing_pubkey: Vec<u8>,
    /// Mevcut durum.
    pub status: GuardianStatus,
    /// Kaydedilme slot'u.
    pub registered_at_slot: SlotId,
    /// Son aktif olduğu slot.
    pub last_active_slot: SlotId,
    /// İmza başarı sayısı (sorumlu toplam imza içinde).
    pub signatures_provided: u64,
    /// İmza taleplerinde yanıt vermediği sayı.
    pub missed_signing_requests: u64,
    /// Yanlış freeze girişimi sayısı (§5.5.4: 3 yanlış → yetkisi askıya alınır).
    pub false_freeze_attempts: u8,
    /// Guardian tercih parametreleri.
    pub preferences: GuardianPreferences,
}

/// Guardian tercih parametreleri (Polkadot ValidatorPrefs benzeri).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianPreferences {
    /// Bu guardian freeze yetkisine sahip mi?
    pub can_freeze: bool,
    /// Bu guardian sosyal kurtarma onayı verebilir mi?
    pub can_approve_recovery: bool,
    /// İmzalama bildirimi için tercih edilen kanal (off-chain, isteğe bağlı).
    pub notification_endpoint_hash: Option<Hash32>,
}

impl Default for GuardianPreferences {
    fn default() -> Self {
        Self {
            can_freeze: true,
            can_approve_recovery: true,
            notification_endpoint_hash: None,
        }
    }
}

impl GuardianRecord {
    /// Yeni guardian kaydı oluştur (Cosmos-SDK NewBaseAccount benzeri fabrika metodu).
    pub fn new(
        party_id: GuardianPartyId,
        category: GuardianCategory,
        signing_pubkey: Vec<u8>,
        current_slot: SlotId,
    ) -> Self {
        let activate_slot = SlotId(current_slot.0 + guardian_change_delay_in_slots());
        Self {
            party_id,
            category,
            signing_pubkey,
            status: GuardianStatus::PendingActivation {
                activates_at_slot: activate_slot,
                remaining_secs: GUARDIAN_CHANGE_DELAY_SECS,
            },
            registered_at_slot: current_slot,
            last_active_slot: current_slot,
            signatures_provided: 0,
            missed_signing_requests: 0,
            false_freeze_attempts: 0,
            preferences: GuardianPreferences::default(),
        }
    }

    /// Avalanchego uptime hesaplama benzeri responsiveness puanı.
    /// Yüksek puan → yüksek sorumluluk.
    pub fn responsiveness_bps(&self) -> u32 {
        let total = self.signatures_provided + self.missed_signing_requests as u64;
        if total == 0 {
            return 10_000; // Henüz hiç talep gelmedi → tam puan
        }
        (self.signatures_provided * 10_000 / total) as u32
    }

    /// Guardian sağlıklı mı? (min %80 responsiveness)
    pub fn is_healthy(&self) -> bool {
        self.status.is_active() && self.responsiveness_bps() >= 8_000
    }
}

/// 7 günlük bekleme süresini slot cinsine çevir (400ms/slot varsayımıyla).
fn guardian_change_delay_in_slots() -> u64 {
    // 400ms per slot → saniye başına 2.5 slot → 7 gün × 86400s × 2.5 ≈ 1,512,000 slot
    GUARDIAN_CHANGE_DELAY_SECS * 1000 / 400
}

// ─── GUARDIAN SET ─────────────────────────────────────────────────────────────

/// Bir hesabın tüm guardian kümesi.
///
/// TSS-Lib'in SortedPartyIDs benzeri deterministic sıralama destekler.
/// TSS-Lib ReSharingParameters → `GuardianSetChange`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSet {
    /// Tüm guardian'lar (hem aktif hem beklemede).
    pub guardians: HashMap<String, GuardianRecord>,
    /// Threshold yapılandırması.
    pub threshold: ThresholdConfig,
    /// Guardian seti versiyonu (her değişimde artar — TSS-Lib epoch benzeri).
    pub version: u64,
    /// Bu setin oluşturulduğu slot.
    pub created_at_slot: SlotId,
}

impl GuardianSet {
    /// Yeni guardian seti oluştur.
    pub fn new(threshold: ThresholdConfig, current_slot: SlotId) -> Self {
        Self {
            guardians: HashMap::new(),
            threshold,
            version: 1,
            created_at_slot: current_slot,
        }
    }

    /// Aktif guardian sayısı.
    pub fn active_count(&self) -> usize {
        self.guardians
            .values()
            .filter(|g| g.status.can_sign())
            .count()
    }

    /// TSS-Lib SortPartyIDs benzeri — sort_key'e göre sıralanmış aktif guardian'lar.
    pub fn sorted_active(&self) -> Vec<&GuardianRecord> {
        let mut active: Vec<_> = self
            .guardians
            .values()
            .filter(|g| g.status.can_sign())
            .collect();
        active.sort_by(|a, b| a.party_id.sort_key.cmp(&b.party_id.sort_key));
        // TSS-Lib: assign index after sort
        active
    }

    /// Threshold karşılanabiliyor mu?
    pub fn threshold_reachable(&self) -> bool {
        self.active_count() >= self.threshold.threshold as usize
    }

    /// Guardian ID listesi (aktif olanlar).
    pub fn active_ids(&self) -> Vec<String> {
        self.guardians
            .iter()
            .filter(|(_, g)| g.status.can_sign())
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Guardian seti değişikliği — TSS-Lib ReSharingParameters karşılığı.
/// Eski komiteden yeni komiteye key share aktarımı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSetChange {
    /// Eski guardian seti (TSS-Lib: old_committee).
    pub old_set: GuardianSet,
    /// Yeni guardian seti (TSS-Lib: new_committee).
    pub new_set: GuardianSet,
    /// Eski threshold (TSS-Lib: OldThreshold).
    pub old_threshold: ThresholdConfig,
    /// Yeni threshold (TSS-Lib: NewThreshold).
    pub new_threshold: ThresholdConfig,
    /// Değişiklik başlatma slot'u.
    pub initiated_at_slot: SlotId,
    /// Değişikliğin gerçekleşeceği slot (7 gün sonra).
    pub executes_at_slot: SlotId,
    /// Değişikliği talep eden (kullanıcı imzalı).
    pub initiated_by: Address,
}

// ─── HESAP YAŞAM DÖNGÜSÜ ─────────────────────────────────────────────────────

/// Hesap yaşam döngüsü durumları.
///
/// Solana'nın Bank lifecycle'ından (genesis→open→freeze→root) ilham alındı.
/// §5.5 Emergency Freeze ve §5.4 Inheritance ile entegre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountLifecycle {
    /// Hesap aktif — tüm işlemler geçerli (Solana: "Bank açık").
    Active,
    /// Hesap dondurulmuş — çıkış işlemleri engellendi (§5.5).
    /// Solana'nın "Bank donduruldu" durumuna benzer ancak nova'da giriş açık.
    Frozen {
        /// Dondurma zamanı (unix timestamp saniye).
        frozen_at_secs: u64,
        /// Dondurma sebebi.
        reason: FreezeReason,
        /// Çözme penceresi bitiş zamanı (72 saat — §5.5.3).
        investigation_ends_at_secs: u64,
    },
    /// Sosyal kurtarma modunda (§5.1.4).
    Recovery {
        /// Kurtarma talebi ID'si.
        request_id: String,
        /// Kurtarma başlangıç slot'u.
        started_at_slot: SlotId,
        /// 48 saat sonra gerçekleşecek slot.
        executes_at_slot: SlotId,
    },
    /// Miras protokolü aktif (§5.4).
    Inheritance {
        /// Miras talebi ID'si.
        plan_id: String,
        /// Tetiklenme slot'u.
        triggered_at_slot: SlotId,
        /// Miras aşaması (1/2/3/4).
        phase: u8,
    },
    /// Hesap kapatıldı / tüm varlıklar devredildi.
    Closed {
        closed_at_slot: SlotId,
    },
}

impl AccountLifecycle {
    /// Hesap normal işlem yapabilir mi?
    pub fn can_transact(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Hesap para alabilir mi?
    pub fn can_receive(&self) -> bool {
        !matches!(self, Self::Closed { .. })
    }

    /// Hesap dondurulmuş mu?
    pub fn is_frozen(&self) -> bool {
        matches!(self, Self::Frozen { .. })
    }

    /// Durum adı.
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::Active => "aktif",
            Self::Frozen { .. } => "dondurulmuş",
            Self::Recovery { .. } => "kurtarma modunda",
            Self::Inheritance { .. } => "miras protokolünde",
            Self::Closed { .. } => "kapalı",
        }
    }
}

// ─── DONDURMA SEBEBİ ─────────────────────────────────────────────────────────

/// Hesap dondurma sebebi — §5.5.2
///
/// Whitepaper'da `FreezeReason` olarak belirtilmiş.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreezeReason {
    /// Kullanıcı kendi dondurdu — tek cihaz yeterli (§5.5.2 Tetikleyici 1).
    UserInitiated,
    /// Guardian threshold dondurdu (§5.5.2 Tetikleyici 2).
    GuardianThreshold {
        /// Onay veren guardian adresleri.
        signers: Vec<Address>,
    },
    /// Otomatik anomali tespiti (§5.5.2 Tetikleyici 3).
    AnomalyDetected {
        /// Anomali skoru (0.0 - 1.0, bps olarak × 10_000).
        anomaly_score_bps: u32,
        /// Anomali türü.
        anomaly_type: AnomalyType,
    },
}

/// Anomali türleri — §5.5.2 Tetikleyici 3
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Olağandışı yüksek transfer hacmi.
    UnusualTransferVolume,
    /// Bilinmeyen adrese büyük transfer.
    TransferToUnknownAddress,
    /// Hızlı arka arkaya çoklu çıkış transferi.
    RapidMultipleWithdrawals,
    /// Coğrafi konumda ani değişim (validator gözlemi).
    GeographicAnomaliy,
    /// Guardian tarafından bildirilen şüpheli davranış.
    GuardianReport,
}

// ─── HESAP YAPILANDIRMASI ─────────────────────────────────────────────────────

/// Hesap yapılandırması — kullanıcı tarafından özelleştirilir.
///
/// Cosmos-SDK'nın module params yapısından ilham alındı.
/// §5.1.2 ve §5.4 whitepaper spesifikasyonuna tam uyum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Threshold yapılandırması.
    pub threshold: ThresholdConfig,
    /// Hesap inaktivite eşiği (saniye) — §5.4.2 minimum 30 gün.
    pub inactivity_period_secs: u64,
    /// Sosyal kurtarma etkin mi?
    pub social_recovery_enabled: bool,
    /// Miras planlaması etkin mi?
    pub inheritance_enabled: bool,
    /// Duress kodu etkin mi? (§5.4.4 Yöntem 2)
    pub duress_code_enabled: bool,
    /// Anomali tespiti etkin mi? (§5.5.2 Tetikleyici 3)
    pub anomaly_detection_enabled: bool,
    /// Heartbeat ücretsiz mi? (Whitepaper: heartbeat fee muafiyeti)
    pub heartbeat_fee_exempt: bool,
    /// Hesap oluşturulma slot'u.
    pub created_at_slot: SlotId,
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self {
            threshold: ThresholdConfig::DEFAULT,
            inactivity_period_secs: MIN_INACTIVITY_PERIOD_SECS * 6, // 180 gün varsayılan
            social_recovery_enabled: true,
            inheritance_enabled: false, // Kullanıcı açıkça etkinleştirmeli
            duress_code_enabled: false, // Opt-in güvenlik özelliği
            anomaly_detection_enabled: true,
            heartbeat_fee_exempt: true,
            created_at_slot: SlotId(0),
        }
    }
}

impl AccountConfig {
    pub fn with_slot(slot: SlotId) -> Self {
        Self {
            created_at_slot: slot,
            ..Self::default()
        }
    }
}

// ─── HESAP DURUMU ─────────────────────────────────────────────────────────────

/// Nova Shield MPC-Native hesap durumu.
///
/// Cosmos-SDK `BaseAccount` {Address, PubKey, AccountNumber, Sequence}
/// yapısını temel alır, ancak MPC-native guardian sistemiyle genişletilir.
///
/// Solana `Bank` struct'ının atomik sayaçlarından ilham alınarak
/// `nonce` ve `tx_count` alanları eklendi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    /// Benzersiz hesap kimliği (Cosmos-SDK: AccountNumber).
    pub account_id: AccountId,
    /// Hesap adresi — MPC ortak public key'den türetilen 20 byte.
    /// `BLAKE3(mpc_public_key)[12..32]`
    pub address: Address,
    /// MPC ortak public key (secp256k1 compressed, 33 byte).
    /// GG20'de `SharedKeys.y` alanına karşılık gelir.
    pub mpc_public_key: Vec<u8>,
    /// İşlem sıralaması için nonce (Cosmos-SDK: Sequence, Ethereum: nonce).
    pub nonce: u64,
    /// Toplam işlem sayısı (Solana Bank: transaction_count).
    pub tx_count: u64,
    /// Guardian kümesi ve threshold yapılandırması.
    pub guardian_set: GuardianSet,
    /// Hesap yapılandırması.
    pub config: AccountConfig,
    /// Yaşam döngüsü durumu.
    pub lifecycle: AccountLifecycle,
    /// Son işlem/imza/heartbeat slot'u (inaktivite sayacı için).
    pub last_activity_slot: SlotId,
    /// Son heartbeat slot'u.
    pub last_heartbeat_slot: SlotId,
    /// Toplam heartbeat sayısı.
    pub heartbeat_count: u64,
    /// Duress kodu hash'i (isteğe bağlı — §5.4.4 Yöntem 2).
    pub duress_code_hash: Option<Hash32>,
    /// Hesap oluşturulma slot'u.
    pub genesis_slot: SlotId,
    /// Son güncelleme slot'u.
    pub last_updated_slot: SlotId,
    /// Bekleyen guardian set değişikliği (7 günlük pencerede).
    pub pending_guardian_change: Option<GuardianSetChange>,
}

impl AccountState {
    /// Yeni hesap durumu oluştur — genesis benzeri başlangıç.
    pub fn genesis(
        account_id: AccountId,
        address: Address,
        mpc_public_key: Vec<u8>,
        guardian_set: GuardianSet,
        config: AccountConfig,
        current_slot: SlotId,
    ) -> Self {
        Self {
            account_id,
            address,
            mpc_public_key,
            nonce: INITIAL_NONCE,
            tx_count: 0,
            guardian_set,
            config,
            lifecycle: AccountLifecycle::Active,
            last_activity_slot: current_slot,
            last_heartbeat_slot: current_slot,
            heartbeat_count: 0,
            duress_code_hash: None,
            genesis_slot: current_slot,
            last_updated_slot: current_slot,
            pending_guardian_change: None,
        }
    }

    /// Hesap aktif mi?
    pub fn is_active(&self) -> bool {
        self.lifecycle.can_transact()
    }

    /// Nonce artır ve yeni değeri döndür.
    pub fn increment_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.tx_count += 1;
        self.nonce
    }

    /// İnaktivite süresi geçti mi?
    pub fn is_inactive(&self, current_slot: SlotId) -> bool {
        let elapsed_slots = current_slot.0.saturating_sub(self.last_activity_slot.0);
        // 400ms/slot varsayımı
        let elapsed_secs = elapsed_slots * 400 / 1000;
        elapsed_secs >= self.config.inactivity_period_secs
    }

    /// Hesap adresi string'i.
    pub fn address_hex(&self) -> String {
        format!("0x{}", hex::encode(self.address.as_bytes()))
    }
}

// ─── HEARTBEAT KAYDI ──────────────────────────────────────────────────────────

/// Heartbeat kaydı — Polkadot Im-Online pallet heartbeat benzeri.
///
/// §5.4.9: "Heartbeat işlemleri protokol tarafından fee muafiyetine sahiptir."
/// Whitepaper §5.4.4: heartbeat inaktivite sayacını sıfırlar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRecord {
    /// Hesap adresi.
    pub account: Address,
    /// Heartbeat gönderildiği slot.
    pub slot: SlotId,
    /// Heartbeat gönderildiği unix timestamp.
    pub timestamp_secs: u64,
    /// MPC imzası (threshold imzası — heartbeat'in gerçek sahibinden).
    pub signature: Vec<u8>,
    /// Duress heartbeat mi? (§5.4.4 Yöntem 2 — gizli alarm)
    pub is_duress: bool,
    /// Bu heartbeat'in hash'i (doğrulama için).
    pub heartbeat_hash: Hash32,
}

impl HeartbeatRecord {
    /// Heartbeat hash'ini hesapla.
    pub fn compute_hash(account: &Address, slot: &SlotId, timestamp_secs: u64) -> Hash32 {
        let mut data = Vec::with_capacity(20 + 8 + 8);
        data.extend_from_slice(account.as_bytes());
        data.extend_from_slice(&slot.0.to_le_bytes());
        data.extend_from_slice(&timestamp_secs.to_le_bytes());
        let hash = blake3::hash(&data);
        Hash32::from_bytes(*hash.as_bytes())
    }
}

// ─── SOSYAL KURTARMA TALEBİ ──────────────────────────────────────────────────

/// Sosyal kurtarma talebi — §5.1.4
///
/// Rolling Shutter'ın DecryptionTrigger → keyper response → key reveal
/// akışından ilham alındı: kullanıcı talep → guardian'lar onay → 48 saat sonra yürütme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Benzersiz talep ID'si.
    pub request_id: String,
    /// Kurtarılacak hesap adresi.
    pub account: Address,
    /// Talebi başlatan kişi (yeni cihaz/adresi).
    pub requester_pubkey: Vec<u8>,
    /// Talebin başlatıldığı slot.
    pub requested_at_slot: SlotId,
    /// Talebin başlatıldığı unix timestamp.
    pub requested_at_secs: u64,
    /// 48 saat sonra yürütme slot'u.
    pub executes_at_slot: SlotId,
    /// Talep geçerlilik sonu (30 gün).
    pub expires_at_secs: u64,
    /// Guardian onayları (guardian_id → onay detayı).
    pub approvals: HashMap<String, RecoveryApproval>,
    /// Talep durumu.
    pub status: RecoveryStatus,
    /// Yeni guardian kümesi (kurtarma başarılı olursa bu set kullanılır).
    pub new_guardian_set: Option<GuardianSet>,
}

/// Guardian kurtarma onayı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryApproval {
    /// Onay veren guardian ID'si.
    pub guardian_id: String,
    /// Onay verme slot'u.
    pub approved_at_slot: SlotId,
    /// Guardian MPC imzası.
    pub signature: Vec<u8>,
    /// Onay verilen requester pubkey hash'i (man-in-the-middle koruması).
    pub requester_hash: Hash32,
}

/// Kurtarma talebi durumu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStatus {
    /// Onay toplanıyor.
    Collecting,
    /// Yeterli onay toplandı, 48 saat bekleniyor.
    WaitingPeriod,
    /// Kullanıcı tarafından iptal edildi.
    CancelledByUser,
    /// Talep süresi geçti.
    Expired,
    /// Kurtarma başarıyla tamamlandı.
    Completed,
}

impl RecoveryRequest {
    /// Yeterli onay var mı?
    pub fn has_sufficient_approvals(&self, threshold: u8) -> bool {
        self.approvals.len() >= threshold as usize
    }

    /// 48 saatlik bekleme süresi geçti mi?
    pub fn waiting_period_elapsed(&self, current_slot: SlotId) -> bool {
        current_slot.0 >= self.executes_at_slot.0
    }

    /// Talep süresi geçti mi?
    pub fn is_expired(&self, current_timestamp_secs: u64) -> bool {
        current_timestamp_secs >= self.expires_at_secs
    }
}

// ─── KEY SHARE BİLGİSİ ───────────────────────────────────────────────────────

/// Bir guardian'ın key share meta verisi — GG20 Keys yapısından ilham alındı.
///
/// NOT: Gerçek private key materyali bu struct'ta bulunmaz.
/// Sadece key share'in doğrulaması için gereken public bilgiler tutulur.
///
/// GG20: `Keys {u_i, y_i, ek, dk, party_index}` → burada sadece public kısım.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyShareInfo {
    /// Guardian'ın parti indeksi (GG20: party_index).
    pub party_index: usize,
    /// Guardian'ın public share (GG20: y_i = G × u_i).
    pub public_share: Vec<u8>,
    /// VSS commitment bytes (Feldman VSS doğrulama için).
    pub vss_commitment: Vec<u8>,
    /// Bu key share'in oluşturulduğu slot.
    pub generated_at_slot: SlotId,
    /// Key share versiyonu (her reshare'de artar).
    pub version: u64,
}

/// MPC key üretim sonucu özeti — GG20 SharedKeys.y benzeri.
/// Gerçek private veri içermez; yalnızca on-chain kaydedilen public bilgi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyGenResult {
    /// Ortak MPC public key (GG20: SharedKeys.y = G × x).
    pub combined_public_key: Vec<u8>,
    /// Bu public key'den türetilen Nova Shield adresi.
    pub derived_address: Address,
    /// Her guardian için key share bilgisi.
    pub key_shares: Vec<KeyShareInfo>,
    /// Keygen tamamlanma slot'u.
    pub finalized_at_slot: SlotId,
    /// Keygen protokol versiyonu (reshare sayısı).
    pub keygen_version: u64,
}

// ─── HESAP ÖZETİ ─────────────────────────────────────────────────────────────

/// Dışa açık hesap özeti — RPC yanıtları için.
/// Cosmos-SDK query response yapısından ilham alındı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_id: AccountId,
    pub address: String,
    pub threshold: ThresholdConfig,
    pub active_guardian_count: usize,
    pub lifecycle_status: String,
    pub nonce: u64,
    pub tx_count: u64,
    pub heartbeat_count: u64,
    pub last_activity_slot: SlotId,
    pub guardian_set_version: u64,
    pub is_healthy: bool,
}

impl From<&AccountState> for AccountSummary {
    fn from(state: &AccountState) -> Self {
        let active_guardian_count = state.guardian_set.active_count();
        let is_healthy = state.is_active()
            && state.guardian_set.threshold_reachable()
            && active_guardian_count >= state.guardian_set.threshold.threshold as usize;
        Self {
            account_id: state.account_id,
            address: state.address_hex(),
            threshold: state.guardian_set.threshold,
            active_guardian_count,
            lifecycle_status: state.lifecycle.status_name().to_string(),
            nonce: state.nonce,
            tx_count: state.tx_count,
            heartbeat_count: state.heartbeat_count,
            last_activity_slot: state.last_activity_slot,
            guardian_set_version: state.guardian_set.version,
            is_healthy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1000)
    }

    // ─── ThresholdConfig testleri ──────────────────────────────────────────

    #[test]
    fn threshold_default_gecerli() {
        assert!(ThresholdConfig::DEFAULT.is_valid());
        assert_eq!(ThresholdConfig::DEFAULT.threshold, 3);
        assert_eq!(ThresholdConfig::DEFAULT.total, 5);
    }

    #[test]
    fn threshold_gecerli_oranlar_kabul_edilir() {
        assert!(ThresholdConfig::new(2, 3).is_ok()); // 2/3 = tam minimum
        assert!(ThresholdConfig::new(3, 4).is_ok()); // 3/4 > 2/3
        assert!(ThresholdConfig::new(3, 5).is_ok()); // 3/5 — whitepaper varsayılanı
        assert!(ThresholdConfig::new(10, 15).is_ok()); // 10/15 = 2/3 tam
    }

    #[test]
    fn threshold_dusuk_oran_reddedilir() {
        // 1/3 < 2/3 → hata
        assert!(ThresholdConfig::new(1, 3).is_err());
        // 2/5 < 2/3 → hata
        assert!(ThresholdConfig::new(2, 5).is_err());
    }

    #[test]
    fn threshold_t_buyuk_n_reddedilir() {
        assert!(ThresholdConfig::new(4, 3).is_err());
    }

    #[test]
    fn threshold_sifir_reddedilir() {
        assert!(ThresholdConfig::new(0, 5).is_err());
    }

    #[test]
    fn threshold_max_guardian_asimi_reddedilir() {
        assert!(ThresholdConfig::new(11, 16).is_err());
    }

    #[test]
    fn threshold_min_guardian_altı_reddedilir() {
        assert!(ThresholdConfig::new(2, 2).is_err());
    }

    #[test]
    fn threshold_ratio_bps_dogru_hesaplanir() {
        let t = ThresholdConfig::new(3, 5).unwrap();
        assert_eq!(t.ratio_bps(), 6000); // 3/5 = 60%
        let t2 = ThresholdConfig::new(2, 3).unwrap();
        assert_eq!(t2.ratio_bps(), 6666); // 2/3 = 66.66%
    }

    #[test]
    fn threshold_max_offline_dogru() {
        let t = ThresholdConfig::new(3, 5).unwrap();
        assert_eq!(t.max_offline(), 2);
    }

    #[test]
    fn threshold_display_dogru() {
        let t = ThresholdConfig::DEFAULT;
        assert_eq!(t.to_string(), "3/5");
    }

    // ─── GuardianPartyId testleri ──────────────────────────────────────────

    #[test]
    fn guardian_party_id_olusturulur() {
        let pid = GuardianPartyId::new("g1", "Alice Guardian", &[0x01, 0x02, 0x03]);
        assert_eq!(pid.id, "g1");
        assert_eq!(pid.moniker, "Alice Guardian");
        assert_eq!(pid.sort_key, vec![0x01, 0x02, 0x03]);
        assert_eq!(pid.index, usize::MAX); // henüz sıralanmadı
    }

    // ─── GuardianCategory testleri ────────────────────────────────────────

    #[test]
    fn guardian_kategori_personal_aktif() {
        let cat = GuardianCategory::Personal {
            address: test_address(),
        };
        assert!(cat.is_active_at_slot(test_slot()));
        assert!(!cat.is_time_locked());
        assert_eq!(cat.category_name(), "kişisel");
    }

    #[test]
    fn guardian_kategori_timelock_slot_kontrolu() {
        let activate_slot = SlotId(2000);
        let cat = GuardianCategory::TimeLock {
            vault_address: test_address(),
            activate_after_slot: activate_slot,
        };
        assert!(!cat.is_active_at_slot(SlotId(1000))); // henüz aktif değil
        assert!(cat.is_active_at_slot(SlotId(2000))); // tam aktivasyon
        assert!(cat.is_active_at_slot(SlotId(3000))); // geçmiş aktivasyon
        assert!(cat.is_time_locked());
    }

    // ─── GuardianRecord testleri ──────────────────────────────────────────

    #[test]
    fn guardian_record_yeni_beklemede_olusturulur() {
        let pid = GuardianPartyId::new("g1", "Test", &[0x01]);
        let cat = GuardianCategory::Personal { address: test_address() };
        let rec = GuardianRecord::new(pid, cat, vec![0x02, 0x03], test_slot());
        // Yeni guardian hemen aktif olmaz — 7 gün bekleme
        assert!(!rec.status.can_sign());
        assert!(matches!(rec.status, GuardianStatus::PendingActivation { .. }));
    }

    #[test]
    fn guardian_responsiveness_ilk_basta_tam_puan() {
        let pid = GuardianPartyId::new("g1", "Test", &[0x01]);
        let cat = GuardianCategory::Personal { address: test_address() };
        let rec = GuardianRecord::new(pid, cat, vec![], test_slot());
        assert_eq!(rec.responsiveness_bps(), 10_000); // Hiç talep yok → tam puan
    }

    #[test]
    fn guardian_responsiveness_hesaplanir() {
        let pid = GuardianPartyId::new("g1", "Test", &[0x01]);
        let cat = GuardianCategory::Personal { address: test_address() };
        let mut rec = GuardianRecord::new(pid, cat, vec![], test_slot());
        rec.signatures_provided = 8;
        rec.missed_signing_requests = 2;
        // 8 / (8+2) = 80%
        assert_eq!(rec.responsiveness_bps(), 8_000);
    }

    // ─── GuardianSet testleri ─────────────────────────────────────────────

    #[test]
    fn guardian_set_bos_threshold_saglanamaz() {
        let set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        assert_eq!(set.active_count(), 0);
        assert!(!set.threshold_reachable());
    }

    // ─── AccountLifecycle testleri ────────────────────────────────────────

    #[test]
    fn lifecycle_active_islem_yapabilir() {
        let lc = AccountLifecycle::Active;
        assert!(lc.can_transact());
        assert!(lc.can_receive());
        assert!(!lc.is_frozen());
    }

    #[test]
    fn lifecycle_frozen_islem_yapamazse_alma_yapabilir() {
        let lc = AccountLifecycle::Frozen {
            frozen_at_secs: 1000,
            reason: FreezeReason::UserInitiated,
            investigation_ends_at_secs: 1000 + 72 * 3600,
        };
        assert!(!lc.can_transact());
        assert!(lc.can_receive()); // Giriş hâlâ açık!
        assert!(lc.is_frozen());
    }

    #[test]
    fn lifecycle_closed_hicbir_sey_yapamaz() {
        let lc = AccountLifecycle::Closed { closed_at_slot: test_slot() };
        assert!(!lc.can_transact());
        assert!(!lc.can_receive());
    }

    // ─── AccountState testleri ────────────────────────────────────────────

    #[test]
    fn account_state_genesis_olusturulur() {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let state = AccountState::genesis(
            AccountId::new(1),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        );
        assert_eq!(state.nonce, 0);
        assert_eq!(state.tx_count, 0);
        assert!(state.is_active());
        assert_eq!(state.genesis_slot, test_slot());
    }

    #[test]
    fn account_state_nonce_artar() {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let mut state = AccountState::genesis(
            AccountId::new(1),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        );
        assert_eq!(state.increment_nonce(), 1);
        assert_eq!(state.increment_nonce(), 2);
        assert_eq!(state.tx_count, 2);
    }

    #[test]
    fn account_state_inactivity_kontrolu() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 30 * 24 * 3600; // 30 gün
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let state = AccountState::genesis(
            AccountId::new(1),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        );
        // 30 gün = 30 * 86400 saniye, 400ms/slot → 30*86400*2.5 = 6,480,000 slot
        let inactive_slot = SlotId(1000 + 30 * 86400 * 1000 / 400 + 1);
        assert!(state.is_inactive(inactive_slot));
        // Henüz inaktif değil
        assert!(!state.is_inactive(SlotId(1001)));
    }

    // ─── HeartbeatRecord testleri ─────────────────────────────────────────

    #[test]
    fn heartbeat_hash_tutarli() {
        let addr = test_address();
        let slot = test_slot();
        let h1 = HeartbeatRecord::compute_hash(&addr, &slot, 1_700_000_000);
        let h2 = HeartbeatRecord::compute_hash(&addr, &slot, 1_700_000_000);
        assert_eq!(h1, h2);
    }

    #[test]
    fn heartbeat_hash_farklı_slot_farklı() {
        let addr = test_address();
        let h1 = HeartbeatRecord::compute_hash(&addr, &SlotId(1000), 1_700_000_000);
        let h2 = HeartbeatRecord::compute_hash(&addr, &SlotId(1001), 1_700_000_000);
        assert_ne!(h1, h2);
    }

    // ─── RecoveryRequest testleri ─────────────────────────────────────────

    #[test]
    fn recovery_request_bekleme_suresi_kontrolu() {
        let req = RecoveryRequest {
            request_id: "req-1".into(),
            account: test_address(),
            requester_pubkey: vec![0x03; 33],
            requested_at_slot: SlotId(1000),
            requested_at_secs: 1_700_000_000,
            executes_at_slot: SlotId(1000 + 432_000), // 48s bekleme ~ slot sayısı
            expires_at_secs: 1_700_000_000 + RECOVERY_EXPIRY_SECS,
            approvals: HashMap::new(),
            status: RecoveryStatus::Collecting,
            new_guardian_set: None,
        };
        assert!(!req.waiting_period_elapsed(SlotId(1001)));
        assert!(req.waiting_period_elapsed(SlotId(1_000 + 432_000)));
        assert!(!req.has_sufficient_approvals(3));
        assert!(!req.is_expired(1_700_000_000));
        assert!(req.is_expired(1_700_000_000 + RECOVERY_EXPIRY_SECS + 1));
    }

    // ─── AccountSummary testleri ──────────────────────────────────────────

    #[test]
    fn account_summary_durumdan_tureyebilir() {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let state = AccountState::genesis(
            AccountId::new(42),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        );
        let summary = AccountSummary::from(&state);
        assert_eq!(summary.account_id, AccountId::new(42));
        assert_eq!(summary.nonce, 0);
        assert_eq!(summary.lifecycle_status, "aktif");
        assert_eq!(summary.threshold, ThresholdConfig::DEFAULT);
    }

    // ─── FreezeReason testleri ────────────────────────────────────────────

    #[test]
    fn freeze_reason_guardian_threshold_signers_icerir() {
        let reason = FreezeReason::GuardianThreshold {
            signers: vec![test_address(), test_address()],
        };
        assert!(matches!(reason, FreezeReason::GuardianThreshold { .. }));
    }

    #[test]
    fn freeze_reason_anomaly_detected_skor_icerir() {
        let reason = FreezeReason::AnomalyDetected {
            anomaly_score_bps: 9500,
            anomaly_type: AnomalyType::RapidMultipleWithdrawals,
        };
        assert!(matches!(reason, FreezeReason::AnomalyDetected { .. }));
    }

    // ─── KeyShareInfo testleri ────────────────────────────────────────────

    #[test]
    fn key_share_info_olusturulur() {
        let ks = KeyShareInfo {
            party_index: 2,
            public_share: vec![0x02; 33],
            vss_commitment: vec![0x03; 33 * 4],
            generated_at_slot: test_slot(),
            version: 1,
        };
        assert_eq!(ks.party_index, 2);
        assert_eq!(ks.version, 1);
    }

    #[test]
    fn guardian_set_change_eski_yeni_tutulur() {
        let old_set = GuardianSet::new(ThresholdConfig::DEFAULT, SlotId(100));
        let new_set = GuardianSet::new(ThresholdConfig::new(3, 4).unwrap(), SlotId(200));
        let change = GuardianSetChange {
            old_threshold: ThresholdConfig::DEFAULT,
            new_threshold: ThresholdConfig::new(3, 4).unwrap(),
            old_set: old_set.clone(),
            new_set: new_set.clone(),
            initiated_at_slot: SlotId(200),
            executes_at_slot: SlotId(200 + guardian_change_delay_in_slots()),
            initiated_by: test_address(),
        };
        assert_eq!(change.old_threshold, ThresholdConfig::DEFAULT);
        assert_ne!(change.old_threshold, change.new_threshold);
    }
}
