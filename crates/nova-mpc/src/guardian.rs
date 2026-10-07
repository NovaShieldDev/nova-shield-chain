//! Guardian tipleri ve yönetimi.
//!
//! §5.1.5: Guardian seçimi ve yönetimi.
//! §5.8: Guardian Marketplace ve İtibar Sistemi.
//! §6.3: Guardian cihaz güvenliği — Secure Enclave zorunluluğu.
//!
//! ## Guardian Nedir?
//!
//! Guardian, kullanıcının MPC hesabındaki anahtar paylarından birini tutan
//! ve threshold imza protokolüne katılan taraftır. 3/5 varsayılan yapıda
//! beş guardian vardır:
//!
//! | Pay | Konum | Açıklama |
//! |-----|-------|----------|
//! | K1  | Telefon | Kullanıcının birincil cihazı |
//! | K2  | Laptop  | İkincil cihaz |
//! | K3  | Guardian1 | Kullanıcının güvendiği kişi/kurum |
//! | K4  | Guardian2 | İkinci güvenilir taraf |
//! | K5  | Time-lock | Zincir üstünde kilitli vault |
//!
//! ## Trust Score
//!
//! Her guardian'ın zincir üstünde ölçülen puanı:
//! `f(stake, yıl, kurtarma_başarı, slash_geçmişi, yanıt_süresi, hesap_sayısı)`

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{MpcError, MpcResult};
use crate::types::{PartyIndex, SessionId};

// ─── GUARDIAN KATEGORİSİ ──────────────────────────────────────────────────────

/// §5.8.2: Guardian kategorileri.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
         BorshSerialize, BorshDeserialize)]
pub enum GuardianCategory {
    /// Kategori 1: Kişisel guardian — kullanıcının tanıdığı biri.
    /// On-chain itibar skoru düşük veya yok. Güven kişisel ilişkiye dayalı.
    Personal,

    /// Kategori 2: Sertifikalı bireysel guardian.
    /// NSC stake etmiş, on-chain geçmişi olan birey. Küçük ücret alabilir.
    CertifiedIndividual,

    /// Kategori 3: Kurumsal guardian.
    /// NSC lisanslı şirket/banka/saklama sağlayıcısı. KYC/AML uyumlu.
    /// SLA zincire yazılı. Sigorta fonuna katkı yapar.
    Institutional,

    /// Kategori 4: DAO guardian.
    /// Akıllı kontrat tabanlı. Yönetişim oyu ile karar. İnsan müdahalesi yok.
    Dao,

    /// Özel: Time-lock vault. Zincir üstünde kilitli, belirli süre sonra açılır.
    TimeLockVault { unlock_slot: u64 },

    /// Kullanıcının kendi cihazı (telefon veya laptop).
    UserDevice,
}

impl GuardianCategory {
    /// Bu kategorinin kurumsal katkı (sigorta fonu) gerektirip gerektirmediği.
    pub fn requires_insurance_contribution(&self) -> bool {
        matches!(self, GuardianCategory::Institutional)
    }

    /// Bu kategori için minimum NSC stake miktarı (NSC cinsinden, 18 decimal).
    /// 0 = stake gerekmez.
    pub fn minimum_stake_nsc(&self) -> u128 {
        match self {
            GuardianCategory::Personal => 0,
            GuardianCategory::UserDevice => 0,
            GuardianCategory::TimeLockVault { .. } => 0,
            GuardianCategory::CertifiedIndividual => 1_000 * 10u128.pow(18),   // 1,000 NSC
            GuardianCategory::Dao => 5_000 * 10u128.pow(18),                   // 5,000 NSC
            GuardianCategory::Institutional => 10_000 * 10u128.pow(18),        // 10,000 NSC
        }
    }
}

// ─── GUARDIAN DURUMU ──────────────────────────────────────────────────────────

/// Guardian'ın mevcut operasyonel durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
         BorshSerialize, BorshDeserialize)]
pub enum GuardianStatus {
    /// Aktif — imza protokolüne katılabilir.
    Active,

    /// Geçici askıya alındı — örn: 30 gün içinde 3 yanlış freeze girişimi.
    Suspended {
        /// Askıya alınma sebebi.
        reason: SuspensionReason,
        /// Askıya alındığı slot numarası.
        since_slot: u64,
        /// Aktif olabileceği minimum slot. 0 = süresiz.
        until_slot: u64,
    },

    /// Kullanıcı tarafından iptal edildi.
    Revoked { at_slot: u64 },

    /// Cihaz çalındı/kayboldu — key share geçersiz kılındı.
    Compromised { reported_at_slot: u64 },

    /// Resharing bekleniyor — key share henüz teslim alınmadı.
    PendingReshare,
}

/// Guardian askıya alınma sebebi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
         BorshSerialize, BorshDeserialize)]
pub enum SuspensionReason {
    /// Yanlış freeze girişimi (§5.8.5: %5 stake kesintisi).
    FalseFreeze,
    /// Protokolde kötü niyetli davranış tespit edildi.
    IdentifiableAbort,
    /// Uzun süreli yanıtsızlık — Trust Score düşüşü.
    LongOffline,
    /// Slashing cezası sonrası.
    Slashed,
}

// ─── TRUST SCORE ─────────────────────────────────────────────────────────────

/// §5.8.3: On-chain itibar skoru.
///
/// `TrustScore = f(stake, aktif_yıl, kurtarma_başarı, slash, yanıt_süresi, hesap)`
///
/// Tüm bileşenler normalize edilerek 0-1000 arası nihai skor üretir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
         BorshSerialize, BorshDeserialize)]
pub struct TrustScore {
    /// Nihai skor (0-1000).
    pub score: u16,

    /// Stake edilen NSC miktarı (18 decimal).
    pub stake_nsc: u64,

    /// Kaç epoch aktif (1 epoch = 24 saat).
    pub active_epochs: u32,

    /// Başarılı kurtarma sayısı.
    pub successful_recoveries: u32,

    /// Başarısız veya yanlış kurtarma sayısı.
    pub failed_recoveries: u32,

    /// Ortalama yanıt süresi (milisaniye).
    pub avg_response_ms: u32,

    /// Koruma altındaki hesap sayısı.
    pub accounts_guarding: u32,

    /// Toplam slash sayısı.
    pub slash_count: u8,
}

impl TrustScore {
    /// Yeni başlayan guardian için sıfır skor.
    pub fn zero() -> Self {
        Self {
            score: 0,
            stake_nsc: 0,
            active_epochs: 0,
            successful_recoveries: 0,
            failed_recoveries: 0,
            avg_response_ms: 0,
            accounts_guarding: 0,
            slash_count: 0,
        }
    }

    /// Kurtarma başarı oranı (0.0 - 1.0). `None` eğer hiç kurtarma yoksa.
    pub fn recovery_success_rate(&self) -> Option<f64> {
        let total = self.successful_recoveries + self.failed_recoveries;
        if total == 0 {
            None
        } else {
            Some(self.successful_recoveries as f64 / total as f64)
        }
    }

    /// Slash cezası alınmış mı?
    pub fn has_slash_history(&self) -> bool {
        self.slash_count > 0
    }

    /// Skoru yeniden hesapla (basitleştirilmiş — gerçek hesap zincir üstünde).
    ///
    /// Ağırlıklar (toplam 1000 puan):
    /// - Stake: 200 puan
    /// - Aktif süre: 150 puan
    /// - Kurtarma başarısı: 300 puan
    /// - Yanıt süresi: 200 puan
    /// - Hesap sayısı: 150 puan
    /// - Slash cezaları: -100 puan/slash (maksimum -300)
    pub fn recompute(&mut self) {
        let mut score: i32 = 0;

        // Stake bileşeni (200 puan max — 10,000 NSC ile max)
        let stake_score = (self.stake_nsc.min(10_000) as f64 / 10_000.0 * 200.0) as i32;
        score += stake_score;

        // Aktif süre (150 puan max — 365 epoch ile max)
        let active_score = (self.active_epochs.min(365) as f64 / 365.0 * 150.0) as i32;
        score += active_score;

        // Kurtarma başarısı (300 puan max)
        if let Some(rate) = self.recovery_success_rate() {
            score += (rate * 300.0) as i32;
        }

        // Yanıt süresi (200 puan max — <500ms = max, >10000ms = 0)
        if self.avg_response_ms > 0 {
            let rt = self.avg_response_ms.min(10_000) as f64;
            let rt_score = ((1.0 - rt / 10_000.0) * 200.0) as i32;
            score += rt_score.max(0);
        }

        // Hesap sayısı (150 puan max — 100 hesapla max)
        let acc_score = (self.accounts_guarding.min(100) as f64 / 100.0 * 150.0) as i32;
        score += acc_score;

        // Slash cezaları (-100/slash, max -300)
        let slash_penalty = (self.slash_count as i32 * 100).min(300);
        score -= slash_penalty;

        self.score = score.max(0).min(1000) as u16;
    }
}

// ─── GUARDIAN KEY SHARE ───────────────────────────────────────────────────────

/// Bir guardian'ın elindeki anahtar payı ve metadata.
///
/// §6.3: Key share Secure Enclave içinde saklanır, asla RAM'e çıkmaz.
/// Bu struct, bellekte tutulurken `ZeroizeOnDrop` ile güvence altına alınır.
#[derive(Debug, ZeroizeOnDrop)]
pub struct GuardianKeyShare {
    /// Bu key share'in ait olduğu parti indeksi.
    #[zeroize(skip)]
    pub party_index: PartyIndex,

    /// Shamir gizli payı `x_i` (32 byte). ZEROIZE ON DROP.
    pub secret_share: [u8; 32],

    /// Bu guardian'ın secp256k1 public key katkısı: `X_i = x_i * G`.
    #[zeroize(skip)]
    pub public_share: [u8; 33],

    /// Aggregate public key — tüm tarafların katkısının toplamı.
    #[zeroize(skip)]
    pub aggregate_public_key: [u8; 33],

    /// Threshold parametresi.
    #[zeroize(skip)]
    pub threshold: u8,

    /// Toplam parti sayısı.
    #[zeroize(skip)]
    pub party_count: u32,

    /// Keygen oturum ID'si — hangi oturumdan geldiğini belirler.
    #[zeroize(skip)]
    pub keygen_session_id: [u8; 32],
}

impl GuardianKeyShare {
    /// Key share geçerli mi?
    pub fn is_valid(&self) -> bool {
        // secret_share sıfır olmamalı (sıfır scalar imza oluşturamaz)
        self.secret_share != [0u8; 32]
            && self.threshold > 0
            && self.party_count >= self.threshold as u32
            && self.party_index.0 > 0
            && self.party_index.0 <= self.party_count
    }
}

// ─── GUARDIAN KAYDI ───────────────────────────────────────────────────────────

/// Zincir üstünde tutulan guardian kaydı.
///
/// Bu yapı serileştirilebilir ve zincire yazılabilir.
/// Gizli veri içermez — tüm gizli veriler `GuardianKeyShare`'de.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct GuardianRecord {
    /// Benzersiz guardian kimliği.
    pub guardian_id: u32,

    /// Guardian'ın zincir adresi (20 byte, Ethereum uyumlu).
    #[serde(with = "crate::serde_helpers::bytes_20")]
    pub address: [u8; 20],

    /// Guardian kategorisi.
    pub category: GuardianCategory,

    /// Mevcut durum.
    pub status: GuardianStatus,

    /// Trust score.
    pub trust_score: TrustScore,

    /// Yıllık ücret (NSC, 18 decimal). 0 = ücretsiz.
    pub annual_fee_nsc: u128,

    /// Başarılı kurtarma başına bonus (NSC, 18 decimal).
    pub recovery_bonus_nsc: u128,

    /// Göreve başladığı slot numarası.
    pub registered_at_slot: u64,

    /// Taahhüt edilen SLA (milisaniye). 0 = SLA yok.
    pub sla_response_ms: u32,

    /// Guardian'ın secp256k1 public key'i (MPC protokolü için, sıkıştırılmış 33 byte).
    /// Bu, keygen'de her guardian'ın katkısının public versiyonudur.
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub mpc_public_key: [u8; 33],

    /// Koruma kapsamındaki hesap sayısı (güncel).
    pub accounts_protected: u32,
}

impl GuardianRecord {
    /// Guardian aktif ve kullanılabilir mi?
    pub fn is_active(&self) -> bool {
        matches!(self.status, GuardianStatus::Active)
    }

    /// Bu guardian'ın freeze yetkisi var mı?
    /// §5.5: Freeze yetkisi olan guardian'lar ayrıca tanımlanabilir.
    pub fn can_freeze(&self) -> bool {
        self.is_active()
            && matches!(
                self.category,
                GuardianCategory::Institutional
                    | GuardianCategory::CertifiedIndividual
                    | GuardianCategory::Dao
            )
    }

    /// Slash cezası uygula.
    ///
    /// §5.8.5: Yanlış freeze → %5 stake kesintisi.
    /// 30 gün içinde 3 yanlış freeze → guardian yetkisi askıya alınır.
    pub fn apply_slash(
        &mut self,
        reason: SuspensionReason,
        current_slot: u64,
        slash_percent: u8,
    ) {
        // Stake'i azalt
        let slash_amount = self.trust_score.stake_nsc as u128
            * slash_percent as u128
            / 100;
        self.trust_score.stake_nsc = self.trust_score.stake_nsc
            .saturating_sub(slash_amount as u64);
        self.trust_score.slash_count =
            self.trust_score.slash_count.saturating_add(1);
        self.trust_score.recompute();

        // 3+ slash → askıya al
        if self.trust_score.slash_count >= 3 {
            self.status = GuardianStatus::Suspended {
                reason,
                since_slot: current_slot,
                until_slot: current_slot + 7 * 24 * 3600 * 1000 / 400, // 7 gün
            };
        }
    }
}

// ─── GUARDIAN YÖNETİMİ ────────────────────────────────────────────────────────

/// Guardian seti yöneticisi.
///
/// Bir kullanıcı hesabına ait guardian kayıtlarını tutar.
/// Threshold doğrulama ve guardian değişikliği işlemlerini yönetir.
#[derive(Debug)]
pub struct GuardianSet {
    /// Aktif guardian'lar.
    pub guardians: Vec<GuardianRecord>,

    /// Threshold — en az kaç guardian imzası gerekli.
    pub threshold: u8,

    /// Guardian değişikliği için bekleme süresi (slot).
    /// §5.1.5: Guardian değişikliği 7 günlük bekleme süresiyle gerçekleşir.
    pub change_delay_slots: u64,
}

impl GuardianSet {
    /// Yeni guardian seti oluştur.
    pub fn new(threshold: u8, change_delay_slots: u64) -> MpcResult<Self> {
        if threshold == 0 {
            return Err(MpcError::InvalidThreshold { t: threshold, n: 0 });
        }
        Ok(Self {
            guardians: Vec::new(),
            threshold,
            change_delay_slots,
        })
    }

    /// Aktif guardian sayısı.
    pub fn active_count(&self) -> usize {
        self.guardians.iter().filter(|g| g.is_active()).count()
    }

    /// Threshold karşılanıyor mu?
    pub fn threshold_met(&self) -> bool {
        self.active_count() >= self.threshold as usize
    }

    /// Guardian bul — ID ile.
    pub fn find_by_id(&self, guardian_id: u32) -> MpcResult<&GuardianRecord> {
        self.guardians
            .iter()
            .find(|g| g.guardian_id == guardian_id)
            .ok_or(MpcError::GuardianNotFound { id: guardian_id })
    }

    /// Guardian bul — mutable, ID ile.
    pub fn find_by_id_mut(&mut self, guardian_id: u32) -> MpcResult<&mut GuardianRecord> {
        self.guardians
            .iter_mut()
            .find(|g| g.guardian_id == guardian_id)
            .ok_or(MpcError::GuardianNotFound { id: guardian_id })
    }

    /// Guardian iptal et.
    ///
    /// §5.1.5: Değişiklik için mevcut threshold imzası + 7 gün bekleme gerekir.
    /// Bu fonksiyon sadece durumu günceller; gerçek bekleme zincir seviyesinde.
    pub fn revoke_guardian(&mut self, guardian_id: u32, at_slot: u64) -> MpcResult<()> {
        let g = self.find_by_id_mut(guardian_id)?;
        if matches!(g.status, GuardianStatus::Revoked { .. }) {
            return Err(MpcError::GuardianRevoked { id: guardian_id });
        }
        g.status = GuardianStatus::Revoked { at_slot };
        Ok(())
    }

    /// Aktif guardian'ların indekslerini döndür (1-tabanlı, threshold protokolü için).
    pub fn active_party_indices(&self) -> Vec<PartyIndex> {
        self.guardians
            .iter()
            .filter(|g| g.is_active())
            .enumerate()
            .map(|(i, _)| PartyIndex(i as u32 + 1))
            .collect()
    }

    /// En yüksek Trust Score'a sahip `n` guardian'ı seç.
    pub fn top_n_by_trust(&self, n: usize) -> Vec<&GuardianRecord> {
        let mut active: Vec<&GuardianRecord> = self
            .guardians
            .iter()
            .filter(|g| g.is_active())
            .collect();
        active.sort_by(|a, b| b.trust_score.score.cmp(&a.trust_score.score));
        active.into_iter().take(n).collect()
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(id: u32, category: GuardianCategory) -> GuardianRecord {
        GuardianRecord {
            guardian_id: id,
            address: [id as u8; 20],
            category,
            status: GuardianStatus::Active,
            trust_score: TrustScore::zero(),
            annual_fee_nsc: 0,
            recovery_bonus_nsc: 0,
            registered_at_slot: 0,
            sla_response_ms: 500,
            mpc_public_key: [0x02; 33],
            accounts_protected: 0,
        }
    }

    #[test]
    fn test_trust_score_recompute_zero() {
        let mut ts = TrustScore::zero();
        ts.recompute();
        assert_eq!(ts.score, 0, "sıfır veri ile skor 0 olmalı");
    }

    #[test]
    fn test_trust_score_max() {
        let mut ts = TrustScore {
            score: 0,
            stake_nsc: 10_000,
            active_epochs: 365,
            successful_recoveries: 100,
            failed_recoveries: 0,
            avg_response_ms: 1,
            accounts_guarding: 100,
            slash_count: 0,
        };
        ts.recompute();
        assert!(ts.score > 900, "tam dolu skor 900+ olmalı, got {}", ts.score);
    }

    #[test]
    fn test_trust_score_slash_penalty() {
        let mut ts = TrustScore {
            score: 0,
            stake_nsc: 10_000,
            active_epochs: 365,
            successful_recoveries: 100,
            failed_recoveries: 0,
            avg_response_ms: 100,
            accounts_guarding: 100,
            slash_count: 3,
        };
        ts.recompute();
        // 3 slash = -300 puan
        let mut ts_no_slash = TrustScore {
            slash_count: 0,
            ..ts
        };
        ts_no_slash.recompute();
        assert!(
            ts_no_slash.score as i32 - ts.score as i32 >= 270,
            "3 slash en az 270 puan düşürmeli"
        );
    }

    #[test]
    fn test_trust_score_recovery_rate() {
        let ts = TrustScore {
            successful_recoveries: 8,
            failed_recoveries: 2,
            ..TrustScore::zero()
        };
        let rate = ts.recovery_success_rate().unwrap();
        assert!((rate - 0.8).abs() < 1e-9, "başarı oranı 0.8 olmalı");
    }

    #[test]
    fn test_trust_score_no_recoveries() {
        let ts = TrustScore::zero();
        assert!(ts.recovery_success_rate().is_none(), "kurtarma yoksa None");
    }

    #[test]
    fn test_guardian_set_threshold() {
        let mut gs = GuardianSet::new(3, 0).unwrap();
        gs.guardians.push(make_record(1, GuardianCategory::Personal));
        gs.guardians.push(make_record(2, GuardianCategory::Personal));
        assert!(!gs.threshold_met(), "2 < 3, threshold karşılanmamalı");
        gs.guardians.push(make_record(3, GuardianCategory::Personal));
        assert!(gs.threshold_met(), "3 == 3, threshold karşılanmalı");
    }

    #[test]
    fn test_guardian_revoke() {
        let mut gs = GuardianSet::new(2, 0).unwrap();
        gs.guardians.push(make_record(1, GuardianCategory::Personal));
        gs.revoke_guardian(1, 1000).unwrap();
        assert!(matches!(
            gs.guardians[0].status,
            GuardianStatus::Revoked { at_slot: 1000 }
        ));
        // İkinci iptal hata vermeli
        assert!(gs.revoke_guardian(1, 2000).is_err());
    }

    #[test]
    fn test_guardian_not_found() {
        let gs = GuardianSet::new(2, 0).unwrap();
        assert!(gs.find_by_id(99).is_err());
    }

    #[test]
    fn test_guardian_top_n_by_trust() {
        let mut gs = GuardianSet::new(2, 0).unwrap();
        let mut r1 = make_record(1, GuardianCategory::CertifiedIndividual);
        r1.trust_score.score = 800;
        let mut r2 = make_record(2, GuardianCategory::Personal);
        r2.trust_score.score = 500;
        let mut r3 = make_record(3, GuardianCategory::Institutional);
        r3.trust_score.score = 950;
        gs.guardians.extend([r1, r2, r3]);
        let top2 = gs.top_n_by_trust(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].trust_score.score, 950);
        assert_eq!(top2[1].trust_score.score, 800);
    }

    #[test]
    fn test_guardian_apply_slash_suspends_after_3() {
        let mut gs = GuardianSet::new(2, 0).unwrap();
        gs.guardians.push(make_record(1, GuardianCategory::Institutional));
        let g = gs.find_by_id_mut(1).unwrap();
        // 3 slash → askıya al
        g.apply_slash(SuspensionReason::FalseFreeze, 100, 5);
        g.apply_slash(SuspensionReason::FalseFreeze, 200, 5);
        g.apply_slash(SuspensionReason::FalseFreeze, 300, 5);
        assert!(
            matches!(g.status, GuardianStatus::Suspended { .. }),
            "3 slash sonrası askıya alınmalı"
        );
    }

    #[test]
    fn test_guardian_category_minimum_stake() {
        assert_eq!(GuardianCategory::Personal.minimum_stake_nsc(), 0);
        assert!(
            GuardianCategory::Institutional.minimum_stake_nsc() > 0,
            "kurumsal guardian stake gerektirmeli"
        );
        assert!(
            GuardianCategory::Institutional.minimum_stake_nsc()
                > GuardianCategory::CertifiedIndividual.minimum_stake_nsc(),
            "kurumsal > sertifikalı bireysel"
        );
    }

    #[test]
    fn test_can_freeze() {
        let personal = make_record(1, GuardianCategory::Personal);
        let institutional = make_record(2, GuardianCategory::Institutional);
        // Personal guardian freeze yapamaz (sadece kurumsal/sertifikalı/dao)
        assert!(
            !personal.can_freeze(),
            "kişisel guardian freeze yapamaz"
        );
        assert!(
            institutional.can_freeze(),
            "kurumsal guardian freeze yapabilir"
        );
    }
}
