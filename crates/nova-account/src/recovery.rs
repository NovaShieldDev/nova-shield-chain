//! nova-account — Sosyal Kurtarma Protokolü
//!
//! Whitepaper §5.1.4 — Sosyal Kurtarma Mekanizması
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **Rolling Shutter** (`rolling-shutter/keyper/`):
//!   `DecryptionTrigger → keyper response → key reveal` akışı:
//!   Kurtarma talebi → guardian onay → 48 saat → yürütme.
//!   `EpochKG` (epoch key generation) → guardian'ların bağımsız key share gönderimi.
//!   `shcrypto.VerifyEpochSecretKeyShare()` → `RecoveryEngine::verify_approval_signature()`.
//!
//! **TSS-Lib** (`tss/party.go`):
//!   `Party.Start() → Update() → NextRound()` → kurtarma protokolünün çok-turlu yapısı.
//!   `BaseUpdate` recursive pattern → `RecoveryEngine::process_approval()`.
//!   `WaitingFor()` → `RecoveryEngine::pending_approvals()`.
//!
//! **Penumbra** (Key hierarchy):
//!   `FullViewingKey → IncomingViewingKey → Address` ayrımı →
//!   Kurtarma sırasında viewing key korunur, spending key reshare edilir.
//!
//! **Cosmos-SDK** (`x/auth/keeper/`):
//!   `SetAccount/GetAccount` Keeper pattern → `RecoveryEngine` metot yapısı.
//!   AnteHandler sequence kontrolü → kurtarma nonce doğrulama.
//!
//! **Multi-party ECDSA GG20** (`gg_2020/blame.rs`):
//!   Identifiable abort → kötü niyetli guardian tespiti.
//!   `blame.rs` → `RecoveryEngine::detect_colluding_guardians()`.
//!
//! **Polkadot-SDK** (GRANDPA finality):
//!   GrandpaJustification {round, commit, votes_ancestries} →
//!   `RecoveryProof {request_id, approvals, threshold_proof}`.

use std::collections::HashMap;
use tracing::{info, warn, debug};

use nova_crypto::{blake3_hash, EcdsaVerifier};
use nova_primitives::crypto::{Hash, PublicKey, Signature};

use crate::errors::{AccountError, AccountResult};
use crate::types::{
    AccountState, GuardianSet, RecoveryApproval, RecoveryRequest, RecoveryStatus,
    ThresholdConfig, RECOVERY_DELAY_SECS, RECOVERY_EXPIRY_SECS,
};
use nova_primitives::types::{Address, Hash32, SlotId};

// ─── SABITLER ─────────────────────────────────────────────────────────────────

/// Kurtarma kimlik doğrulama hash boyutu: 32 byte
const RECOVERY_ID_PREFIX: &str = "recovery-v1-";

/// Maksimum eşzamanlı kurtarma talebi sayısı
const MAX_CONCURRENT_RECOVERY_REQUESTS: usize = 1;

// ─── KURTARMA KANITI ─────────────────────────────────────────────────────────

/// Kurtarma kanıtı — Polkadot GrandpaJustification benzeri.
/// Kurtarma tamamlandığında zincire yazılan on-chain kanıt.
///
/// GrandpaJustification {round, commit, votes_ancestries} →
/// RecoveryProof {request_id, approvals, threshold_proof, new_address}
#[derive(Debug, Clone)]
pub struct RecoveryProof {
    /// Kurtarma talebi ID'si.
    pub request_id: String,
    /// Kurtarmayı onaylayan guardian imzaları (en az threshold kadar).
    pub approvals: Vec<RecoveryApproval>,
    /// Threshold imza kanıtı (guardian'ların birleşik imzası).
    pub threshold_proof: Vec<u8>,
    /// Yeni hesap adresi (kurtarma sonrası).
    pub new_address: Address,
    /// Kurtarma tamamlanma slot'u.
    pub completed_at_slot: SlotId,
}

// ─── KURTARMA TALEBI AYRI AŞAMASI ────────────────────────────────────────────

/// Kurtarma protokolü aşaması.
/// TSS-Lib Round interface'inden ilham alındı: `Start() → Update() → NextRound()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryPhase {
    /// Aşama 1: Onay toplama — guardianlardan imza bekleniyor.
    /// TSS-Lib: Round 1 — commitment toplama.
    CollectingApprovals,
    /// Aşama 2: 48 saat bekleme süresi — güvenlik penceresi.
    /// Rolling Shutter: block commit sonrası reveal window.
    WaitingPeriod {
        ready_since_slot: SlotId,
        executes_at_slot: SlotId,
    },
    /// Aşama 3: Yürütme hazır.
    ReadyToExecute,
    /// Tamamlandı.
    Completed,
    /// İptal edildi.
    Cancelled { reason: String },
}

// ─── KURTARMA MOTORU ─────────────────────────────────────────────────────────

/// Sosyal kurtarma protokolü motoru.
///
/// TSS-Lib BaseParty: Start/Update/Running pattern.
/// Rolling Shutter KeyperCore: merkezi koordinasyon.
/// Cosmos-SDK Keeper: iş mantığını durum yönetiminden ayır.
pub struct RecoveryEngine;

impl RecoveryEngine {
    // ─── TALEP BAŞLATMA ───────────────────────────────────────────────────

    /// Yeni kurtarma talebi başlat.
    ///
    /// §5.1.4:
    /// 1. Kullanıcı yeni cihazda kurtarma talebi başlatır
    /// 2. Guardian'lara bildirim gönderilir
    /// 3. Guardian'lar kimlik doğrulama yapar
    ///
    /// Rolling Shutter DecryptionTrigger: sequencer → keyperler bildirim.
    pub fn initiate_recovery(
        account: Address,
        requester_pubkey: Vec<u8>,
        new_guardian_set: Option<GuardianSet>,
        current_slot: SlotId,
        current_timestamp_secs: u64,
    ) -> AccountResult<RecoveryRequest> {
        // Public key geçerliliği
        if requester_pubkey.is_empty() {
            return Err(AccountError::InvalidParameter {
                param: "requester_pubkey".into(),
                reason: "boş olamaz".into(),
            });
        }

        // Kurtarma talebi ID üret (deterministik)
        let request_id = Self::generate_request_id(&account, current_slot, current_timestamp_secs);

        // 48 saat sonrası yürütme slot'u
        let delay_slots = RECOVERY_DELAY_SECS * 1000 / 400; // 400ms/slot
        let executes_at_slot = SlotId(current_slot.0 + delay_slots);

        // 30 gün sonrası geçerlilik sonu
        let expires_at_secs = current_timestamp_secs + RECOVERY_EXPIRY_SECS;

        let request = RecoveryRequest {
            request_id: request_id.clone(),
            account,
            requester_pubkey: requester_pubkey.clone(),
            requested_at_slot: current_slot,
            requested_at_secs: current_timestamp_secs,
            executes_at_slot,
            expires_at_secs,
            approvals: HashMap::new(),
            status: RecoveryStatus::Collecting,
            new_guardian_set,
        };

        info!(
            request_id = %request_id,
            account = %hex::encode(account.as_bytes()),
            executes_at_slot = executes_at_slot.0,
            "kurtarma talebi başlatıldı"
        );

        Ok(request)
    }

    // ─── ONAY İŞLEME ─────────────────────────────────────────────────────

    /// Guardian'ın kurtarma onayını işle.
    ///
    /// TSS-Lib BaseUpdate recursive pattern:
    /// - Onay geldi → sakla
    /// - Threshold sağlandı mı kontrol et → `CanProceed()`
    /// - Sağlandıysa → `NextRound()` (WaitingPeriod'a geç)
    ///
    /// Rolling Shutter: her keyper kendi epoch secret key share'ini gönderir.
    pub fn process_approval(
        request: &mut RecoveryRequest,
        guardian_id: impl Into<String>,
        guardian_signature: Vec<u8>,
        guardian_pubkey: &[u8],
        threshold: u8,
        current_slot: SlotId,
    ) -> AccountResult<RecoveryPhase> {
        let guardian_id = guardian_id.into();

        // Talep durumu kontrolü
        match &request.status {
            RecoveryStatus::CancelledByUser => {
                return Err(AccountError::RecoveryCancelledByUser {
                    request_id: request.request_id.clone(),
                });
            }
            RecoveryStatus::Completed => {
                return Err(AccountError::InternalError {
                    reason: "kurtarma zaten tamamlandı".into(),
                });
            }
            RecoveryStatus::Expired => {
                return Err(AccountError::RecoveryExpired {
                    request_id: request.request_id.clone(),
                });
            }
            _ => {}
        }

        // NOVA-RED-ACCOUNT-001: Gerçek kriptografik imza doğrulaması
        // Guardian, recovery approval mesajını kendi secp256k1 private key'i ile imzalar.
        // Boş imza veya kriptografik doğrulama başarısızlığı → reddedilir.
        if guardian_signature.is_empty() {
            return Err(AccountError::SignatureVerificationFailed {
                reason: format!("guardian {} boş imza gönderdi", guardian_id),
            });
        }

        // Requester hash'i hesapla (man-in-the-middle koruması)
        let requester_hash = Self::compute_requester_hash(&request.requester_pubkey);

        // Recovery approval mesajını hesapla ve imzayı doğrula
        Self::verify_approval_signature(
            &request.request_id,
            &request.account,
            &requester_hash,
            current_slot,
            &guardian_signature,
            guardian_pubkey,
            &guardian_id,
        )?;

        // Onayı kaydet
        let approval = RecoveryApproval {
            guardian_id: guardian_id.clone(),
            approved_at_slot: current_slot,
            signature: guardian_signature,
            requester_hash,
        };
        request.approvals.insert(guardian_id.clone(), approval);

        debug!(
            request_id = %request.request_id,
            guardian_id = %guardian_id,
            approval_count = request.approvals.len(),
            threshold = threshold,
            "kurtarma onayı eklendi"
        );

        // TSS-Lib CanProceed() benzeri: threshold karşılandı mı?
        if request.has_sufficient_approvals(threshold) {
            // TSS-Lib NextRound(): WaitingPeriod'a geç
            request.status = RecoveryStatus::WaitingPeriod;
            info!(
                request_id = %request.request_id,
                "threshold karşılandı — 48 saat bekleme başladı"
            );
            return Ok(RecoveryPhase::WaitingPeriod {
                ready_since_slot: current_slot,
                executes_at_slot: request.executes_at_slot,
            });
        }

        Ok(RecoveryPhase::CollectingApprovals)
    }

    // ─── YÜRÜTME ─────────────────────────────────────────────────────────

    /// Kurtarmayı yürüt — 48 saat geçtikten sonra.
    ///
    /// §5.1.4:
    /// 5. 48 saat bekleme süresi (güvenlik penceresi)
    /// 6. Bu süre içinde kullanıcı işlemi iptal edebilir
    /// 7. 48 saat sonra yeni cihazlara anahtar parçaları yeniden dağıtılır
    ///
    /// Rolling Shutter: DecryptionKey threshold sağlandıktan sonra reveal.
    pub fn execute_recovery(
        account_state: &mut AccountState,
        request: &mut RecoveryRequest,
        threshold: u8,
        current_slot: SlotId,
        current_timestamp_secs: u64,
    ) -> AccountResult<RecoveryProof> {
        // Talep geçerliliği
        if request.is_expired(current_timestamp_secs) {
            request.status = RecoveryStatus::Expired;
            return Err(AccountError::RecoveryExpired {
                request_id: request.request_id.clone(),
            });
        }

        // Threshold kontrolü
        if !request.has_sufficient_approvals(threshold) {
            return Err(AccountError::RecoveryInsufficientApprovals {
                approvals: request.approvals.len(),
                required: threshold as usize,
            });
        }

        // 48 saat bekleme kontrolü
        if !request.waiting_period_elapsed(current_slot) {
            let remaining_slots = request.executes_at_slot.0.saturating_sub(current_slot.0);
            let remaining_secs = remaining_slots * 400 / 1000;
            return Err(AccountError::RecoveryPeriodNotElapsed { remaining_seconds: remaining_secs });
        }

        // Kurtarma yürüt
        // 1. Guardian seti güncelle (varsa)
        if let Some(new_set) = request.new_guardian_set.take() {
            account_state.guardian_set = new_set;
            account_state.guardian_set.version += 1;
        }

        // 2. Nonce sıfırla (yeni cihaz — eski nonce geçersiz)
        // NOT: Gerçek implementasyonda nonce sıfırlanmaz; sadece yeni key share dağıtılır.
        // Cosmos-SDK: sequence değişmez, sadece pubkey güncellenir.

        // 3. Aktivite güncelle
        account_state.last_activity_slot = current_slot;
        account_state.last_updated_slot = current_slot;

        // 4. Talep durumunu güncelle
        request.status = RecoveryStatus::Completed;

        // Kanıt oluştur (Polkadot GrandpaJustification benzeri)
        let approvals: Vec<RecoveryApproval> = request.approvals.values().cloned().collect();
        let threshold_proof = Self::compute_threshold_proof(&approvals);

        let proof = RecoveryProof {
            request_id: request.request_id.clone(),
            approvals,
            threshold_proof,
            new_address: account_state.address,
            completed_at_slot: current_slot,
        };

        info!(
            request_id = %request.request_id,
            account = %account_state.address_hex(),
            slot = current_slot.0,
            "kurtarma başarıyla tamamlandı"
        );

        Ok(proof)
    }

    // ─── İPTAL ────────────────────────────────────────────────────────────

    /// Kullanıcı tarafından kurtarma talebi iptal et.
    ///
    /// §5.1.4: "48 saat bekleme süresi (güvenlik penceresi) —
    /// Bu süre içinde kullanıcı işlemi iptal edebilir"
    ///
    /// Güvenlik: Saldırgan kullanıcının hesabına kurtarma başlatırsa,
    /// kullanıcı 48 saat içinde iptal edebilir.
    pub fn cancel_recovery(
        request: &mut RecoveryRequest,
        canceller: Address,
    ) -> AccountResult<()> {
        match &request.status {
            RecoveryStatus::Completed => {
                return Err(AccountError::InternalError {
                    reason: "tamamlanmış kurtarma iptal edilemez".into(),
                });
            }
            RecoveryStatus::CancelledByUser => {
                return Err(AccountError::RecoveryCancelledByUser {
                    request_id: request.request_id.clone(),
                });
            }
            _ => {}
        }

        request.status = RecoveryStatus::CancelledByUser;
        warn!(
            request_id = %request.request_id,
            canceller = %hex::encode(canceller.as_bytes()),
            "kurtarma talebi iptal edildi"
        );
        Ok(())
    }

    // ─── KOLLUSİON TESPİTİ ───────────────────────────────────────────────

    /// Kötü niyetli guardian tespiti — GG20 blame.rs'den ilham alındı.
    ///
    /// GG20: `blame.rs` — identifiable abort, protocol sabote eden taraf tespit.
    /// Nova: guardian'ların aynı anda kurtarma onayı vermesi şüpheli mi?
    pub fn detect_suspicious_approvals(
        request: &RecoveryRequest,
        time_window_secs: u64,
    ) -> Vec<String> {
        let mut suspicious = Vec::new();

        // Çok kısa sürede gelen çok sayıda onay koordineli saldırı işareti olabilir
        let approval_slots: Vec<SlotId> = request.approvals.values()
            .map(|a| a.approved_at_slot)
            .collect();

        if approval_slots.len() < 2 {
            return suspicious;
        }

        let min_slot = approval_slots.iter().min().unwrap().0;
        let max_slot = approval_slots.iter().max().unwrap().0;
        let span_secs = (max_slot - min_slot) * 400 / 1000;

        // Tüm onaylar çok kısa sürede geldiyse şüpheli
        if span_secs < time_window_secs && approval_slots.len() >= 3 {
            warn!(
                span_secs = span_secs,
                approval_count = approval_slots.len(),
                "şüpheli koordineli kurtarma onayı tespiti"
            );
            for guardian_id in request.approvals.keys() {
                suspicious.push(guardian_id.clone());
            }
        }

        suspicious
    }

    // ─── YARDIMCI METODLAR ────────────────────────────────────────────────

    /// Kurtarma talebi ID üret (deterministik).
    fn generate_request_id(account: &Address, slot: SlotId, timestamp_secs: u64) -> String {
        let mut data = Vec::new();
        data.extend_from_slice(RECOVERY_ID_PREFIX.as_bytes());
        data.extend_from_slice(account.as_bytes());
        data.extend_from_slice(&slot.0.to_le_bytes());
        data.extend_from_slice(&timestamp_secs.to_le_bytes());
        let hash = blake3::hash(&data);
        format!("{}{}", RECOVERY_ID_PREFIX, hex::encode(&hash.as_bytes()[..16]))
    }

    /// Requester public key'i hash'le (man-in-the-middle koruması).
    pub fn compute_requester_hash(requester_pubkey: &[u8]) -> Hash32 {
        let hash = blake3::hash(requester_pubkey);
        Hash32::from_bytes(*hash.as_bytes())
    }

    /// Recovery approval mesajını hesapla.
    ///
    /// Format: blake3("nova_recovery_approval_v1" || request_id || account || requester_hash || slot_le)
    ///
    /// Domain separator "nova_recovery_approval_v1" replay saldırısını önler —
    /// başka bağlamlardaki imzalar burada geçerli değil.
    pub fn compute_approval_message(
        request_id: &str,
        account: &Address,
        requester_hash: &Hash32,
        slot: SlotId,
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova_recovery_approval_v1");
        hasher.update(request_id.as_bytes());
        hasher.update(account.as_bytes());
        hasher.update(requester_hash.as_bytes());
        hasher.update(&slot.0.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Guardian imzasını kriptografik olarak doğrula (NOVA-RED-ACCOUNT-001 düzeltmesi).
    ///
    /// `guardian_pubkey`: secp256k1 compressed public key (33 byte).
    /// `guardian_signature`: ECDSA imzası (65 byte: r||s||v).
    ///
    /// Önceki kod sadece `is_empty()` kontrolü yapıyordu — herhangi bir `vec![0xFF; 64]`
    /// gibi rastgele byte dizisi kabul ediliyordu. Bu düzeltme ile gerçek secp256k1
    /// ECDSA imzası zorunludur.
    fn verify_approval_signature(
        request_id: &str,
        account: &Address,
        requester_hash: &Hash32,
        slot: SlotId,
        guardian_signature: &[u8],
        guardian_pubkey: &[u8],
        guardian_id: &str,
    ) -> AccountResult<()> {
        // Public key boyutu kontrolü
        if guardian_pubkey.len() != 33 {
            return Err(AccountError::SignatureVerificationFailed {
                reason: format!(
                    "guardian {guardian_id}: pubkey {} byte (33 bekleniyordu)",
                    guardian_pubkey.len()
                ),
            });
        }

        // Public key'i ayrıştır
        let pk = PublicKey::from_slice(guardian_pubkey)
            .ok_or_else(|| AccountError::SignatureVerificationFailed {
                reason: format!("guardian {guardian_id}: geçersiz secp256k1 public key"),
            })?;

        // Approval mesajını hesapla
        let msg_bytes = Self::compute_approval_message(request_id, account, requester_hash, slot);
        let hash = Hash::from_bytes(msg_bytes);

        // İmzayı ayrıştır
        let sig = Signature::new_ecdsa(guardian_signature.to_vec());

        // Kriptografik doğrulama
        EcdsaVerifier::verify(&hash, &sig, &pk).map_err(|e| {
            AccountError::SignatureVerificationFailed {
                reason: format!("guardian {guardian_id}: ECDSA doğrulama başarısız — {e}"),
            }
        })
    }

    /// Threshold kanıtı hesapla — Polkadot GrandpaJustification.commit benzeri.
    fn compute_threshold_proof(approvals: &[RecoveryApproval]) -> Vec<u8> {
        let mut data = Vec::new();
        for approval in approvals {
            data.extend_from_slice(approval.guardian_id.as_bytes());
            data.extend_from_slice(&approval.signature);
            data.extend_from_slice(approval.requester_hash.as_bytes());
        }
        let hash = blake3::hash(&data);
        hash.as_bytes().to_vec()
    }

    /// Bekleyen onaylar listesi — TSS-Lib WaitingFor() benzeri.
    pub fn pending_approvals<'a>(
        request: &'a RecoveryRequest,
        all_guardian_ids: &'a [String],
    ) -> Vec<&'a str> {
        all_guardian_ids
            .iter()
            .filter(|id| !request.approvals.contains_key(*id))
            .map(|id| id.as_str())
            .collect()
    }

    /// Talep aktif mi? (süresi dolmadı ve iptal edilmedi)
    pub fn is_request_active(
        request: &RecoveryRequest,
        current_timestamp_secs: u64,
    ) -> bool {
        matches!(
            request.status,
            RecoveryStatus::Collecting | RecoveryStatus::WaitingPeriod
        ) && !request.is_expired(current_timestamp_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountConfig, AccountId, AccountState, GuardianSet, ThresholdConfig};
    use nova_crypto::{EcdsaKeypair};
    use nova_primitives::crypto::Hash;

    /// Test yardımcısı: Guardian için deterministik ECDSA keypair üret.
    /// `seed`: 0-255 arası tekil seed byte.
    fn make_guardian_keypair(seed: u8) -> EcdsaKeypair {
        let mut secret = [0u8; 32];
        secret[0] = seed;
        secret[1] = 0xAB; // ayırt edici
        EcdsaKeypair::from_secret_bytes(&secret).expect("test keypair oluşturulamadı")
    }

    /// Test yardımcısı: Guardian approval imzası oluştur.
    /// `request`: Kurtarma talebi.
    /// `seed`: Guardian için deterministik seed.
    /// Döner: (pubkey_bytes: Vec<u8>, signature_bytes: Vec<u8>)
    fn sign_approval(request: &RecoveryRequest, seed: u8, slot: SlotId) -> (Vec<u8>, Vec<u8>) {
        let kp = make_guardian_keypair(seed);
        let pk = kp.public_key().expect("pubkey alınamadı");
        let requester_hash = RecoveryEngine::compute_requester_hash(&request.requester_pubkey);
        let msg_bytes = RecoveryEngine::compute_approval_message(
            &request.request_id,
            &request.account,
            &requester_hash,
            slot,
        );
        let hash = Hash::from_bytes(msg_bytes);
        let sig = kp.sign_hash(&hash).expect("imzalama başarısız");
        (pk.as_bytes().to_vec(), sig.bytes.clone())
    }

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn requester_address() -> Address {
        Address::from_bytes([2u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1_000_000)
    }

    fn test_timestamp() -> u64 {
        1_700_000_000
    }

    fn make_account_state() -> AccountState {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        AccountState::genesis(
            AccountId::new(1),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        )
    }

    fn make_recovery_request() -> RecoveryRequest {
        RecoveryEngine::initiate_recovery(
            test_address(),
            vec![0x03; 33],
            None,
            test_slot(),
            test_timestamp(),
        ).unwrap()
    }

    // ─── Talep başlatma testleri ──────────────────────────────────────────

    #[test]
    fn recovery_talebi_baslatilir() {
        let req = make_recovery_request();
        assert!(!req.request_id.is_empty());
        assert!(req.request_id.starts_with(RECOVERY_ID_PREFIX));
        assert_eq!(req.status, RecoveryStatus::Collecting);
        assert!(req.approvals.is_empty());
    }

    #[test]
    fn recovery_talep_id_deterministik() {
        let r1 = RecoveryEngine::initiate_recovery(
            test_address(), vec![0x03; 33], None, test_slot(), test_timestamp(),
        ).unwrap();
        let r2 = RecoveryEngine::initiate_recovery(
            test_address(), vec![0x03; 33], None, test_slot(), test_timestamp(),
        ).unwrap();
        assert_eq!(r1.request_id, r2.request_id);
    }

    #[test]
    fn recovery_talep_id_farkli_slot_farkli() {
        let r1 = RecoveryEngine::initiate_recovery(
            test_address(), vec![0x03; 33], None, SlotId(1000), test_timestamp(),
        ).unwrap();
        let r2 = RecoveryEngine::initiate_recovery(
            test_address(), vec![0x03; 33], None, SlotId(1001), test_timestamp(),
        ).unwrap();
        assert_ne!(r1.request_id, r2.request_id);
    }

    #[test]
    fn recovery_bos_pubkey_reddedilir() {
        let result = RecoveryEngine::initiate_recovery(
            test_address(), vec![], None, test_slot(), test_timestamp(),
        );
        assert!(matches!(result, Err(AccountError::InvalidParameter { .. })));
    }

    #[test]
    fn recovery_yurutme_suresi_dogru_hesaplanir() {
        let req = make_recovery_request();
        let delay_slots = RECOVERY_DELAY_SECS * 1000 / 400;
        assert_eq!(req.executes_at_slot.0, test_slot().0 + delay_slots);
    }

    // ─── Onay işleme testleri ─────────────────────────────────────────────

    #[test]
    fn onay_eklenir() {
        let mut req = make_recovery_request();
        let (pk, sig) = sign_approval(&req, 1, test_slot());
        let phase = RecoveryEngine::process_approval(
            &mut req,
            "g1",
            sig,
            &pk,
            3, // threshold
            test_slot(),
        ).unwrap();
        assert_eq!(phase, RecoveryPhase::CollectingApprovals);
        assert_eq!(req.approvals.len(), 1);
    }

    #[test]
    fn threshold_saglandikta_waiting_period_baslar() {
        let mut req = make_recovery_request();
        for i in 0u8..3 {
            let guardian_id = format!("g{}", i);
            let (pk, sig) = sign_approval(&req, i + 10, test_slot());
            let phase = RecoveryEngine::process_approval(
                &mut req,
                &guardian_id,
                sig,
                &pk,
                3,
                test_slot(),
            ).unwrap();
            if i < 2 {
                assert_eq!(phase, RecoveryPhase::CollectingApprovals);
            } else {
                // 3. onayda threshold karşılandı
                assert!(matches!(phase, RecoveryPhase::WaitingPeriod { .. }));
                assert_eq!(req.status, RecoveryStatus::WaitingPeriod);
            }
        }
    }

    #[test]
    fn bos_imza_reddedilir() {
        let mut req = make_recovery_request();
        let (pk, _) = sign_approval(&req, 1, test_slot());
        let result = RecoveryEngine::process_approval(
            &mut req,
            "g1",
            vec![], // boş imza
            &pk,
            3,
            test_slot(),
        );
        assert!(matches!(result, Err(AccountError::SignatureVerificationFailed { .. })));
    }

    #[test]
    fn yanlis_imza_reddedilir() {
        // NOVA-RED-ACCOUNT-001 regression: Geçersiz imza reddedilmeli
        let mut req = make_recovery_request();
        let (pk, _correct_sig) = sign_approval(&req, 1, test_slot());
        // Başka bir keypair'in imzasını gönder → doğrulama başarısız
        let (_, wrong_sig) = sign_approval(&req, 99, test_slot());
        let result = RecoveryEngine::process_approval(
            &mut req,
            "g1",
            wrong_sig,
            &pk,
            3,
            test_slot(),
        );
        assert!(
            matches!(result, Err(AccountError::SignatureVerificationFailed { .. })),
            "Yanlış imza reddedilmeli"
        );
    }

    // ─── Yürütme testleri ─────────────────────────────────────────────────

    #[test]
    fn yurutme_bekleme_suresi_dolmadan_reddedilir() {
        let mut state = make_account_state();
        let mut req = make_recovery_request();
        // Threshold'u karşıla
        for i in 0u8..3 {
            let (pk, sig) = sign_approval(&req, i + 20, test_slot());
            RecoveryEngine::process_approval(
                &mut req, &format!("g{}", i), sig, &pk, 3, test_slot(),
            ).unwrap();
        }
        // Hemen yürütmeye çalış (48 saat dolmadan)
        let result = RecoveryEngine::execute_recovery(
            &mut state, &mut req, 3, SlotId(test_slot().0 + 1), test_timestamp(),
        );
        assert!(matches!(result, Err(AccountError::RecoveryPeriodNotElapsed { .. })));
    }

    #[test]
    fn yurutme_48_saat_sonra_basarili() {
        let mut state = make_account_state();
        let mut req = make_recovery_request();
        for i in 0u8..3 {
            let (pk, sig) = sign_approval(&req, i + 30, test_slot());
            RecoveryEngine::process_approval(
                &mut req, &format!("g{}", i), sig, &pk, 3, test_slot(),
            ).unwrap();
        }
        // 48 saat sonrası slot
        let execute_slot = SlotId(req.executes_at_slot.0);
        let proof = RecoveryEngine::execute_recovery(
            &mut state, &mut req, 3, execute_slot, test_timestamp(),
        ).unwrap();
        assert_eq!(proof.request_id, req.request_id);
        assert_eq!(proof.approvals.len(), 3);
        assert_eq!(req.status, RecoveryStatus::Completed);
    }

    #[test]
    fn yurutme_yetersiz_onay_reddedilir() {
        let mut state = make_account_state();
        let mut req = make_recovery_request();
        // Sadece 2 onay (threshold 3)
        for i in 0u8..2 {
            let (pk, sig) = sign_approval(&req, i + 40, test_slot());
            RecoveryEngine::process_approval(
                &mut req, &format!("g{}", i), sig, &pk, 3, test_slot(),
            ).unwrap();
        }
        let execute_slot = req.executes_at_slot;
        let result = RecoveryEngine::execute_recovery(
            &mut state, &mut req, 3, execute_slot, test_timestamp(),
        );
        assert!(matches!(result, Err(AccountError::RecoveryInsufficientApprovals { .. })));
    }

    #[test]
    fn yurutme_suresi_dolan_talep_reddedilir() {
        let mut state = make_account_state();
        let mut req = make_recovery_request();
        for i in 0u8..3 {
            let (pk, sig) = sign_approval(&req, i + 50, test_slot());
            RecoveryEngine::process_approval(
                &mut req, &format!("g{}", i), sig, &pk, 3, test_slot(),
            ).unwrap();
        }
        // 30 günden fazla sonra (süresi doldu)
        let expired_ts = test_timestamp() + RECOVERY_EXPIRY_SECS + 1;
        let execute_slot = req.executes_at_slot;
        let result = RecoveryEngine::execute_recovery(
            &mut state, &mut req, 3, execute_slot, expired_ts,
        );
        assert!(matches!(result, Err(AccountError::RecoveryExpired { .. })));
    }

    // ─── İptal testleri ───────────────────────────────────────────────────

    #[test]
    fn iptal_kullanici_tarafindan_calisiyor() {
        let mut req = make_recovery_request();
        let result = RecoveryEngine::cancel_recovery(&mut req, test_address());
        assert!(result.is_ok());
        assert_eq!(req.status, RecoveryStatus::CancelledByUser);
    }

    #[test]
    fn iptal_tamamlanmis_talep_reddedilir() {
        let mut req = RecoveryRequest {
            request_id: "r1".into(),
            account: test_address(),
            requester_pubkey: vec![0x03; 33],
            requested_at_slot: test_slot(),
            requested_at_secs: test_timestamp(),
            executes_at_slot: SlotId(test_slot().0 + 1000),
            expires_at_secs: test_timestamp() + RECOVERY_EXPIRY_SECS,
            approvals: HashMap::new(),
            status: RecoveryStatus::Completed,
            new_guardian_set: None,
        };
        let result = RecoveryEngine::cancel_recovery(&mut req, test_address());
        assert!(result.is_err());
    }

    // ─── Kolluziyon tespiti testleri ─────────────────────────────────────

    #[test]
    fn kolluziyon_cok_hizli_onayda_tespit_edilir() {
        let mut req = make_recovery_request();
        // Tüm onaylar aynı anda (şüpheli)
        for i in 0..3 {
            let approval = RecoveryApproval {
                guardian_id: format!("g{}", i),
                approved_at_slot: test_slot(), // AYNI SLOT
                signature: vec![0xFF; 64],
                requester_hash: Hash32::ZERO,
            };
            req.approvals.insert(format!("g{}", i), approval);
        }
        // 0 saniyeye bakıyoruz → hepsi aynı slot → şüpheli
        let suspicious = RecoveryEngine::detect_suspicious_approvals(&req, 60);
        assert_eq!(suspicious.len(), 3);
    }

    #[test]
    fn kolluziyon_normal_aralikta_tespit_edilmez() {
        let mut req = make_recovery_request();
        // Farklı slotlarda onaylar (normal)
        for i in 0..3 {
            let approval = RecoveryApproval {
                guardian_id: format!("g{}", i),
                approved_at_slot: SlotId(test_slot().0 + i as u64 * 10_000), // 1 saatlik aralık
                signature: vec![0xFF; 64],
                requester_hash: Hash32::ZERO,
            };
            req.approvals.insert(format!("g{}", i), approval);
        }
        // 60 saniye penceresinde → normal dağılım
        let suspicious = RecoveryEngine::detect_suspicious_approvals(&req, 60);
        assert!(suspicious.is_empty());
    }

    // ─── Bekleyen onaylar testleri ────────────────────────────────────────

    #[test]
    fn bekleyen_onaylar_dogru_hesaplanir() {
        let mut req = make_recovery_request();
        let (pk, sig) = sign_approval(&req, 60, test_slot());
        RecoveryEngine::process_approval(
            &mut req, "g1", sig, &pk, 3, test_slot(),
        ).unwrap();

        let all_ids: Vec<String> = vec!["g1".into(), "g2".into(), "g3".into()];
        let pending = RecoveryEngine::pending_approvals(&req, &all_ids);
        assert_eq!(pending.len(), 2);
        assert!(pending.contains(&"g2"));
        assert!(pending.contains(&"g3"));
        assert!(!pending.contains(&"g1"));
    }

    // ─── Aktiflik kontrolü testleri ──────────────────────────────────────

    #[test]
    fn aktif_talep_kontrolu() {
        let req = make_recovery_request();
        assert!(RecoveryEngine::is_request_active(&req, test_timestamp()));
    }

    #[test]
    fn suresi_dolan_talep_aktif_degil() {
        let req = make_recovery_request();
        let expired_ts = test_timestamp() + RECOVERY_EXPIRY_SECS + 1;
        assert!(!RecoveryEngine::is_request_active(&req, expired_ts));
    }
}
