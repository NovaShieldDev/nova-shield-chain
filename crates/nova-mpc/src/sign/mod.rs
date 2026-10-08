//! İmzalama protokolü — FROST-style Schnorr Threshold Signing.
//!
//! §5.9.2: "Non-interactive pre-signing + ~10ms online aşama."
//! §6.1.1: "Nova Shield Chain yalnızca CGGMP21 kullanır."
//!
//! ## Protokol Özeti
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  OFFLINE (güçlü kriptografi, guardian'lar önceden yapar)         │
//! │  Presign Round 1: her guardian nonce taahhüdü gönderir          │
//! │  Presign Round 2: nonce açıklanır + DLog kanıtı                 │
//! │  → PreSignState / PreSignShare güvenli depolamaya alınır        │
//! └─────────────────────────────────────────────────────────────────┘
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  ONLINE (~10ms) — TX onay sinyali gelince                        │
//! │  Her signer: s_i = r_i + challenge * x_i * λ_i                  │
//! │  Combiner: s = ∑s_i → ThresholdSignature(R, s)                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modüller
//!
//! - `presign`: Offline pre-signing protokolü (2 round)
//! - `online`: Online imzalama (~10ms, 1 round)

pub mod online;
pub mod presign;

pub use online::{
    aggregate_partial_signatures, compute_lagrange_coefficient, compute_sign_challenge,
    create_partial_signature, is_valid_signature_format, verify_partial_signature,
    verify_threshold_signature,
};
pub use presign::{
    compute_nonce_commitment, create_presign_share, finalize_presign, run_presign_round1,
    run_presign_round2, PresignRound1State,
};

use crate::errors::MpcResult;
use crate::types::{
    LocalKey, PartialSignature, PartyIndex, PreSignShare, PreSignState,
    PresignRound1Msg, PresignRound2Msg, SessionId, ThresholdSignature,
};

// ─── PRESIGN ÇIKTISI ──────────────────────────────────────────────────────────

/// Pre-signing protokolü tamamlandıktan sonra elde edilen çıktı.
///
/// `PreSignState` online imzalama için hazır durum — ephemeral ve bir kez kullanılır.
/// `PreSignShare` güvenli depolamaya alınan kalıcı form — batch kullanım için.
#[derive(Debug)]
pub struct PresignOutput {
    /// Online imzalama için hazır durum.
    pub state: PreSignState,
    /// Depolanabilir form — `valid_until_slot` dolana kadar kullanılabilir.
    pub share: PreSignShare,
}

// ─── İMZA ÇIKTISI ─────────────────────────────────────────────────────────────

/// Online imzalama tamamlandıktan sonra elde edilen çıktı.
#[derive(Debug)]
pub struct SignOutput {
    /// Birleştirilmiş threshold Schnorr imzası.
    pub signature: ThresholdSignature,
    /// İmzalanan mesaj hash'i (ek doğrulama için).
    pub message_hash: [u8; 32],
    /// İmzacı parti listesi.
    pub signers: Vec<PartyIndex>,
    /// Bu imzanın aggregate public key'i.
    pub public_key: [u8; 33],
}

impl SignOutput {
    /// İmzayı doğrula — kolaylık yöntemi.
    pub fn verify(&self) -> MpcResult<bool> {
        verify_threshold_signature(&self.signature, &self.public_key, &self.message_hash)
    }
}

// ─── PRESIGN OTURUM DURUMU ────────────────────────────────────────────────────

/// Pre-signing protokolünün mevcut aşaması.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresignPhase {
    /// Round 1 mesajları bekleniyor (nonce taahhütleri).
    AwaitingRound1,
    /// Round 2 mesajları bekleniyor (nonce açılımları).
    AwaitingRound2,
    /// Pre-sign tamamlandı — PreSignState hazır.
    Complete,
    /// Protokol iptal edildi.
    Aborted,
}

/// Pre-signing oturum durumu.
#[derive(Debug)]
pub struct PresignSession {
    /// İmzacı parti listesi.
    pub signers: Vec<PartyIndex>,
    /// Bu partinin indeksi.
    pub party_index: PartyIndex,
    /// Mevcut aşama.
    pub phase: PresignPhase,
    /// Session ID.
    pub session_id: SessionId,
    /// Bu presign'in geçerliliği son slot'u.
    pub valid_until_slot: u64,
}

impl PresignSession {
    /// Yeni pre-signing oturumu başlat.
    pub fn new(
        party_index: PartyIndex,
        signers: Vec<PartyIndex>,
        session_id: SessionId,
        valid_until_slot: u64,
    ) -> MpcResult<Self> {
        if !signers.contains(&party_index) {
            return Err(crate::errors::MpcError::InvalidPartyIndex {
                index: party_index.0,
                max: signers.iter().map(|p| p.0).max().unwrap_or(0),
            });
        }
        Ok(Self {
            signers,
            party_index,
            phase: PresignPhase::AwaitingRound1,
            session_id,
            valid_until_slot,
        })
    }

    /// Oturum aktif mi?
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            PresignPhase::AwaitingRound1 | PresignPhase::AwaitingRound2
        )
    }
}

// ─── SIGNING OTURUM DURUMU ────────────────────────────────────────────────────

/// Online imzalama oturum durumu.
#[derive(Debug)]
pub struct SigningSession {
    /// İmzacı parti listesi.
    pub signers: Vec<PartyIndex>,
    /// Bu partinin indeksi.
    pub party_index: PartyIndex,
    /// İmzalanacak mesaj hash'i.
    pub message_hash: [u8; 32],
    /// Session ID.
    pub session_id: [u8; 32],
    /// Toplanan kısmi imzalar.
    pub collected_partial_sigs: Vec<PartialSignature>,
}

impl SigningSession {
    /// Yeni online imzalama oturumu başlat.
    pub fn new(
        party_index: PartyIndex,
        signers: Vec<PartyIndex>,
        message_hash: [u8; 32],
        session_id: [u8; 32],
    ) -> Self {
        Self {
            signers,
            party_index,
            message_hash,
            session_id,
            collected_partial_sigs: Vec::new(),
        }
    }

    /// Kısmi imza ekle.
    pub fn add_partial_signature(&mut self, sig: PartialSignature) -> MpcResult<()> {
        // Yineleme kontrolü
        if self.collected_partial_sigs.iter().any(|s| s.party_index == sig.party_index) {
            return Err(crate::errors::MpcError::DuplicateMessage {
                round: 3,
                from: sig.party_index.0,
            });
        }
        // Session ID kontrolü
        if &sig.session_id != &self.session_id {
            return Err(crate::errors::MpcError::SessionMismatch {
                expected: self.session_id.to_vec(),
                got: sig.session_id.to_vec(),
            });
        }
        self.collected_partial_sigs.push(sig);
        Ok(())
    }

    /// Tüm partial imzalar toplandı mı?
    pub fn is_ready(&self) -> bool {
        self.collected_partial_sigs.len() == self.signers.len()
    }

    /// İmzayı finalleştir.
    pub fn finalize(
        &self,
        combined_nonce: &[u8; 33],
        public_key: &[u8; 33],
    ) -> MpcResult<SignOutput> {
        if !self.is_ready() {
            return Err(crate::errors::MpcError::InsufficientShares {
                needed: self.signers.len(),
                got: self.collected_partial_sigs.len(),
            });
        }

        let signature = aggregate_partial_signatures(
            &self.collected_partial_sigs,
            combined_nonce,
            &self.session_id,
            &self.signers,
        )?;

        // NOVA-RED-MPC-002: Aggregasyon sonrası imzayı doğrula.
        // Sahte s_i değerleri göndererek geçersiz imza üretilebilir.
        // `verify_threshold_signature` false dönerse → hata fırlat.
        let valid = verify_threshold_signature(&signature, public_key, &self.message_hash)?;
        if !valid {
            return Err(crate::errors::MpcError::SignatureVerificationFailed {
                reason: "aggregasyon sonrası imza doğrulaması başarısız — geçersiz partial imza".into(),
            });
        }

        Ok(SignOutput {
            signature,
            message_hash: self.message_hash,
            signers: self.signers.clone(),
            public_key: *public_key,
        })
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{compute_keygen_ssid, compute_presign_ssid};
    use crate::types::SessionId;
    use nova_primitives::types::ChainId;

    #[test]
    fn test_presign_session_new_valid() {
        let signers = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];
        let session = SessionId([0u8; 32]);
        let s = PresignSession::new(PartyIndex(2), signers, session, 9999).unwrap();
        assert_eq!(s.phase, PresignPhase::AwaitingRound1);
        assert!(s.is_active());
    }

    #[test]
    fn test_presign_session_not_in_signers() {
        let signers = vec![PartyIndex(1), PartyIndex(2)];
        let session = SessionId([0u8; 32]);
        let result = PresignSession::new(PartyIndex(5), signers, session, 9999);
        assert!(result.is_err(), "imzacı listesinde olmayan parti reddedilmeli");
    }

    #[test]
    fn test_signing_session_add_duplicate_partial() {
        let mut sess = SigningSession::new(
            PartyIndex(1),
            vec![PartyIndex(1), PartyIndex(2)],
            [0u8; 32],
            [0u8; 32],
        );
        let ps = PartialSignature {
            party_index: PartyIndex(1),
            s_i: [1u8; 32],
            session_id: [0u8; 32],
        };
        sess.add_partial_signature(ps.clone()).unwrap();
        // İkinci kez aynı parti → hata
        assert!(sess.add_partial_signature(ps).is_err());
    }

    #[test]
    fn test_signing_session_wrong_session_rejected() {
        let mut sess = SigningSession::new(
            PartyIndex(1),
            vec![PartyIndex(1), PartyIndex(2)],
            [0u8; 32],
            [0xAAu8; 32], // session A
        );
        let ps = PartialSignature {
            party_index: PartyIndex(2),
            s_i: [1u8; 32],
            session_id: [0xBBu8; 32], // session B — farklı!
        };
        assert!(
            sess.add_partial_signature(ps).is_err(),
            "yanlış session ID reddedilmeli"
        );
    }

    #[test]
    fn test_sign_output_verify() {
        use crate::crypto_utils::scalar_from_u32;
        use crate::sign::online::{compute_sign_challenge, verify_threshold_signature};
        use crate::crypto_utils::{scalar_mul_generator, scalar_to_bytes};
        use k256::Scalar;
        use k256::elliptic_curve::Field;
        use rand::rngs::OsRng;

        // Manuel imza oluştur ve doğrula
        let secret = Scalar::random(&mut OsRng);
        let public_key = scalar_mul_generator(&secret);
        let r_secret = Scalar::random(&mut OsRng);
        let r_point = scalar_mul_generator(&r_secret);
        let msg_hash = [0x42u8; 32];

        let e = compute_sign_challenge(&r_point, &public_key, &msg_hash).unwrap();
        let s = r_secret + e * secret;

        let sig = ThresholdSignature {
            r_point,
            s_scalar: scalar_to_bytes(&s),
        };

        let out = SignOutput {
            signature: sig,
            message_hash: msg_hash,
            signers: vec![PartyIndex(1)],
            public_key,
        };

        assert!(out.verify().unwrap(), "SignOutput.verify() çalışmalı");
    }
}
