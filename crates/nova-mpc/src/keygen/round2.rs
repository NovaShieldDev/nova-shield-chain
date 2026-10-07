//! Keygen Round 2 — VSS Açılımı + Şifreli P2P Paylar
//!
//! §01-multi-party-ecdsa.md §4.2 Phase 2: "VSS dağıtımı"
//! §02-tss-lib.md §4 Round 2: "Decommit + Shamir payları"
//! §p2p_encrypt.rs: AES-256-GCM ile P2P şifreleme
//!
//! ## Protokol
//!
//! Round 1'deki taahhütler alındıktan sonra her parti:
//!
//! 1. Tüm Round 1 mesajlarını doğrular (basit format kontrolü)
//! 2. VSS taahhüdünü broadcast eder (Round 1 taahhüdüyle eşleşmeli)
//! 3. Her diğer partiye ECDH + AES-256-GCM ile şifreli pay gönderir:
//!    - `nonce, ciphertext = AES-GCM(HKDF(ECDH(ours, theirs)), f_i(j), AAD)`
//!    - `AAD = session_id || from.le || to.le`
//!
//! ## Güvenlik
//!
//! - VSS açılımı Round 1 hash'iyle doğrulanır → adaptive attack önlenir
//! - ECDH forward secrecy: her oturum için ephemeral keypair
//! - AES-GCM AAD: yanlış alıcıya yönlendirme tespiti

use crate::crypto_utils::{point_add, IDENTITY_BYTES};
use crate::errors::{MpcError, MpcResult};
use crate::keygen::round1::{compute_commitment_hash, Round1LocalState};
use crate::p2p_encrypt::encrypt_share;
use crate::types::{
    EcdhPublicKey, KeygenRound1Msg, KeygenRound2BroadcastMsg, KeygenRound2P2pMsg,
    PartyIndex, SessionId,
};
use crate::vss::feldman_vss_verify;

// ─── ROUND 2 LOCAL STATE ──────────────────────────────────────────────────────

/// Round 2 sonrası yerel durum — Round 3'e taşınır.
#[derive(Debug)]
pub struct Round2LocalState {
    /// Round 1 state'i (VSS, ECDH secret, vb.)
    pub r1: Round1LocalState,

    /// Diğer partilerden gelen Round 1 mesajları (doğrulanmış).
    pub received_r1_msgs: Vec<KeygenRound1Msg>,

    /// Diğer partilerden gelen Round 2 broadcast mesajları.
    /// Round 3'te bunları doğrularız.
    pub received_r2_broadcast: Vec<KeygenRound2BroadcastMsg>,

    /// Her partiden gelen şifreli P2P pay — sadece bize ait olanlar.
    /// `received_shares[i]` = parti i+1'den gelen şifreli pay bilgisi.
    pub received_r2_p2p: Vec<ReceivedP2pShare>,
}

/// Bir partiden alınan şifreli P2P pay.
#[derive(Debug, Clone)]
pub struct ReceivedP2pShare {
    pub from: PartyIndex,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

// ─── ROUND 2 ÇALIŞTIRICISI ────────────────────────────────────────────────────

/// Keygen Round 2'yi çalıştır.
///
/// ## Parametreler
///
/// - `r1_state`: Round 1'den gelen yerel durum.
/// - `all_r1_msgs`: Tüm partilerden gelen Round 1 mesajları (kendi dahil).
///
/// ## Döndürür
///
/// - `KeygenRound2BroadcastMsg`: Herkese broadcast edilecek (VSS açılımı).
/// - `Vec<KeygenRound2P2pMsg>`: Her partiye P2P gönderilecek şifreli pay.
/// - `Round2LocalState`: Round 3'e taşınacak durum.
pub fn run_round2(
    r1_state: Round1LocalState,
    all_r1_msgs: Vec<KeygenRound1Msg>,
) -> MpcResult<(
    KeygenRound2BroadcastMsg,
    Vec<KeygenRound2P2pMsg>,
    Round2LocalState,
)> {
    let party_count = r1_state.party_count;
    let threshold = r1_state.threshold;
    let my_index = r1_state.party_index;
    let session_id = r1_state.session_id;

    // ── 1. Mesaj sayısı doğrula ───────────────────────────────────────────────
    if all_r1_msgs.len() != party_count as usize {
        return Err(MpcError::InvalidShareCount {
            expected: party_count as usize,
            got: all_r1_msgs.len(),
        });
    }

    // ── 2. Her Round 1 mesajını temel format kontrolünden geçir ─���────────────
    for msg in &all_r1_msgs {
        crate::keygen::round1::verify_round1_msg(msg, party_count, &session_id)?;
        // Yinelenen mesaj kontrolü (from indeksleri benzersiz olmalı)
    }

    // Yinelenen from indeksleri var mı?
    {
        let mut froms: Vec<u32> = all_r1_msgs.iter().map(|m| m.from.0).collect();
        froms.sort();
        froms.dedup();
        if froms.len() != party_count as usize {
            return Err(MpcError::InternalError(
                "round1 mesajlarında yinelenen parti indeksi".into(),
            ));
        }
    }

    // ── 3. Broadcast: VSS taahhüdünü açıkla ──────────────────────────────────
    let broadcast_msg = KeygenRound2BroadcastMsg {
        from: my_index,
        vss_commitment: r1_state.vss_output.commitment.clone(),
    };

    // ── 4. P2P: Her partiye şifreli pay gönder ────────────────────────────────
    let mut p2p_msgs = Vec::with_capacity((party_count - 1) as usize);

    for target_party in 1..=party_count {
        if target_party == my_index.0 {
            continue; // Kendimize pay göndermiyoruz
        }

        let to = PartyIndex(target_party);

        // Bu parti için hesaplanan VSS payı: f_i(target_party)
        // shares[target_party - 1].value = f_i(target_party)
        let share_bytes = &r1_state.vss_output.shares[(target_party - 1) as usize].value;

        // Alıcının ECDH public key'ini bul
        let their_ecdh_pk = find_ecdh_pk(&all_r1_msgs, to)?;

        // Şifrele
        let (nonce, ciphertext) = encrypt_share(
            &r1_state.ecdh_secret,
            &their_ecdh_pk,
            &session_id,
            my_index,
            to,
            share_bytes,
        )?;

        p2p_msgs.push(KeygenRound2P2pMsg {
            from: my_index,
            to,
            encrypted_share_nonce: nonce,
            encrypted_share_ciphertext: ciphertext,
        });
    }

    // ── 5. Yerel durum kaydet ─────────────────────────────────────────────────
    let state = Round2LocalState {
        r1: r1_state,
        received_r1_msgs: all_r1_msgs,
        received_r2_broadcast: Vec::new(), // Round 3'te doldurulacak
        received_r2_p2p: Vec::new(),       // Round 3'te doldurulacak
    };

    Ok((broadcast_msg, p2p_msgs, state))
}

// ─── ROUND 2 MESAJ DOĞRULAMA ─────────────────────────────────────────────────

/// Gelen Round 2 broadcast mesajını doğrula.
///
/// - VSS taahhüdü Round 1'de verilen hash ile eşleşmeli
/// - VSS polinom derecesi threshold - 1 olmalı
pub fn verify_round2_broadcast(
    msg: &KeygenRound2BroadcastMsg,
    r1_msgs: &[KeygenRound1Msg],
    session_id: &SessionId,
    threshold: u8,
) -> MpcResult<()> {
    // Gönderenin Round 1 mesajını bul
    let r1_msg = r1_msgs
        .iter()
        .find(|m| m.from == msg.from)
        .ok_or(MpcError::MissingMessage {
            round: 1,
            from: msg.from.0,
        })?;

    // VSS taahhüt hash'ini yeniden hesapla ve karşılaştır
    let expected_hash = compute_commitment_hash(&msg.vss_commitment, session_id)?;

    if expected_hash != r1_msg.commitment_hash {
        return Err(MpcError::CommitmentMismatch { party: msg.from.0 });
    }

    // Polinom derecesi: t katsayı = threshold katsayı (degree = t-1)
    if msg.vss_commitment.points.len() != threshold as usize {
        return Err(MpcError::InvalidShareCount {
            expected: threshold as usize,
            got: msg.vss_commitment.points.len(),
        });
    }

    // Commitment noktaları geçerli secp256k1 noktaları mı?
    for point in &msg.vss_commitment.points {
        crate::crypto_utils::point_from_bytes(point)
            .map_err(|_| MpcError::InvalidPoint)?;
    }

    Ok(())
}

/// Gelen P2P payını doğrulama için ihtiyaç duyulan bilgileri topla.
/// Gerçek şifre çözme + VSS doğrulaması Round 3'te yapılır.
pub fn verify_round2_p2p_format(msg: &KeygenRound2P2pMsg) -> MpcResult<()> {
    if msg.encrypted_share_ciphertext.is_empty() {
        return Err(MpcError::InternalError("şifreli pay boş".into()));
    }
    // AES-256-GCM: 32 byte plaintext + 16 byte GCM tag = 48 byte ciphertext
    if msg.encrypted_share_ciphertext.len() < 32 {
        return Err(MpcError::InternalError(
            format!("şifreli pay çok kısa: {} byte", msg.encrypted_share_ciphertext.len())
        ));
    }
    Ok(())
}

// ─── YARDIMCI ────────────────────────────────────────────────────────────────

/// Round 1 mesajlarından belirtilen partinin ECDH public key'ini bul.
fn find_ecdh_pk(r1_msgs: &[KeygenRound1Msg], party: PartyIndex) -> MpcResult<EcdhPublicKey> {
    r1_msgs
        .iter()
        .find(|m| m.from == party)
        .map(|m| m.ecdh_pk.clone())
        .ok_or(MpcError::MissingMessage {
            round: 1,
            from: party.0,
        })
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::round1::run_round1;
    use crate::session::compute_keygen_ssid;
    use nova_primitives::types::ChainId;

    fn setup_round1(n: u32, t: u8) -> (SessionId, Vec<(KeygenRound1Msg, Round1LocalState)>) {
        let parties: Vec<PartyIndex> = (1..=n).map(PartyIndex).collect();
        let session = compute_keygen_ssid(ChainId(1984), &parties, &[0u8; 32]);
        let results: Vec<_> = (1..=n)
            .map(|i| run_round1(PartyIndex(i), t, n, &session).unwrap())
            .collect();
        (session, results)
    }

    #[test]
    fn test_round2_basic_3_of_5() {
        let (session, r1_results) = setup_round1(5, 3);
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();

        // Parti 1 için Round 2 çalıştır
        let (_, r1_state) = r1_results.into_iter().next().unwrap();
        let (broadcast, p2p_msgs, _state) =
            run_round2(r1_state, r1_msgs).unwrap();

        assert_eq!(broadcast.from, PartyIndex(1));
        assert_eq!(broadcast.vss_commitment.points.len(), 3); // threshold=3
        assert_eq!(p2p_msgs.len(), 4); // 5-1 = 4 parti
    }

    #[test]
    fn test_round2_p2p_targets_correct() {
        let (session, r1_results) = setup_round1(5, 3);
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();
        let (_, r1_state) = r1_results.into_iter().next().unwrap();
        let (_, p2p_msgs, _) = run_round2(r1_state, r1_msgs).unwrap();

        // Parti 1 kendine mesaj göndermemeli
        for msg in &p2p_msgs {
            assert_ne!(msg.to, PartyIndex(1));
            assert_eq!(msg.from, PartyIndex(1));
        }

        // Her hedef parti için bir mesaj olmalı
        let targets: std::collections::HashSet<u32> =
            p2p_msgs.iter().map(|m| m.to.0).collect();
        assert_eq!(targets, (2..=5).collect());
    }

    #[test]
    fn test_round2_broadcast_verification() {
        let (session, r1_results) = setup_round1(3, 2);
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();
        let (_, r1_state) = r1_results.into_iter().next().unwrap();
        let (broadcast, _, _) = run_round2(r1_state, r1_msgs.clone()).unwrap();

        // Doğrulama başarılı olmalı
        assert!(
            verify_round2_broadcast(&broadcast, &r1_msgs, &session, 2).is_ok(),
            "geçerli broadcast doğrulanmalı"
        );
    }

    #[test]
    fn test_round2_tampered_commitment_rejected() {
        let (session, r1_results) = setup_round1(3, 2);
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();
        let (_, r1_state) = r1_results.into_iter().next().unwrap();
        let (mut broadcast, _, _) = run_round2(r1_state, r1_msgs.clone()).unwrap();

        // VSS taahhüdünü boz
        broadcast.vss_commitment.points[0][5] ^= 0xFF;

        assert!(
            verify_round2_broadcast(&broadcast, &r1_msgs, &session, 2).is_err(),
            "bozuk taahhüt reddedilmeli"
        );
    }

    #[test]
    fn test_round2_p2p_ciphertext_nonempty() {
        let (_, r1_results) = setup_round1(3, 2);
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();
        let (_, r1_state) = r1_results.into_iter().next().unwrap();
        let (_, p2p_msgs, _) = run_round2(r1_state, r1_msgs).unwrap();

        for msg in &p2p_msgs {
            assert!(!msg.encrypted_share_ciphertext.is_empty());
            assert!(verify_round2_p2p_format(msg).is_ok());
        }
    }
}
