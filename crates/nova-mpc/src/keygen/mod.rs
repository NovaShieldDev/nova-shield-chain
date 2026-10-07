//! Keygen protokolü — 3 roundlı dağıtık anahtar üretimi.
//!
//! §5.1.3: "CGGMP21 Non-Interactive Pre-Signing + FROST-style Schnorr Threshold"
//! §6.1.1: "Nova Shield Chain yalnızca CGGMP21 kullanır."
//!
//! ## Protokol Özeti
//!
//! ```text
//! Round 1 (Broadcast):
//!   Her parti P_i:
//!   - u_i = random scalar (gizli katkı)
//!   - VSS polinom: f_i(x) = u_i + a_1*x + ... + a_{t-1}*x^{t-1}
//!   - VSS taahhüt: C_{i,j} = a_j * G
//!   - com_i = BLAKE3(VSS_i || session_id)
//!   - dlog_i = Schnorr(u_i, U_i = u_i*G)
//!   - ecdh_pk_i = ephemeral ECDH keypair public
//!   Yayım: (com_i, dlog_i, ecdh_pk_i)
//!
//! Round 2 (Broadcast + P2P):
//!   Her parti P_i:
//!   - VSS açılımını broadcast et: VSS_i
//!   - Her P_j için şifreli pay gönder: AES-GCM(f_i(j), ECDH(ecdh_i, ecdh_j))
//!
//! Round 3 (Final):
//!   Her parti P_i:
//!   - Tüm broadcast'leri ve DLog kanıtlarını doğrula
//!   - Kendi payını şifreden çöz + VSS ile doğrula
//!   - x_i = sum(f_j(i), j=1..n)   ← aggregate secret share
//!   - X = sum(C_{j,0}, j=1..n)    ← aggregate public key
//!   - X_i = x_i * G               ← kendi public payı
//!   - LocalKey üret
//! ```
//!
//! ## Kullanım
//!
//! ```rust,ignore
//! // Round 1
//! let (msg1, state1) = keygen::round1::run_round1(party_index, threshold, n, &session)?;
//!
//! // Round 2 (tüm round1 mesajları toplandıktan sonra)
//! let (broadcast, p2p_msgs, state2) = keygen::round2::run_round2(state1, all_r1_msgs)?;
//!
//! // Round 3 (tüm round2 mesajları toplandıktan sonra)
//! let (round3_msg, local_key) = keygen::round3::run_round3(state2, all_r2_broadcasts, my_p2p)?;
//! ```

pub mod round1;
pub mod round2;
pub mod round3;

pub use round1::{run_round1, Round1LocalState};
pub use round2::{run_round2, Round2LocalState};
pub use round3::{run_round3, compute_public_share_from_vss};

use crate::errors::MpcResult;
use crate::types::{LocalKey, PartyIndex, SessionId};

// ─── KEYGEN STATE MACHINE ─────────────────────────────────────────────────────

/// Keygen protokolünün mevcut aşaması.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeygenPhase {
    /// Round 1 mesajları bekleniyor.
    AwaitingRound1,
    /// Round 2 mesajları bekleniyor.
    AwaitingRound2,
    /// Round 3 tamamlandı — LocalKey hazır.
    Complete,
    /// Protokol iptal edildi.
    Aborted,
}

/// Keygen oturumu durumu.
#[derive(Debug)]
pub struct KeygenState {
    /// Bu partinin indeksi.
    pub party_index: PartyIndex,
    /// Mevcut aşama.
    pub phase: KeygenPhase,
    /// Session ID.
    pub session_id: SessionId,
    /// Threshold.
    pub threshold: u8,
    /// Parti sayısı.
    pub party_count: u32,
}

impl KeygenState {
    /// Yeni keygen oturumu başlat.
    pub fn new(
        party_index: PartyIndex,
        threshold: u8,
        party_count: u32,
        session_id: SessionId,
    ) -> MpcResult<Self> {
        PartyIndex::new(party_index.0, party_count)?;
        if threshold == 0 || threshold as u32 > party_count {
            return Err(crate::errors::MpcError::InvalidThreshold {
                t: threshold,
                n: party_count as u8,
            });
        }
        Ok(Self {
            party_index,
            phase: KeygenPhase::AwaitingRound1,
            session_id,
            threshold,
            party_count,
        })
    }
}

/// Keygen protokolü çıktısı.
#[derive(Debug)]
pub struct KeygenOutput {
    /// Üretilen yerel anahtar.
    pub local_key: LocalKey,
    /// Bu keygen oturumunun session ID'si.
    pub session_id: SessionId,
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::compute_keygen_ssid;
    use nova_primitives::types::ChainId;

    #[test]
    fn test_keygen_state_new_valid() {
        let parties: Vec<PartyIndex> = (1..=5).map(PartyIndex).collect();
        let session = compute_keygen_ssid(ChainId(1984), &parties, &[0u8; 32]);
        let state = KeygenState::new(PartyIndex(3), 3, 5, session).unwrap();
        assert_eq!(state.phase, KeygenPhase::AwaitingRound1);
        assert_eq!(state.threshold, 3);
        assert_eq!(state.party_count, 5);
    }

    #[test]
    fn test_keygen_state_invalid_threshold() {
        let session = SessionId([0u8; 32]);
        assert!(KeygenState::new(PartyIndex(1), 0, 5, session).is_err());
        assert!(KeygenState::new(PartyIndex(1), 6, 5, session).is_err());
    }

    #[test]
    fn test_keygen_state_invalid_party() {
        let session = SessionId([0u8; 32]);
        assert!(KeygenState::new(PartyIndex(0), 3, 5, session).is_err());
        assert!(KeygenState::new(PartyIndex(6), 3, 5, session).is_err());
    }
}
