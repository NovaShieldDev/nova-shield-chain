//! nova-freeze çekirdek veri tipleri — §5.5 Emergency Freeze Protocol
//!
//! Araştırma kaynakları:
//! - Polkadot assets-freezer: `Freezes<AssetId,AccountId>` multi-reason freeze
//! - Solana ALT freeze: `LookupTableStatus` state machine
//! - go-ethereum txpool: `OperationType` reject bitmap
//! - TSS-lib signing: `ThresholdProof` yapısı
//! - Cosmos SDK slashing: `FreezeAuditEntry` accountability log

pub use nova_account::AnomalyType;
pub use nova_account::FreezeReason;
use nova_crypto::{BlsKeypair, BlsVerifier};
use nova_primitives::{crypto::{BlsPublicKey, BlsSignature}, Address, SlotId, SLOT_DURATION_MS};
use serde::{Deserialize, Serialize};

// ─── Sabitler ────────────────────────────────────────────────────────────────

/// 72 saatlik soruşturma penceresi slot sayısı.
/// = FREEZE_INVESTIGATION_WINDOW_SECS * 1000 / SLOT_DURATION_MS
/// = 259200 * 1000 / 400 = 648_000
pub const INVESTIGATION_WINDOW_SLOTS: u64 = 648_000;

/// 24 saatlik kullanıcı iptal penceresi (guardian freeze sonrası).
/// = 24 * 3600 * 1000 / 400 = 216_000
pub const USER_CANCELLATION_WINDOW_SLOTS: u64 = 216_000;

/// Anomali onay süresi: 10 dakika.
/// Pattern: Rolling Shutter commit-reveal timing.
/// = 10 * 60 * 1000 / 400 = 1_500
pub const ANOMALY_CONFIRM_SLOTS: u64 = 1_500;

/// Yanlış freeze sonrası guardian'a uygulanan slash oranı: %5 = 500 bps.
/// Whitepaper §5.5.4: "guardian stake'inden %5 kesinti".
pub const WRONG_FREEZE_SLASH_BPS: u32 = 500;

/// 30 günlük pencerede bu kadar yanlış freeze → guardian jailed.
/// Whitepaper §5.5.4: "30 gün içinde 3 yanlış freeze → guardian yetkisi askıya".
pub const WRONG_FREEZE_JAIL_TRIGGER: u32 = 3;

/// Guardian jail süresi: 30 gün slot cinsinden.
/// = 30 * 24 * 3600 * 1000 / 400 = 6_480_000
pub const GUARDIAN_JAIL_DURATION_SLOTS: u64 = 6_480_000;

/// Yanlış freeze sayım penceresi: 30 gün slot cinsinden.
pub const WRONG_FREEZE_WINDOW_SLOTS: u64 = GUARDIAN_JAIL_DURATION_SLOTS;

/// Anomali tespiti için varsayılan minimum skor eşiği: 70% = 7000 bps.
pub const DEFAULT_ANOMALY_THRESHOLD_BPS: u32 = 7_000;

/// Audit log maksimum boyutu.
pub const MAX_AUDIT_LOG_SIZE: usize = 10_000;

// ─── FreezeId ────────────────────────────────────────────────────────────────

/// Freeze işlemi benzersiz kimliği — blake3(account || slot || reason_tag).
pub type FreezeId = [u8; 32];

/// FreezeId'yi insan okunabilir hex stringe çevirir.
pub fn freeze_id_to_hex(id: &FreezeId) -> String {
    hex::encode(id)
}

// ─── FreezeRecord ─────────────────────────────────────────────────────────────

/// Aktif bir freeze işleminin kaydı.
///
/// Pattern: Polkadot assets-freezer `IdAmount<FreezeReason, Balance>` — her
/// freeze sebebi kendi kaydını tutar, birden fazla aktif olabilir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreezeRecord {
    /// Benzersiz freeze ID'si.
    pub freeze_id: FreezeId,
    /// Dondurulmuş hesap.
    pub account: Address,
    /// Dondurma sebebi (nova-account FreezeReason).
    pub reason: FreezeReason,
    /// İşlemi başlatan adres (kullanıcı, guardian, anomaly detector).
    pub initiator: Address,
    /// Dondurma başlangıç slot'u.
    pub initiated_at_slot: SlotId,
    /// Dondurma başlangıç zamanı (unix timestamp saniye).
    pub initiated_at_timestamp: u64,
}

impl FreezeRecord {
    /// Guardian threshold freeze mi?
    pub fn is_guardian_freeze(&self) -> bool {
        matches!(self.reason, FreezeReason::GuardianThreshold { .. })
    }

    /// Kullanıcı kendi başlattı mı?
    pub fn is_user_initiated(&self) -> bool {
        matches!(self.reason, FreezeReason::UserInitiated)
    }

    /// Anomali kaynaklı mı?
    pub fn is_anomaly_freeze(&self) -> bool {
        matches!(self.reason, FreezeReason::AnomalyDetected { .. })
    }

    /// İmzalayan guardian'ları döner (guardian freeze ise).
    pub fn guardian_signers(&self) -> Option<&[Address]> {
        if let FreezeReason::GuardianThreshold { signers } = &self.reason {
            Some(signers)
        } else {
            None
        }
    }
}

// ─── FreezePhase ─────────────────────────────────────────────────────────────

/// Freeze soruşturma aşaması.
///
/// Pattern: Solana address-lookup-table `LookupTableStatus` state machine —
/// `Active → Deactivating → Deactivated` benzeri, burada:
/// `Active → UserCancellationWindow | InvestigationOpen → Resolved`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreezePhase {
    /// Freeze aktif, henüz pencere açılmadı.
    FreezeActive,
    /// Kullanıcı iptal penceresi açık (24 saat — guardian freeze sonrası).
    /// Pattern: Whitepaper §5.5.2 "24 saat kullanıcı iptal edebilir".
    UserCancellationWindow {
        expires_at_slot: SlotId,
    },
    /// 72 saatlik tam soruşturma penceresi açık.
    /// Pattern: Whitepaper §5.5.3 "72 saatlik soruşturma penceresi".
    InvestigationOpen {
        freeze_id: FreezeId,
        started_at_slot: SlotId,
        expires_at_slot: SlotId,
    },
    /// Soruşturma penceresi doldu, miras protokolüne devredilecek.
    TimedOut {
        frozen_since_slot: SlotId,
    },
    /// Çözüldü (unfreeze edildi veya handoff yapıldı).
    Resolved {
        resolved_at_slot: SlotId,
        outcome: UnfreezeOutcome,
    },
}

impl FreezePhase {
    /// Freeze hâlâ aktif mi (çözülmedi)?
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Resolved { .. })
    }

    /// Soruşturma penceresi belirli bir slot'ta açık mı?
    pub fn is_investigation_open(&self, current_slot: SlotId) -> bool {
        match self {
            Self::InvestigationOpen { expires_at_slot, .. } => current_slot < *expires_at_slot,
            _ => false,
        }
    }

    /// Kullanıcı iptal penceresi açık mı?
    pub fn is_cancellation_open(&self, current_slot: SlotId) -> bool {
        match self {
            Self::UserCancellationWindow { expires_at_slot } => current_slot < *expires_at_slot,
            _ => false,
        }
    }
}

// ─── FreezeState ─────────────────────────────────────────────────────────────

/// Bir hesabın anlık freeze durumu.
///
/// Pattern: Polkadot assets-freezer `FrozenBalances` + `Freezes` ayrımı —
/// aktif freeze(ler) + toplam durum ayrı tutulur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeState {
    /// Hesap adresi.
    pub account: Address,
    /// Hesap şu an dondurulmuş mu?
    pub is_frozen: bool,
    /// Aktif freeze kaydı (varsa).
    pub active_freeze: Option<FreezeRecord>,
    /// Soruşturma aşaması.
    pub phase: FreezePhase,
    /// Bu hesabın toplam freeze sayısı (tüm zamanlar).
    pub total_freeze_count: u32,
}

impl FreezeState {
    /// Yeni boş freeze state — aktif, dondurulmamış.
    pub fn new(account: Address) -> Self {
        Self {
            account,
            is_frozen: false,
            active_freeze: None,
            phase: FreezePhase::FreezeActive,
            total_freeze_count: 0,
        }
    }

    /// Freeze state'i dondurulmuş olarak ayarla.
    pub fn apply_freeze(&mut self, record: FreezeRecord) {
        self.is_frozen = true;
        self.total_freeze_count += 1;
        let is_guardian = record.is_guardian_freeze();
        let slot = record.initiated_at_slot;
        self.active_freeze = Some(record);
        if is_guardian {
            self.phase = FreezePhase::UserCancellationWindow {
                expires_at_slot: SlotId(slot.0 + USER_CANCELLATION_WINDOW_SLOTS),
            };
        } else {
            self.phase = FreezePhase::FreezeActive;
        }
    }

    /// Soruşturma penceresini açar.
    pub fn open_investigation(&mut self, current_slot: SlotId) {
        if let Some(ref record) = self.active_freeze {
            self.phase = FreezePhase::InvestigationOpen {
                freeze_id: record.freeze_id,
                started_at_slot: current_slot,
                expires_at_slot: SlotId(current_slot.0 + INVESTIGATION_WINDOW_SLOTS),
            };
        }
    }

    /// Freeze'i çöz.
    pub fn resolve(&mut self, outcome: UnfreezeOutcome, resolved_at_slot: SlotId) {
        self.is_frozen = false;
        self.active_freeze = None;
        self.phase = FreezePhase::Resolved {
            resolved_at_slot,
            outcome,
        };
    }

    /// Aktif freeze ID'sini döner.
    pub fn active_freeze_id(&self) -> Option<&FreezeId> {
        self.active_freeze.as_ref().map(|r| &r.freeze_id)
    }
}

// ─── ThresholdProof ───────────────────────────────────────────────────────────

/// MPC threshold imzası kanıtı — unfreeze işlemi için gerekli.
///
/// NOVA-RED-FREEZE-002 DÜZELTMESİ: Önceki BLAKE3 commitment tabanlı "proof"
/// kriptografik olarak güvensizdi — herkes signer listesini bilerek geçerli
/// proof üretebiliyordu (imza gerekmiyordu).
///
/// Yeni tasarım: Gerçek BLS aggregate signature.
/// - Her guardian mesajı kendi BLS private key'i ile imzalar.
/// - Tüm imzalar aggregate edilir (`Σ sig_i`).
/// - Doğrulama: `BlsVerifier::verify_aggregate_same_msg(msg, [sig_agg], [pk_i])`
/// - Bir guardian'ın private key'i olmadan geçerli proof üretilemez.
///
/// Pattern: Ethereum beacon chain BLS aggregate (Altair) — n validator imzası
/// tek aggregate imzaya sıkıştırılır, O(1) doğrulama.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdProof {
    /// İmzalayan guardian/party adresleri (sıralı).
    pub signers: Vec<Address>,
    /// Her imzalayanın BLS public key'i (signers ile birebir hizalı).
    pub public_keys: Vec<BlsPublicKey>,
    /// İmzalanan mesajın hash'i.
    /// Format: blake3("nova_threshold_proof_v2" || account || freeze_id || slot_le_bytes)
    pub message_hash: [u8; 32],
    /// Aggregate BLS signature: Σ bls_sign(secret_key_i, message_hash)
    pub aggregate_signature: BlsSignature,
    /// Proof oluşturulduğu slot.
    pub signed_at_slot: SlotId,
}

impl ThresholdProof {
    /// Mesaj hash'ini hesaplar.
    ///
    /// "nova_threshold_proof_v2" domain separator ile önceki versiyondan
    /// farklılaştırılmıştır (NOVA-RED-FREEZE-002 replay önlemi).
    pub fn compute_message_hash(account: &Address, freeze_id: &FreezeId, slot: SlotId) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova_threshold_proof_v2");
        hasher.update(account.as_bytes());
        hasher.update(freeze_id.as_slice());
        hasher.update(&slot.0.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Proof'u doğrular — BLS aggregate imzasını kriptografik olarak doğrular.
    ///
    /// Kontroller:
    /// 1. Mesaj hash'i beklenen değerle eşleşiyor mu?
    /// 2. `public_keys.len() == signers.len()` (her imzalayan için bir pubkey)
    /// 3. Aggregate BLS imzası tüm public key'lere karşı geçerli mi?
    ///
    /// Bir guardian'ın BLS private key'i olmadan bu doğrulamayı geçmek
    /// matematiksel olarak imkânsızdır (BLS12-381 güvenliği: 128-bit).
    pub fn verify(&self, account: &Address, freeze_id: &FreezeId) -> bool {
        // 1. Mesaj hash kontrolü
        let expected_msg = Self::compute_message_hash(account, freeze_id, self.signed_at_slot);
        if expected_msg != self.message_hash {
            return false;
        }

        // 2. public_keys / signers sayısı eşleşmeli
        if self.public_keys.len() != self.signers.len() || self.public_keys.is_empty() {
            return false;
        }

        // 3. Aggregate BLS doğrulama
        let pk_refs: Vec<&BlsPublicKey> = self.public_keys.iter().collect();
        let sig_refs = [&self.aggregate_signature];
        BlsVerifier::verify_aggregate_same_msg(
            &self.message_hash,
            &sig_refs,
            &pk_refs,
        ).is_ok()
    }

    /// Gerçek BLS imzalarıyla proof oluşturur.
    ///
    /// `guardian_ikm_seeds`: Her guardian için deterministik IKM (32 byte).
    /// Her guardian kendi BLS keypair'ini IKM'den türetir, mesajı imzalar.
    /// Aggregate imza oluşturulur ve proof döndürülür.
    pub fn create_with_bls_keys(
        account: &Address,
        freeze_id: &FreezeId,
        signers: Vec<Address>,
        guardian_ikm_seeds: &[[u8; 32]],
        slot: SlotId,
    ) -> Result<Self, String> {
        if signers.len() != guardian_ikm_seeds.len() {
            return Err("signers ve ikm_seeds sayısı eşit olmalı".to_string());
        }

        let message_hash = Self::compute_message_hash(account, freeze_id, slot);

        let mut public_keys = Vec::with_capacity(signers.len());
        let mut bls_signatures = Vec::with_capacity(signers.len());

        for ikm in guardian_ikm_seeds {
            let kp = BlsKeypair::from_ikm(ikm)
                .map_err(|e| format!("BLS keypair türetme hatası: {e}"))?;
            public_keys.push(kp.bls_public_key());
            bls_signatures.push(kp.sign_raw(&message_hash));
        }

        // Aggregate signature oluştur
        use blst::min_pk::{AggregateSignature, Signature as BlstSig};
        let blst_sigs: Vec<BlstSig> = bls_signatures.iter()
            .map(|s| BlstSig::from_bytes(s.as_bytes())
                .map_err(|e| format!("BlstSig dönüşüm hatası: {e:?}")))
            .collect::<Result<_, _>>()?;

        let sig_refs: Vec<&BlstSig> = blst_sigs.iter().collect();
        let mut agg = AggregateSignature::from_signature(sig_refs[0]);
        for s in &sig_refs[1..] {
            agg.add_signature(s, true)
                .map_err(|e| format!("aggregate hatası: {e:?}"))?;
        }
        let final_sig = agg.to_signature();
        let aggregate_signature = BlsSignature::from_bytes(final_sig.to_bytes());

        Ok(Self {
            signers,
            public_keys,
            message_hash,
            aggregate_signature,
            signed_at_slot: slot,
        })
    }

    /// Test kolaylığı — deterministik IKM seed'ler üretir (signers adreslerinden).
    ///
    /// Gerçek sistemde her guardian kendi private key'ini bilir;
    /// bu yardımcı yalnızca testlerde kullanılır.
    pub fn mock_valid(
        account: &Address,
        freeze_id: &FreezeId,
        signers: Vec<Address>,
        slot: SlotId,
    ) -> Self {
        // Her imzalayan için adresinden deterministik IKM türet
        let ikm_seeds: Vec<[u8; 32]> = signers.iter().map(|addr| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"nova-test-guardian-ikm-v1");
            hasher.update(addr.as_bytes());
            *hasher.finalize().as_bytes()
        }).collect();

        Self::create_with_bls_keys(account, freeze_id, signers, &ikm_seeds, slot)
            .expect("mock_valid: BLS proof oluşturma başarısız")
    }
}

// ─── OperationType ────────────────────────────────────────────────────────────

/// Hesap üzerinde gerçekleştirilebilecek işlem türleri.
///
/// Pattern: go-ethereum txpool `ValidationOptions.Accept` bitmap —
/// hangi işlem türleri kabul ediliyor/reddediliyor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    /// Token transferi (çıkış) — FREEZE'de REDDEDILIR.
    Transfer,
    /// Swap işlemi — FREEZE'de REDDEDILIR.
    Swap,
    /// Token alımı (giriş) — FREEZE'de İZİN VERİLİR.
    /// Whitepaper §5.5.3: "Hesaba giriş hâlâ mümkün".
    Receive,
    /// Freeze kaldırma — FREEZE'de İZİN VERİLİR.
    Unfreeze,
    /// Stake/delegation — FREEZE'de REDDEDILIR.
    StakeDelegate,
    /// Kontrat çağrısı — FREEZE'de REDDEDILIR.
    ContractCall,
    /// Diğer işlemler — FREEZE'de REDDEDILIR.
    Other,
}

impl OperationType {
    /// Dondurulmuş hesapta bu işlem izinli mi?
    pub fn is_allowed_when_frozen(&self) -> bool {
        matches!(self, Self::Receive | Self::Unfreeze)
    }
}

// ─── FreezeConfig ─────────────────────────────────────────────────────────────

/// FreezeEngine yapılandırması.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeConfig {
    /// Guardian freeze için gerekli minimum imzalayan sayısı.
    pub guardian_freeze_threshold: u8,
    /// Soruşturma penceresi slot sayısı (varsayılan: 648_000 = 72 saat).
    pub investigation_window_slots: u64,
    /// Kullanıcı iptal penceresi slot sayısı (varsayılan: 216_000 = 24 saat).
    pub user_cancellation_window_slots: u64,
    /// Anomali onay süresi (varsayılan: 1_500 = 10 dakika).
    pub anomaly_confirm_slots: u64,
    /// Anomali eşiği (bps cinsinden, varsayılan: 7_000 = 70%).
    pub anomaly_threshold_bps: u32,
}

impl Default for FreezeConfig {
    fn default() -> Self {
        Self {
            guardian_freeze_threshold: 2,
            investigation_window_slots: INVESTIGATION_WINDOW_SLOTS,
            user_cancellation_window_slots: USER_CANCELLATION_WINDOW_SLOTS,
            anomaly_confirm_slots: ANOMALY_CONFIRM_SLOTS,
            anomaly_threshold_bps: DEFAULT_ANOMALY_THRESHOLD_BPS,
        }
    }
}

// ─── UnfreezeOutcome ─────────────────────────────────────────────────────────

/// Freeze çözüm sonucu.
///
/// Pattern: Whitepaper §5.5.3 üç çözüm senaryosuna birebir karşılık gelir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnfreezeOutcome {
    /// Kullanıcı MPC threshold imzasıyla kimliğini kanıtladı → hesap açıldı.
    /// §5.5.3: "MPC threshold imzası ile kimlik doğrulama → Hesap anında açılır".
    UserIdentityProven { unfreeze_slot: SlotId },
    /// Saldırı doğrulandı → yeni guardian set ile hesap açıldı.
    /// §5.5.3: "Guardian'lar yeni cihazlara key share dağıtır".
    AttackConfirmed {
        new_guardian_set: Vec<Address>,
        unfreeze_slot: SlotId,
    },
    /// 72 saat doldu, miras protokolü devreye girdi.
    /// §5.5.3: "Kullanıcıya ulaşılamazsa → Miras protokolü devreye girer".
    TimedOutToInheritance { handoff_slot: SlotId },
    /// Kullanıcı guardian freeze'ini 24 saat içinde iptal etti.
    UserCancelled { cancelled_at_slot: SlotId },
}

// ─── FreezeEvent ─────────────────────────────────────────────────────────────

/// FreezeEngine tarafından yayınlanan event'ler.
///
/// Pattern: Polkadot assets-freezer `Event::Frozen | Event::Thawed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeEvent {
    pub account: Address,
    pub event_type: FreezeEventType,
    pub slot: SlotId,
}

/// Event türleri.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FreezeEventType {
    /// Hesap donduruldu.
    AccountFrozen {
        freeze_id: FreezeId,
        reason_summary: String,
    },
    /// Hesap açıldı.
    AccountUnfrozen {
        freeze_id: FreezeId,
        outcome: UnfreezeOutcome,
    },
    /// Soruşturma penceresi açıldı.
    InvestigationWindowOpened {
        freeze_id: FreezeId,
        expires_at_slot: SlotId,
    },
    /// Soruşturma penceresi doldu — miras protokolüne handoff.
    InvestigationWindowExpired { freeze_id: FreezeId },
    /// Kullanıcı iptal penceresi açıldı.
    UserCancellationWindowOpened {
        freeze_id: FreezeId,
        expires_at_slot: SlotId,
    },
    /// Guardian slashlandı.
    /// Pattern: Cosmos SDK slashing `EventTypeSlash`.
    GuardianSlashed {
        guardian: Address,
        slash_bps: u32,
        reason_freeze_id: FreezeId,
    },
    /// Guardian jailed.
    GuardianJailed {
        guardian: Address,
        jailed_until_slot: SlotId,
        wrong_freeze_count: u32,
    },
    /// Anomali tespit edildi — onay bekleniyor.
    AnomalyDetected {
        score_bps: u32,
        anomaly_type: AnomalyType,
        confirm_deadline_slot: SlotId,
    },
    /// Anomali onaylandı — freeze uygulandı.
    AnomalyConfirmed { score_bps: u32 },
    /// Guardian freeze yetkili listesi güncellendi.
    AuthorizedGuardiansUpdated { account: Address, count: usize },
}

// ─── PendingAnomaly ───────────────────────────────────────────────────────────

/// Onay bekleyen anomali kaydı.
///
/// Pattern: Polkadot im-online `HeartbeatStatus { session_index, sent_at }` —
/// "bekliyor mu, ne zamandan beri?" sorusunu yanıtlar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAnomaly {
    /// Anomali skoru (bps).
    pub score_bps: u32,
    /// Anomali türü.
    pub anomaly_type: AnomalyType,
    /// Tespit edildiği slot.
    pub detected_at_slot: SlotId,
    /// Onay son slot (detected + anomaly_confirm_slots).
    pub confirm_deadline_slot: SlotId,
}

impl PendingAnomaly {
    /// Onay süresi doldu mu?
    pub fn is_deadline_passed(&self, current_slot: SlotId) -> bool {
        current_slot >= self.confirm_deadline_slot
    }
}

// ─── FreezeAuditEntry ────────────────────────────────────────────────────────

/// Denetim logu girişi — değiştirilemez.
///
/// Pattern: multi-party-ecdsa blame.rs `GlobalStatePhase5` — hesap verebilirlik
/// için tüm freeze olayları imutable şekilde loglanır.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeAuditEntry {
    /// Sıra numarası (monoton artan).
    pub seq: u64,
    /// Freeze ID.
    pub freeze_id: FreezeId,
    /// Etkilenen hesap.
    pub account: Address,
    /// İşlemi başlatan adres.
    pub initiator: Address,
    /// Freeze sebebi.
    pub reason: FreezeReason,
    /// Slot.
    pub slot: SlotId,
    /// Çözüm kaydı (henüz çözülmediyse None).
    pub outcome: Option<FreezeOutcomeRecord>,
}

/// Freeze çözüm kaydı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeOutcomeRecord {
    pub resolved_at_slot: SlotId,
    pub outcome: UnfreezeOutcome,
    /// Bu freeze yanlış mıydı? (guardian slashing için)
    pub was_wrong_freeze: bool,
}

// ─── FreezeSummary ────────────────────────────────────────────────────────────

/// Engine istatistik özeti.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeSummary {
    pub total_frozen_accounts: usize,
    pub total_freeze_events: u64,
    pub total_guardian_slashes: u64,
    pub total_wrong_freezes: u64,
    pub slots_processed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_freeze_id(seed: u8) -> FreezeId {
        [seed; 32]
    }

    // ─── OperationType ────────────────────────────────────────────────────────

    #[test]
    fn test_operation_type_allowed_when_frozen() {
        // Whitepaper §5.5.3: sadece Receive ve Unfreeze izinli
        assert!(!OperationType::Transfer.is_allowed_when_frozen());
        assert!(!OperationType::Swap.is_allowed_when_frozen());
        assert!(OperationType::Receive.is_allowed_when_frozen());
        assert!(OperationType::Unfreeze.is_allowed_when_frozen());
        assert!(!OperationType::StakeDelegate.is_allowed_when_frozen());
        assert!(!OperationType::ContractCall.is_allowed_when_frozen());
        assert!(!OperationType::Other.is_allowed_when_frozen());
    }

    // ─── FreezeRecord ─────────────────────────────────────────────────────────

    #[test]
    fn test_freeze_record_reason_helpers() {
        let user_record = FreezeRecord {
            freeze_id: make_freeze_id(1),
            account: addr(1),
            reason: FreezeReason::UserInitiated,
            initiator: addr(1),
            initiated_at_slot: SlotId(100),
            initiated_at_timestamp: 1_000_000,
        };
        assert!(user_record.is_user_initiated());
        assert!(!user_record.is_guardian_freeze());
        assert!(!user_record.is_anomaly_freeze());

        let guardian_record = FreezeRecord {
            freeze_id: make_freeze_id(2),
            account: addr(2),
            reason: FreezeReason::GuardianThreshold {
                signers: vec![addr(10), addr(11)],
            },
            initiator: addr(10),
            initiated_at_slot: SlotId(200),
            initiated_at_timestamp: 2_000_000,
        };
        assert!(guardian_record.is_guardian_freeze());
        assert_eq!(guardian_record.guardian_signers().unwrap().len(), 2);

        let anomaly_record = FreezeRecord {
            freeze_id: make_freeze_id(3),
            account: addr(3),
            reason: FreezeReason::AnomalyDetected {
                anomaly_score_bps: 8500,
                anomaly_type: AnomalyType::RapidMultipleWithdrawals,
            },
            initiator: Address::ZERO,
            initiated_at_slot: SlotId(300),
            initiated_at_timestamp: 3_000_000,
        };
        assert!(anomaly_record.is_anomaly_freeze());
    }

    // ─── FreezePhase ──────────────────────────────────────────────────────────

    #[test]
    fn test_freeze_phase_active_states() {
        let active = FreezePhase::FreezeActive;
        assert!(active.is_active());
        assert!(!active.is_investigation_open(SlotId(100)));

        let cancel_window = FreezePhase::UserCancellationWindow {
            expires_at_slot: SlotId(1000),
        };
        assert!(cancel_window.is_active());
        assert!(cancel_window.is_cancellation_open(SlotId(500)));
        assert!(!cancel_window.is_cancellation_open(SlotId(1000)));
        assert!(!cancel_window.is_cancellation_open(SlotId(1001)));

        let inv_window = FreezePhase::InvestigationOpen {
            freeze_id: make_freeze_id(1),
            started_at_slot: SlotId(0),
            expires_at_slot: SlotId(648_000),
        };
        assert!(inv_window.is_active());
        assert!(inv_window.is_investigation_open(SlotId(100)));
        assert!(!inv_window.is_investigation_open(SlotId(648_000)));

        let resolved = FreezePhase::Resolved {
            resolved_at_slot: SlotId(200),
            outcome: UnfreezeOutcome::UserIdentityProven { unfreeze_slot: SlotId(200) },
        };
        assert!(!resolved.is_active());
    }

    // ─── FreezeState ──────────────────────────────────────────────────────────

    #[test]
    fn test_freeze_state_apply_freeze() {
        let mut state = FreezeState::new(addr(1));
        assert!(!state.is_frozen);
        assert_eq!(state.total_freeze_count, 0);

        let record = FreezeRecord {
            freeze_id: make_freeze_id(1),
            account: addr(1),
            reason: FreezeReason::UserInitiated,
            initiator: addr(1),
            initiated_at_slot: SlotId(100),
            initiated_at_timestamp: 1000,
        };
        state.apply_freeze(record);

        assert!(state.is_frozen);
        assert_eq!(state.total_freeze_count, 1);
        assert!(state.active_freeze.is_some());
        // Kullanıcı freeze → FreezeActive (guardian değil)
        assert_eq!(state.phase, FreezePhase::FreezeActive);
    }

    #[test]
    fn test_freeze_state_guardian_freeze_opens_cancellation_window() {
        let mut state = FreezeState::new(addr(2));
        let record = FreezeRecord {
            freeze_id: make_freeze_id(2),
            account: addr(2),
            reason: FreezeReason::GuardianThreshold {
                signers: vec![addr(10), addr(11)],
            },
            initiator: addr(10),
            initiated_at_slot: SlotId(500),
            initiated_at_timestamp: 5000,
        };
        state.apply_freeze(record);

        // Guardian freeze → UserCancellationWindow otomatik açılır
        assert!(matches!(state.phase, FreezePhase::UserCancellationWindow { .. }));
        if let FreezePhase::UserCancellationWindow { expires_at_slot } = state.phase {
            assert_eq!(expires_at_slot.0, 500 + USER_CANCELLATION_WINDOW_SLOTS);
        }
    }

    #[test]
    fn test_freeze_state_resolve() {
        let mut state = FreezeState::new(addr(3));
        let record = FreezeRecord {
            freeze_id: make_freeze_id(3),
            account: addr(3),
            reason: FreezeReason::UserInitiated,
            initiator: addr(3),
            initiated_at_slot: SlotId(100),
            initiated_at_timestamp: 1000,
        };
        state.apply_freeze(record);
        assert!(state.is_frozen);

        state.resolve(
            UnfreezeOutcome::UserIdentityProven { unfreeze_slot: SlotId(500) },
            SlotId(500),
        );
        assert!(!state.is_frozen);
        assert!(state.active_freeze.is_none());
        assert!(!state.phase.is_active());
    }

    // ─── ThresholdProof ───────────────────────────────────────────────────────

    #[test]
    fn test_threshold_proof_mock_valid_verifies() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let signers = vec![addr(10), addr(11), addr(12)];
        let slot = SlotId(1000);

        let proof = ThresholdProof::mock_valid(&account, &freeze_id, signers, slot);
        assert!(proof.verify(&account, &freeze_id), "mock proof doğrulanmalı");
    }

    #[test]
    fn test_threshold_proof_wrong_account_fails() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let signers = vec![addr(10), addr(11)];
        let slot = SlotId(1000);

        let proof = ThresholdProof::mock_valid(&account, &freeze_id, signers, slot);
        let wrong_account = addr(99);
        assert!(!proof.verify(&wrong_account, &freeze_id), "yanlış hesap reddedilmeli");
    }

    #[test]
    fn test_threshold_proof_wrong_freeze_id_fails() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let wrong_id = make_freeze_id(99);
        let signers = vec![addr(10), addr(11)];
        let slot = SlotId(1000);

        let proof = ThresholdProof::mock_valid(&account, &freeze_id, signers, slot);
        assert!(!proof.verify(&account, &wrong_id), "yanlış freeze_id reddedilmeli");
    }

    #[test]
    fn test_threshold_proof_signer_order_independent() {
        let account = addr(1);
        let freeze_id = make_freeze_id(1);
        let slot = SlotId(1000);

        // Farklı sıralarda aynı imzalayanlar → aynı commitment (sort yapılıyor)
        let proof1 = ThresholdProof::mock_valid(
            &account,
            &freeze_id,
            vec![addr(10), addr(11), addr(12)],
            slot,
        );
        let proof2 = ThresholdProof::mock_valid(
            &account,
            &freeze_id,
            vec![addr(12), addr(10), addr(11)],
            slot,
        );
        // Deterministik IKM'den üretilen keypair → aynı signers → aynı aggregate imza
        assert_eq!(proof1.aggregate_signature, proof2.aggregate_signature);
    }

    // ─── Sabitler ─────────────────────────────────────────────────────────────

    #[test]
    fn test_slot_constants_match_wall_clock() {
        // 72 saat = 648_000 slot @ 400ms
        let expected_72h = 72u64 * 3600 * 1000 / SLOT_DURATION_MS;
        assert_eq!(INVESTIGATION_WINDOW_SLOTS, expected_72h);

        // 24 saat = 216_000 slot
        let expected_24h = 24u64 * 3600 * 1000 / SLOT_DURATION_MS;
        assert_eq!(USER_CANCELLATION_WINDOW_SLOTS, expected_24h);

        // 10 dakika = 1_500 slot
        let expected_10min = 10u64 * 60 * 1000 / SLOT_DURATION_MS;
        assert_eq!(ANOMALY_CONFIRM_SLOTS, expected_10min);

        // 30 gün = 6_480_000 slot
        let expected_30d = 30u64 * 24 * 3600 * 1000 / SLOT_DURATION_MS;
        assert_eq!(GUARDIAN_JAIL_DURATION_SLOTS, expected_30d);
    }

    #[test]
    fn test_freeze_id_to_hex_length() {
        let id: FreezeId = [0xAB; 32];
        let hex_str = freeze_id_to_hex(&id);
        assert_eq!(hex_str.len(), 64);
    }
}
