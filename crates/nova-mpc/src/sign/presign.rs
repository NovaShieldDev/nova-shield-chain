//! Pre-signing protokolü — offline aşama (2 round).
//!
//! §5.9.2: "Guardian'lar boşta zamanlarında pre-signature shares üretir.
//!          Bu parçalar şifreli olarak depolanır (henüz bir TX'e bağlı değil)."
//!
//! §6.1.1: "Non-interactive pre-signing: guardian'ların aynı anda online
//!          olması gerekmez."
//!
//! ## Protokol
//!
//! **Round 1 (Nonce Taahhüdü):**
//! Her imzacı P_i:
//! 1. Rastgele nonce `r_i` seçer
//! 2. `R_i = r_i * G` hesaplar
//! 3. `com_i = BLAKE3("nova-presign-r1" || R_i || session_id)` taahhüdü yayımlar
//!
//! **Round 2 (Nonce Açılımı):**
//! Her imzacı P_i:
//! 1. `R_i` ve DLog kanıtını yayımlar
//! 2. Her parti tüm `R_j`'leri doğrular
//! 3. `R = sum(R_j)` birleşik nonce noktasını hesaplar
//! 4. `PreSignState` saklar
//!
//! ## Güvenlik
//!
//! - Nonce taahhüdü adaptive attack'ı önler (R_i seçildikten sonra değiştirilemez)
//! - DLog kanıtı: her parti `r_i`'yi gerçekten bildiğini kanıtlar
//! - Nonce tek kullanımlık: kullanıldıktan sonra sıfırlanır

use blake3::Hasher;
use k256::Scalar;
use k256::elliptic_curve::Field;
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::crypto_utils::{
    point_add, scalar_mul_generator, scalar_to_bytes, IDENTITY_BYTES,
};
use crate::dlog::{dlog_prove, dlog_verify};
use crate::errors::{MpcError, MpcResult};
use crate::types::{
    DlogProofBytes, LocalKey, PartyIndex, PreSignShare, PreSignState,
    PresignRound1Msg, PresignRound2Msg, SessionId,
};

// ─── PRE-SIGN ROUND 1 ────────────────────────────────────────────────────────

/// Pre-sign Round 1 yerel durumu.
#[derive(Debug)]
pub struct PresignRound1State {
    /// Gizli nonce `r_i`. ZEROIZE AFTER USE.
    nonce_secret: [u8; 32],
    /// Public nonce `R_i = r_i * G`.
    nonce_point: [u8; 33],
    /// Nonce taahhüdü.
    nonce_commitment: [u8; 32],
    /// Session ID.
    session_id: SessionId,
    /// Bu partinin indeksi.
    party_index: PartyIndex,
    /// Geçerlilik bitiş slotu.
    valid_until_slot: u64,
}

impl Drop for PresignRound1State {
    fn drop(&mut self) {
        self.nonce_secret.zeroize();
    }
}

/// Pre-sign Round 1'i çalıştır.
///
/// Nonce taahhüdü üretir — henüz R_i açıklanmaz.
pub fn run_presign_round1(
    party_index: PartyIndex,
    session_id: &SessionId,
    valid_until_slot: u64,
) -> MpcResult<(PresignRound1Msg, PresignRound1State)> {
    // Rastgele nonce r_i üret
    let r_i = Scalar::random(&mut OsRng);
    let nonce_secret = scalar_to_bytes(&r_i);
    let nonce_point = scalar_mul_generator(&r_i);

    // Nonce taahhüdü: com_i = BLAKE3("nova-presign-r1" || R_i || session_id)
    let nonce_commitment = compute_nonce_commitment(&nonce_point, session_id);

    let msg = PresignRound1Msg {
        from: party_index,
        nonce_commitment,
    };

    let state = PresignRound1State {
        nonce_secret,
        nonce_point,
        nonce_commitment,
        session_id: *session_id,
        party_index,
        valid_until_slot,
    };

    Ok((msg, state))
}

/// Nonce taahhüdü hesapla.
pub fn compute_nonce_commitment(nonce_point: &[u8; 33], session_id: &SessionId) -> [u8; 32] {
    let mut hasher = Hasher::new_derive_key("nova-presign-r1-nonce-commitment-v1");
    hasher.update(nonce_point);
    hasher.update(session_id.as_bytes());
    *hasher.finalize().as_bytes()
}

// ─── PRE-SIGN ROUND 2 ────────────────────────────────────────────────────────

/// Pre-sign Round 2'yi çalıştır.
///
/// Nonce açılımını yayımlar, tüm partilerin nonce'larını toplar.
pub fn run_presign_round2(
    r1_state: PresignRound1State,
    all_r1_msgs: Vec<PresignRound1Msg>,
) -> MpcResult<(PresignRound2Msg, Vec<PresignRound1Msg>, PresignRound1State)> {
    let party_index = r1_state.party_index;
    let session_id = r1_state.session_id;

    // DLog kanıtı: r_i bilindiğini kanıtla
    let dlog_proof = dlog_prove(
        &r1_state.nonce_secret,
        &r1_state.nonce_point,
        "nova-presign-nonce-dlog-v1",
        session_id.as_bytes(),
    )?;

    let msg = PresignRound2Msg {
        from: party_index,
        nonce_point: r1_state.nonce_point,
        dlog_proof,
    };

    Ok((msg, all_r1_msgs, r1_state))
}

/// Pre-sign tamamla — `PreSignState` üret.
///
/// Tüm Round 2 mesajları doğrulanır ve birleşik nonce hesaplanır.
pub fn finalize_presign(
    r1_state: PresignRound1State,
    r1_msgs: Vec<PresignRound1Msg>,
    all_r2_msgs: Vec<PresignRound2Msg>,
    local_key: &LocalKey,
    signers: Vec<PartyIndex>,
) -> MpcResult<PreSignState> {
    let my_index = r1_state.party_index;
    let session_id = r1_state.session_id;

    // ── 1. Mesaj sayısını doğrula ────────────────────────────────────────────
    let signer_count = signers.len();
    if all_r2_msgs.len() != signer_count {
        return Err(MpcError::InvalidSignerCount {
            expected: signer_count,
            got: all_r2_msgs.len(),
        });
    }

    // ── 2. Her Round 2 mesajını doğrula ─────────────────────────────────────
    for r2_msg in &all_r2_msgs {
        // Round 1 taahhüdüyle eşleşiyor mu?
        let r1_msg = r1_msgs
            .iter()
            .find(|m| m.from == r2_msg.from)
            .ok_or(MpcError::MissingMessage { round: 1, from: r2_msg.from.0 })?;

        let expected_commitment = compute_nonce_commitment(&r2_msg.nonce_point, &session_id);
        if expected_commitment != r1_msg.nonce_commitment {
            return Err(MpcError::CommitmentMismatch { party: r2_msg.from.0 });
        }

        // DLog kanıtını doğrula
        let valid = dlog_verify(
            &r2_msg.nonce_point,
            &r2_msg.dlog_proof,
            "nova-presign-nonce-dlog-v1",
            session_id.as_bytes(),
        )?;

        if !valid {
            return Err(MpcError::DlogProofInvalid { party: r2_msg.from.0 });
        }
    }

    // ── 3. R = sum(R_j) — Birleşik nonce noktası ────────────────────────────
    let mut combined_nonce = IDENTITY_BYTES;
    for r2_msg in &all_r2_msgs {
        combined_nonce = point_add(&combined_nonce, &r2_msg.nonce_point)?;
    }

    // ── 4. PreSignState oluştur ──────────────────────────────────────────────
    let presign_state = PreSignState {
        our_index: my_index,
        signers,
        our_nonce_secret: r1_state.nonce_secret,
        combined_nonce,
        public_key: local_key.public_key,
        session_id: *session_id.as_bytes(),
        nonce_used: std::sync::atomic::AtomicBool::new(false),
    };

    Ok(presign_state)
}

/// PreSignShare oluştur — saklama için.
///
/// `PreSignState`'i serileştirilebilir `PreSignShare`'e dönüştür.
pub fn create_presign_share(state: &PreSignState, valid_until_slot: u64) -> MpcResult<PreSignShare> {
    // combined nonce'tan R_i = kendi nonce noktamızı bulmak için:
    // Sadece kendi r_i'yi ve combined R'yi sakla.
    // Online aşamada bu ikisini kullanırız.

    // Kendi nonce noktasını hesapla
    let our_nonce_point = {
        let r_scalar = crate::crypto_utils::scalar_from_bytes(&state.our_nonce_secret)?;
        scalar_mul_generator(&r_scalar)
    };

    Ok(PreSignShare {
        party_index: state.our_index,
        nonce_secret: state.our_nonce_secret,
        nonce_point: our_nonce_point,
        valid_until_slot,
        session_id: state.session_id,
    })
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::compute_presign_ssid;

    fn test_presign_session(signers: &[PartyIndex]) -> SessionId {
        let keygen_session = SessionId([0x11u8; 32]);
        compute_presign_ssid(&keygen_session, signers, 1000, &[0u8; 32])
    }

    #[test]
    fn test_presign_round1_basic() {
        let session = test_presign_session(&[PartyIndex(1), PartyIndex(2), PartyIndex(3)]);
        let (msg, state) = run_presign_round1(PartyIndex(1), &session, 5000).unwrap();
        assert_eq!(msg.from, PartyIndex(1));
        assert_ne!(msg.nonce_commitment, [0u8; 32]);
        assert_ne!(state.nonce_point, [0u8; 33]);
        assert_ne!(state.nonce_secret, [0u8; 32]);
    }

    #[test]
    fn test_presign_round1_nonce_unique() {
        let session = test_presign_session(&[PartyIndex(1), PartyIndex(2)]);
        let (m1, _) = run_presign_round1(PartyIndex(1), &session, 5000).unwrap();
        let (m2, _) = run_presign_round1(PartyIndex(1), &session, 5000).unwrap();
        // Her çağrıda farklı rastgele nonce
        assert_ne!(m1.nonce_commitment, m2.nonce_commitment);
    }

    #[test]
    fn test_presign_nonce_commitment_deterministic() {
        let point = [0x02u8; 33];
        let session = SessionId([0xAAu8; 32]);
        let c1 = compute_nonce_commitment(&point, &session);
        let c2 = compute_nonce_commitment(&point, &session);
        assert_eq!(c1, c2, "deterministik taahhüt");
    }

    #[test]
    fn test_presign_round2_dlog_proof() {
        let session = test_presign_session(&[PartyIndex(1), PartyIndex(2)]);
        let (r1_msg, r1_state) = run_presign_round1(PartyIndex(1), &session, 5000).unwrap();
        let (r2_msg, _, _) =
            run_presign_round2(r1_state, vec![r1_msg]).unwrap();

        // DLog kanıtı doğrulanabilmeli
        let valid = dlog_verify(
            &r2_msg.nonce_point,
            &r2_msg.dlog_proof,
            "nova-presign-nonce-dlog-v1",
            session.as_bytes(),
        )
        .unwrap();
        assert!(valid, "presign nonce DLog kanıtı doğrulanmalı");
    }

    #[test]
    fn test_presign_commitment_mismatch_rejected() {
        let signers = vec![PartyIndex(1), PartyIndex(2)];
        let session = test_presign_session(&signers);

        let (r1_msg1, r1_state1) = run_presign_round1(PartyIndex(1), &session, 5000).unwrap();
        let (r1_msg2, r1_state2) = run_presign_round1(PartyIndex(2), &session, 5000).unwrap();

        let r1_msgs_for_1 = vec![r1_msg1.clone(), r1_msg2.clone()];
        let (r2_msg1, r1_msgs_kept1, _) =
            run_presign_round2(r1_state1, r1_msgs_for_1.clone()).unwrap();
        let (mut r2_msg2, _r1_msgs_kept2, r1_state2_moved) =
            run_presign_round2(r1_state2, r1_msgs_for_1).unwrap();

        // R_i'yi boz → commitment eşleşmeyecek
        r2_msg2.nonce_point[5] ^= 0xFF;

        // Sahte LocalKey
        let local_key = LocalKey {
            party_index: PartyIndex(1),
            threshold: nova_primitives::types::Threshold { t: 2, n: 2 },
            party_count: 2,
            secret_share: [1u8; 32],
            public_key: [0x02u8; 33],
            session_id: [0u8; 32],
            all_vss_commitments: vec![],
        };

        let r1_msgs_final = vec![r1_msg1, r1_msg2];
        let result = finalize_presign(
            r1_state2_moved,
            r1_msgs_final,
            vec![r2_msg1, r2_msg2],
            &local_key,
            signers,
        );
        assert!(result.is_err(), "bozuk nonce noktas�� reddedilmeli");
    }
}
