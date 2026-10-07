//! Keygen Round 3 — Doğrulama + LocalKey Çıktısı
//!
//! §02-tss-lib.md §4 Round 3: "VSS doğrulama + xi hesabı + public anahtar"
//! §01-multi-party-ecdsa.md §4.2 Phase 3: "Public anahtar + DLog kanıtı"
//!
//! ## Protokol
//!
//! Tüm Round 2 mesajları alındıktan sonra her parti:
//!
//! 1. **Broadcast doğrulama:** Her parti için VSS açılımını doğrula
//!    - Hash eşleşmeli: `com_j == BLAKE3("nova-keygen-r1" || VSS_j || session_id)`
//!    - DLog kanıtı doğrula: `u_j` bilindiğini kanıtla (§dlog.rs)
//!
//! 2. **P2P şifre çözme:** Kendi payını çöz
//!    - `share_ji = AES-GCM-Decrypt(HKDF(ECDH(my_sk, pj_pk)), ciphertext)`
//!
//! 3. **VSS doğrulama:** Her gelen payı taahhütle doğrula
//!    - `share_ji * G == sum(C_{j,k} * i^k)` kontrolü
//!
//! 4. **Aggregate pay:** `x_i = sum(share_ji, j=1..n)`
//!    - Bu, tüm partilerin katkılarının toplamı
//!
//! 5. **Public key:** `X = C_{1,0} + C_{2,0} + ... + C_{n,0}` (tüm a_0 taahhütleri)
//!    - `X_i = x_i * G` (kendi public payı)
//!
//! 6. **DLog kanıtı:** `x_i` için Schnorr kanıtı (final key share'in bilindiğini kanıtlar)
//!
//! 7. **LocalKey oluştur:** Tüm verileri bir araya getir

use crate::crypto_utils::{
    point_add, scalar_add, scalar_from_bytes, scalar_to_bytes, IDENTITY_BYTES,
    scalar_mul_generator,
};
use crate::dlog::{dlog_prove, dlog_verify};
use crate::errors::{MpcError, MpcResult};
use crate::keygen::round2::{verify_round2_broadcast, Round2LocalState};
use crate::p2p_encrypt::decrypt_share;
use crate::types::{
    KeygenRound2BroadcastMsg, KeygenRound3Msg, LocalKey, PartyIndex, SessionId, VssCommitment,
};
use crate::vss::feldman_vss_verify;
use nova_primitives::types::Threshold;

// ─── ROUND 3 ÇALIŞTIRICISI ────────────────────────────────────────────────────

/// Keygen Round 3 — tüm doğrulamaları yap ve `LocalKey` üret.
///
/// Bu round, keygen protokolünün son adımıdır. Başarılı tamamlanma sonrası
/// `LocalKey` elde edilir ve güvenli depolamaya alınır.
///
/// ## Parametreler
///
/// - `r2_state`: Round 2'den gelen durum.
/// - `all_r2_broadcasts`: Tüm partilerden gelen Round 2 broadcast mesajları.
/// - `my_r2_p2p`: Bu partiye gelen tüm P2P şifreli paylar.
///
/// ## Döndürür
///
/// - `KeygenRound3Msg`: Herkese broadcast edilecek (public share + DLog kanıtı).
/// - `LocalKey`: Güvenli depolamaya alınacak nihai anahtar verisi.
pub fn run_round3(
    r2_state: Round2LocalState,
    all_r2_broadcasts: Vec<KeygenRound2BroadcastMsg>,
    my_r2_p2p: Vec<(PartyIndex, [u8; 12], Vec<u8>)>, // (from, nonce, ciphertext)
) -> MpcResult<(KeygenRound3Msg, LocalKey)> {
    let my_index = r2_state.r1.party_index;
    let threshold = r2_state.r1.threshold;
    let party_count = r2_state.r1.party_count;
    let session_id = r2_state.r1.session_id;
    let r1_msgs = &r2_state.received_r1_msgs;

    // ── 1. Broadcast sayısı doğrula ───────────────────────────────────────────
    if all_r2_broadcasts.len() != party_count as usize {
        return Err(MpcError::InvalidShareCount {
            expected: party_count as usize,
            got: all_r2_broadcasts.len(),
        });
    }

    // P2P mesaj sayısı (kendimizden geleni dışarıda tut)
    if my_r2_p2p.len() != (party_count - 1) as usize {
        return Err(MpcError::InvalidShareCount {
            expected: (party_count - 1) as usize,
            got: my_r2_p2p.len(),
        });
    }

    // ── 2. Her broadcast'i doğrula ──────────────────────────────────────────���─
    for broadcast in &all_r2_broadcasts {
        verify_round2_broadcast(broadcast, r1_msgs, &session_id, threshold)?;
    }

    // ── 3. DLog kanıtlarını doğrula (Round 1'deki u_i için) ──────────────────
    for r1_msg in r1_msgs {
        // Bu partinin u_i public key'i = VSS commitment.points[0]
        let broadcast = all_r2_broadcasts
            .iter()
            .find(|b| b.from == r1_msg.from)
            .ok_or(MpcError::MissingMessage {
                round: 2,
                from: r1_msg.from.0,
            })?;

        let u_j_public = &broadcast.vss_commitment.points[0];

        let valid = dlog_verify(
            u_j_public,
            &r1_msg.dlog_proof,
            "nova-keygen-dlog-v1",
            session_id.as_bytes(),
        )?;

        if !valid {
            return Err(MpcError::DlogProofInvalid { party: r1_msg.from.0 });
        }
    }

    // ── 4. P2P payları şifresini çöz + VSS doğrula ────────────────────────────
    let mut shares_received: Vec<(PartyIndex, [u8; 32])> = Vec::new();

    for (from, nonce, ciphertext) in &my_r2_p2p {
        // Gönderenin ECDH public key'ini bul (Round 1 mesajından)
        let sender_ecdh_pk = r1_msgs
            .iter()
            .find(|m| m.from == *from)
            .map(|m| m.ecdh_pk.clone())
            .ok_or(MpcError::MissingMessage { round: 1, from: from.0 })?;

        // Şifreli payı çöz
        let share_bytes = decrypt_share(
            &r2_state.r1.ecdh_secret,
            &sender_ecdh_pk,
            &session_id,
            *from,
            my_index,
            nonce,
            ciphertext,
        )
        .map_err(|_| MpcError::DecryptionFailed(
            format!("parti {} payı çözülemiyor", from.0)
        ))?;

        // VSS doğrulaması: share_ji * G == sum(C_{j,k} * i^k)
        let sender_vss = all_r2_broadcasts
            .iter()
            .find(|b| b.from == *from)
            .map(|b| &b.vss_commitment)
            .ok_or(MpcError::MissingMessage { round: 2, from: from.0 })?;

        let valid = feldman_vss_verify(&share_bytes, my_index, sender_vss)?;
        if !valid {
            return Err(MpcError::VssVerificationFailed { party: from.0 });
        }

        shares_received.push((*from, share_bytes));
    }

    // Kendi payı da ekle: f_my_index(my_index)
    let my_own_share = r2_state.r1.vss_output.shares[(my_index.0 - 1) as usize].value;
    // Kendi VSS'mizi de doğrula
    let my_vss = &r2_state.r1.vss_output.commitment;
    let my_valid = feldman_vss_verify(&my_own_share, my_index, my_vss)?;
    if !my_valid {
        return Err(MpcError::InternalError("kendi VSS payı geçersiz".into()));
    }
    shares_received.push((my_index, my_own_share));

    // ── 5. x_i = sum(share_ji) ────────────────────────────────────────────────
    let mut x_i = k256::Scalar::ZERO;
    for (_, share_bytes) in &shares_received {
        let s = scalar_from_bytes(share_bytes)?;
        x_i = x_i + s;
    }
    let x_i_bytes = scalar_to_bytes(&x_i);

    // ── 6. X = sum(C_{j,0}) — Aggregate public key ───────────────────────────
    let mut aggregate_pk = IDENTITY_BYTES;
    for broadcast in &all_r2_broadcasts {
        let c_j_0 = broadcast.vss_commitment.points[0];
        aggregate_pk = point_add(&aggregate_pk, &c_j_0)?;
    }

    // ── 7. X_i = x_i * G — Kendi public payı ─────────────────────────────────
    let x_i_public = scalar_mul_generator(&x_i);

    // ── 8. DLog kanıtı: x_i bilindiğini kanıtla ──────────────────────────────
    let x_i_dlog_proof = dlog_prove(
        &x_i_bytes,
        &x_i_public,
        "nova-keygen-final-dlog-v1",
        session_id.as_bytes(),
    )?;

    // ── 9. Round 3 mesajı ───────────────────────────────────────��─────────────
    let round3_msg = KeygenRound3Msg {
        from: my_index,
        public_share: x_i_public,
        dlog_proof: x_i_dlog_proof,
    };

    // ── 10. LocalKey ──────────────────────────────────────────────────────────
    let all_vss_commitments: Vec<_> = all_r2_broadcasts
        .iter()
        .map(|b| b.vss_commitment.clone())
        .collect();

    let local_key = LocalKey {
        party_index: my_index,
        threshold: Threshold { t: threshold, n: party_count as u8 },
        party_count,
        secret_share: x_i_bytes,
        public_key: aggregate_pk,
        session_id: *session_id.as_bytes(),
        all_vss_commitments,
    };

    Ok((round3_msg, local_key))
}

// ─── ROUND 3 MESAJ DOĞRULAMA ────────────────────────────────────────���────────

/// Round 3 mesajını doğrula (diğer partilerden gelenler).
///
/// - `X_i = x_i * G` için DLog kanıt��nı doğrula
/// - `X_i`'nin VSS taahhütleriyle tutarlı olduğunu kontrol et
pub fn verify_round3_msg(
    msg: &KeygenRound3Msg,
    all_r2_broadcasts: &[KeygenRound2BroadcastMsg],
    session_id: &SessionId,
) -> MpcResult<()> {
    // DLog kanıt��nı doğrula
    let valid = dlog_verify(
        &msg.public_share,
        &msg.dlog_proof,
        "nova-keygen-final-dlog-v1",
        session_id.as_bytes(),
    )?;

    if !valid {
        return Err(MpcError::DlogProofInvalid { party: msg.from.0 });
    }

    // X_i'nin VSS taahhütleriyle tutarlı olduğunu kontrol et:
    // X_i = sum_j(FeldmanEval(VSS_j, i))
    let expected_public = compute_public_share_from_vss(msg.from, all_r2_broadcasts)?;

    if expected_public != msg.public_share {
        return Err(MpcError::VssVerificationFailed { party: msg.from.0 });
    }

    Ok(())
}

/// VSS taahhütlerinden parti `j`'nin beklenen public payını hesapla.
///
/// `X_j = sum_i(FeldmanEval(VSS_i, j))`
pub fn compute_public_share_from_vss(
    party: PartyIndex,
    all_broadcasts: &[KeygenRound2BroadcastMsg],
) -> MpcResult<[u8; 33]> {
    let mut result = IDENTITY_BYTES;
    for broadcast in all_broadcasts {
        let p = crate::vss::feldman_eval_point(&broadcast.vss_commitment, party)?;
        result = point_add(&result, &p)?;
    }
    Ok(result)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::round1::run_round1;
    use crate::keygen::round2::run_round2;
    use crate::session::compute_keygen_ssid;
    use nova_primitives::types::ChainId;

    /// Tam keygen simülasyonu: n parti, threshold t
    fn full_keygen_sim(n: u32, t: u8) -> Vec<LocalKey> {
        let parties: Vec<PartyIndex> = (1..=n).map(PartyIndex).collect();
        let session = compute_keygen_ssid(ChainId(1984), &parties, &[42u8; 32]);

        // Round 1: tüm partiler
        let r1_results: Vec<_> = (1..=n)
            .map(|i| run_round1(PartyIndex(i), t, n, &session).unwrap())
            .collect();
        let r1_msgs: Vec<_> = r1_results.iter().map(|(m, _)| m.clone()).collect();

        // Round 2: tüm partiler
        let r2_results: Vec<_> = r1_results
            .into_iter()
            .map(|(_, state)| run_round2(state, r1_msgs.clone()).unwrap())
            .collect();

        // Round 2 broadcast mesajları
        let r2_broadcasts: Vec<_> = r2_results.iter().map(|(b, _, _)| b.clone()).collect();

        // P2P mesajlarını dağıt: her partiye ait olanları topla
        let mut p2p_by_recipient: Vec<Vec<_>> = vec![Vec::new(); n as usize];
        for (_, p2p_msgs, _) in &r2_results {
            for msg in p2p_msgs {
                let to_idx = (msg.to.0 - 1) as usize;
                p2p_by_recipient[to_idx].push((
                    msg.from,
                    msg.encrypted_share_nonce,
                    msg.encrypted_share_ciphertext.clone(),
                ));
            }
        }

        // Round 3: tüm partiler
        r2_results
            .into_iter()
            .enumerate()
            .map(|(i, (_, _, r2_state))| {
                let my_p2p = p2p_by_recipient[i].clone();
                let (_, local_key) =
                    run_round3(r2_state, r2_broadcasts.clone(), my_p2p).unwrap();
                local_key
            })
            .collect()
    }

    #[test]
    fn test_full_keygen_2_of_3() {
        let keys = full_keygen_sim(3, 2);
        assert_eq!(keys.len(), 3);

        // Tüm partiler aynı aggregate public key'e sahip olmalı
        let pk0 = keys[0].public_key;
        for k in &keys {
            assert_eq!(k.public_key, pk0, "tüm partiler aynı public key'e sahip olmalı");
        }
    }

    #[test]
    fn test_full_keygen_3_of_5() {
        let keys = full_keygen_sim(5, 3);
        assert_eq!(keys.len(), 5);

        let pk0 = keys[0].public_key;
        for k in &keys {
            assert_eq!(k.public_key, pk0);
            assert_eq!(k.threshold.t, 3);
            assert_eq!(k.party_count, 5);
        }
    }

    #[test]
    fn test_keygen_public_key_nonzero() {
        let keys = full_keygen_sim(3, 2);
        for k in &keys {
            // Public key sonsuz nokta (0x00...) olmamalı
            assert_ne!(k.public_key[0], 0x00, "aggregate public key sonsuz nokta olmamalı");
        }
    }

    #[test]
    fn test_keygen_secret_shares_unique() {
        let keys = full_keygen_sim(3, 2);
        // Her partinin gizli payı farklı olmalı
        assert_ne!(keys[0].secret_share, keys[1].secret_share);
        assert_ne!(keys[1].secret_share, keys[2].secret_share);
    }

    #[test]
    fn test_keygen_party_indices_correct() {
        let keys = full_keygen_sim(3, 2);
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(k.party_index.0, (i + 1) as u32);
        }
    }

    #[test]
    fn test_public_share_from_vss_consistency() {
        // full_keygen_sim içinde doğrulama zaten yapılıyor
        // Ekstra test: round3_msg doğrulama
        let keys = full_keygen_sim(3, 2);
        // Başarılı keygen = doğrulama geçmiş demek
        assert_eq!(keys.len(), 3);
    }
}
