//! Key Resharing Protokolü — Epoch Rotasyonu
//!
//! §5.4: "Epoch rotasyonu: her epoch başında guardian key payları reshare edilir.
//!        Yeni guardian seti aynı aggregate public key'i paylaşır; eski paylar geçersiz."
//!
//! §01-multi-party-ecdsa.md §4.5: Resharing protocol referans alındı.
//! §02-tss-lib.md §5 Resharing: tss-lib implementasyonu referans alındı.
//!
//! ## Protokol Özeti
//!
//! ```text
//! Eski Komite (threshold kadar üye):
//!   Round 1 (Old):
//!   - Her eski üye P_i:
//!     - f_i(x) = x_i + a_1*x + ... + a_{t_new-1}*x^{t_new-1}  [yeni VSS]
//!     - C_{i,k} = a_k * G  →  broadcast (VSS taahhüt)
//!     - DLog kanıtı: x_i bilindiğini kanıtla
//!     - Her yeni parti j için: AES-GCM-Encrypt(f_i(j))  →  P2P
//!
//! Yeni Komite:
//!   Round 2 (New):
//!   - Her yeni üye P'_j:
//!     - Tüm bundle'ları doğrula (DLog + VSS)
//!     - P2P payları şifreden çöz: f_i(j)
//!     - VSS doğrula: f_i(j) * G == ∑_k C_{i,k} * j^k
//!     - x'_j = ∑_{i∈old} λ_i · f_i(j)  →  yeni gizli pay
//!     - Public key teyit: X değişmedi
//!     - Yeni LocalKey kaydet
//! ```
//!
//! ## Güvenlik Garantisi
//!
//! - **Aynı sır**: `∑ λ'_j · x'_j = s` (orijinal sır korunur)
//! - **Public key sabit**: `X = s * G` değişmez
//! - **Forward secrecy**: Eski paylar kullanılamaz hale gelir
//! - **Identifiable abort**: Kötü niyetli eski üye tespit edilebilir
//!   (DLog kanıtı başarısız veya VSS uyuşmazlığı)

pub mod round1;
pub mod round2;

pub use round1::{
    run_reshare_round1, verify_reshare_public_key_consistency, verify_reshare_round1_bundle,
    ReshareRound1Bundle, ReshareRound1State,
};
pub use round2::{
    run_reshare_round2, verify_new_public_share_from_vss, verify_reshared_key_public,
};

use crate::errors::MpcResult;
use crate::p2p_encrypt::EcdhSecret;
use crate::types::{EcdhPublicKey, LocalKey, PartyIndex, SessionId};

// ─── RESHARE ÇIKTISI ──────────────────────────────────────────────────────────

/// Resharing protokolü tamamlandıktan sonra elde edilen çıktı.
#[derive(Debug)]
pub struct ReshareOutput {
    /// Yeni LocalKey — aynı aggregate public key, yeni secret share ile.
    pub new_local_key: LocalKey,
    /// Resharing oturumunun SSID'si.
    pub reshare_session_id: [u8; 32],
    /// Eski komite imzacı listesi.
    pub old_signers: Vec<PartyIndex>,
    /// Yeni komite parti listesi.
    pub new_parties: Vec<PartyIndex>,
}

impl ReshareOutput {
    /// Aggregate public key'in değişmediğini doğrula.
    pub fn verify_public_key_preserved(&self, expected_pk: &[u8; 33]) -> bool {
        verify_reshared_key_public(&self.new_local_key, expected_pk)
    }
}

// ─── YENİ PARTİ ECDH KURULUMU ─────────────────────────────────────────────────

/// Yeni komite üyesi için ECDH hazırlık yapısı.
///
/// Her yeni parti, reshare başlamadan önce bir ephemeral ECDH keypair üretmeli
/// ve public key'ini eski komite ile paylaşmalıdır.
#[derive(Debug)]
pub struct NewPartyEcdhSetup {
    /// Bu yeni partinin indeksi.
    pub party_index: PartyIndex,
    /// Bu partinin ECDH gizli anahtarı (P2P şifre çözme için).
    pub ecdh_secret: EcdhSecret,
    /// Bu partinin ECDH public key'i (eski komiteye iletilir).
    pub ecdh_pk: EcdhPublicKey,
}

impl NewPartyEcdhSetup {
    /// Yeni reshare için ECDH keypair üret.
    pub fn generate(party_index: PartyIndex) -> Self {
        let ecdh_secret = EcdhSecret::generate();
        let ecdh_pk = ecdh_secret.public_key();
        Self {
            party_index,
            ecdh_secret,
            ecdh_pk,
        }
    }
}

// ─── RESHARE OTURUM DURUMU ────────────────────────────────────────────────────

/// Resharing protokolünün mevcut aşaması.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResharePhase {
    /// Eski komite round1 mesajları bekleniyor.
    AwaitingOldCommitteeRound1,
    /// Yeni komite round2 tamamlanıyor.
    AwaitingNewCommitteeRound2,
    /// Reshare tamamlandı — yeni LocalKey hazır.
    Complete,
    /// Protokol iptal edildi.
    Aborted,
}

/// Resharing oturum yöneticisi.
///
/// Bir yeni komite üyesinin bakış açısından reshare durumunu takip eder.
#[derive(Debug)]
pub struct ReshareSession {
    /// Bu yeni partinin indeksi.
    pub new_party_index: PartyIndex,
    /// Yeni eşik parametresi.
    pub new_threshold: u8,
    /// Yeni komite büyüklüğü.
    pub new_party_count: u32,
    /// Eski komite imzacı listesi.
    pub old_signers: Vec<PartyIndex>,
    /// Mevcut aşama.
    pub phase: ResharePhase,
    /// Reshare session ID.
    pub session_id: SessionId,
    /// Beklenen aggregate public key (değişmemeli).
    pub expected_public_key: [u8; 33],
    /// Eski keygen session ID.
    pub old_keygen_session_id: [u8; 32],
}

impl ReshareSession {
    /// Yeni reshare oturumu başlat.
    pub fn new(
        new_party_index: PartyIndex,
        new_threshold: u8,
        new_party_count: u32,
        old_signers: Vec<PartyIndex>,
        session_id: SessionId,
        expected_public_key: [u8; 33],
        old_keygen_session_id: [u8; 32],
    ) -> MpcResult<Self> {
        if new_threshold == 0 || new_threshold as u32 > new_party_count {
            return Err(crate::errors::MpcError::InvalidThreshold {
                t: new_threshold,
                n: new_party_count as u8,
            });
        }
        if old_signers.is_empty() {
            return Err(crate::errors::MpcError::InsufficientShares {
                needed: 1,
                got: 0,
            });
        }
        Ok(Self {
            new_party_index,
            new_threshold,
            new_party_count,
            old_signers,
            phase: ResharePhase::AwaitingOldCommitteeRound1,
            session_id,
            expected_public_key,
            old_keygen_session_id,
        })
    }

    /// Reshare aktif mi?
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            ResharePhase::AwaitingOldCommitteeRound1
                | ResharePhase::AwaitingNewCommitteeRound2
        )
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::compute_reshare_ssid;
    use crate::types::SessionId;

    #[test]
    fn test_reshare_session_new_valid() {
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];
        let session = SessionId([0u8; 32]);
        let pk = [0x02u8; 33];

        let sess = ReshareSession::new(
            PartyIndex(1), 2, 3,
            old_signers, session, pk, [0u8; 32],
        ).unwrap();

        assert_eq!(sess.phase, ResharePhase::AwaitingOldCommitteeRound1);
        assert!(sess.is_active());
        assert_eq!(sess.new_threshold, 2);
        assert_eq!(sess.new_party_count, 3);
    }

    #[test]
    fn test_reshare_session_invalid_threshold() {
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];
        let session = SessionId([0u8; 32]);
        let pk = [0x02u8; 33];

        // t=0 → hata
        assert!(ReshareSession::new(
            PartyIndex(1), 0, 3,
            old_signers.clone(), session, pk, [0u8; 32]
        ).is_err());

        // t > n → hata
        assert!(ReshareSession::new(
            PartyIndex(1), 5, 3,
            old_signers, session, pk, [0u8; 32]
        ).is_err());
    }

    #[test]
    fn test_reshare_session_empty_old_signers() {
        let session = SessionId([0u8; 32]);
        let pk = [0x02u8; 33];
        assert!(ReshareSession::new(
            PartyIndex(1), 2, 3,
            vec![], // boş old signers
            session, pk, [0u8; 32]
        ).is_err());
    }

    #[test]
    fn test_new_party_ecdh_setup_unique() {
        let s1 = NewPartyEcdhSetup::generate(PartyIndex(1));
        let s2 = NewPartyEcdhSetup::generate(PartyIndex(1));
        // Her üretimde farklı keypair
        assert_ne!(
            s1.ecdh_pk.compressed, s2.ecdh_pk.compressed,
            "her ECDH setup benzersiz olmalı"
        );
    }

    #[test]
    fn test_reshare_output_verify() {
        let fake_pk = [0x02u8; 33];
        use crate::vss::feldman_vss_generate;
        use nova_primitives::types::Threshold;

        // Minimal sahte LocalKey oluştur
        let lk = LocalKey {
            party_index: PartyIndex(1),
            threshold: Threshold { t: 2, n: 3 },
            party_count: 3,
            secret_share: [0u8; 32],
            public_key: fake_pk,
            session_id: [0u8; 32],
            all_vss_commitments: vec![],
        };
        let output = ReshareOutput {
            new_local_key: lk,
            reshare_session_id: [0u8; 32],
            old_signers: vec![PartyIndex(1), PartyIndex(2)],
            new_parties: vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)],
        };

        assert!(output.verify_public_key_preserved(&fake_pk));
        assert!(!output.verify_public_key_preserved(&[0x03u8; 33]));
    }
}
