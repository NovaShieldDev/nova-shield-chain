//! Reshare Round 1 — Eski Komite: Yeni VSS Üretimi
//!
//! §5.4: "Epoch rotasyonu: her epoch başında guardian key payları reshare edilir."
//! §01-multi-party-ecdsa.md §4.5: Resharing protocol referans alındı.
//! §02-tss-lib.md §5 Resharing: "New committee aggregate + verify" referans alındı.
//!
//! ## Protokol
//!
//! Eski komite üyesi P_i (gizli pay x_i sahibi):
//!
//! 1. Yeni VSS polinomu oluştur: `f_i(x) = x_i + a_{i,1}*x + ... + a_{i,t_new-1}*x^{t_new-1}`
//!    - `f_i(0) = x_i` (kendi gizli payı — sır)
//!    - `f_i(j)` = yeni komite üyesi j'ye gönderilecek reshare payı
//! 2. VSS taahhütleri: `C_{i,k} = a_{i,k} * G` → broadcast
//! 3. DLog kanıtı: `x_i` bilindiğini kanıtla (C_{i,0} = x_i * G için)
//! 4. Ephemeral ECDH keypair üret → yeni komite P2P şifreleme için
//! 5. Her yeni parti j için: `f_i(j)` hesapla ve şifrele
//!    - Şifreleme: `AES-256-GCM(HKDF(ECDH(old_ecdh_sk, new_ecdh_pk_j)), f_i(j))`
//!
//! ## Güvenlik
//!
//! - `f_i(0) = x_i`: Eski pay sıfır noktasında korunur → kurtarma imkansız
//! - DLog kanıtı: Eski parti gerçekten x_i'yi bildiğini kanıtlar
//! - ECDH forward secrecy: Her reshare için yeni ephemeral keypair
//! - Feldman VSS: Yeni komite her payın geçerliliğini bağımsız doğrular

use crate::crypto_utils::{scalar_mul_generator, scalar_to_bytes};
use crate::dlog::dlog_prove;
use crate::errors::{MpcError, MpcResult};
use crate::p2p_encrypt::{encrypt_share, EcdhSecret};
use crate::types::{
    DlogProofBytes, EcdhPublicKey, LocalKey, PartyIndex, ReshareRound1Msg,
    ReshareRound2P2pMsg, SessionId, VssCommitment,
};
use crate::vss::feldman_vss_generate;

// ─── ROUND 1 YEREL DURUMU ─────────────────────────────────────────────────────

/// Reshare Round 1 sonrası yerel durum — Round 2'ye taşınır.
#[derive(Debug)]
pub struct ReshareRound1State {
    /// Bu eski komite üyesinin indeksi.
    pub old_party_index: PartyIndex,
    /// Eski komite gizli payı `x_i`.
    pub old_secret_share: [u8; 32],
    /// `x_i * G` (public verification için).
    pub old_share_public: [u8; 33],
    /// Yeni VSS çıktısı (f_i polinom değerleri + taahhütler).
    pub new_vss_output: crate::vss::VssOutput,
    /// Bu oturum için üretilen ephemeral ECDH secret.
    pub ecdh_secret: EcdhSecret,
    /// Session ID.
    pub session_id: SessionId,
    /// Yeni komite boyutu.
    pub new_party_count: u32,
    /// Yeni threshold.
    pub new_threshold: u8,
    /// Eski komite imzacı listesi (Lagrange katsayısı hesabı için).
    pub old_signers: Vec<PartyIndex>,
}

/// Broadcast için eski parti ECDH public key'i ile birleşik paket.
///
/// `ReshareRound1Msg` P2P şifreleme doğrulaması için yeni partilere
/// eski partinin ECDH public key'ini de taşımalıdır.
#[derive(Clone, Debug)]
pub struct ReshareRound1Bundle {
    /// Broadcast mesajı (VSS commitment + DLog proof).
    pub msg: ReshareRound1Msg,
    /// Bu eski partinin ephemeral ECDH public key'i.
    /// → Yeni partiler P2P şifre çözme için bunu kullanır.
    pub old_ecdh_pk: EcdhPublicKey,
}

// ─── ROUND 1 ÇALIŞTIRICISI ────────────────────────────────────────────────────

/// Reshare Round 1'i çalıştır.
///
/// Eski komite üyesi P_i çalıştırır.
///
/// ## Parametreler
///
/// - `local_key`: Eski komite LocalKey'i.
/// - `old_signers`: Reshare'e katılan eski komite indeksleri (threshold kadar).
/// - `new_party_count`: Yeni komite büyüklüğü.
/// - `new_threshold`: Yeni eşik parametresi.
/// - `new_party_ecdh_pks`: Yeni komite üyelerinin ECDH public key'leri.
/// - `session_id`: Bu reshare oturumunun SSID'si.
///
/// ## Döndürür
///
/// - `ReshareRound1Bundle`: Herkese broadcast edilecek (VSS + DLog + ECDH pk).
/// - `Vec<ReshareRound2P2pMsg>`: Her yeni partiye gönderilecek şifreli paylar.
/// - `ReshareRound1State`: Round 2'ye taşınacak durum.
pub fn run_reshare_round1(
    local_key: &LocalKey,
    old_signers: Vec<PartyIndex>,
    new_party_count: u32,
    new_threshold: u8,
    new_party_ecdh_pks: &[(PartyIndex, EcdhPublicKey)],
    session_id: &SessionId,
) -> MpcResult<(ReshareRound1Bundle, Vec<ReshareRound2P2pMsg>, ReshareRound1State)> {
    let old_index = local_key.party_index;

    // ── 1. Parametre doğrulamaları ────────────────────────────────────────────
    if new_threshold == 0 || new_threshold as u32 > new_party_count {
        return Err(MpcError::InvalidThreshold {
            t: new_threshold,
            n: new_party_count as u8,
        });
    }
    if new_party_ecdh_pks.len() != new_party_count as usize {
        return Err(MpcError::InvalidShareCount {
            expected: new_party_count as usize,
            got: new_party_ecdh_pks.len(),
        });
    }
    if !old_signers.contains(&old_index) {
        return Err(MpcError::InvalidPartyIndex {
            index: old_index.0,
            max: old_signers.iter().map(|p| p.0).max().unwrap_or(0),
        });
    }

    // ── 2. Yeni VSS üret — f_i(0) = x_i (eski gizli pay) ────────────────────
    let new_vss_output = feldman_vss_generate(
        new_threshold,
        new_party_count,
        Some(local_key.secret_share),
        old_index,
    )?;

    // ── 3. DLog kanıtı: x_i bilindiğini kanıtla ──────────────────────────────
    // C_{i,0} = x_i * G = eski payın public versiyonu
    let old_share_public = scalar_mul_generator(
        &crate::crypto_utils::scalar_from_bytes(&local_key.secret_share)?
    );
    let dlog_proof = dlog_prove(
        &local_key.secret_share,
        &old_share_public,
        "nova-reshare-old-share-dlog-v1",
        session_id.as_bytes(),
    )?;

    // ── 4. Ephemeral ECDH keypair ─────────────────────────────────────────────
    let ecdh_secret = EcdhSecret::generate();
    let old_ecdh_pk = ecdh_secret.public_key();

    // ── 5. Her yeni partiye şifreli pay gönder ────────────────────────────────
    let mut p2p_msgs = Vec::with_capacity(new_party_count as usize);

    for (new_party, new_ecdh_pk) in new_party_ecdh_pks {
        // Bu yeni parti için VSS payı: f_i(new_party)
        // new_vss_output.shares[(new_party.0 - 1) as usize].value = f_i(new_party)
        let share_idx = (new_party.0 - 1) as usize;
        if share_idx >= new_vss_output.shares.len() {
            return Err(MpcError::InvalidPartyIndex {
                index: new_party.0,
                max: new_party_count,
            });
        }
        let share_bytes = &new_vss_output.shares[share_idx].value;

        let (nonce, ciphertext) = encrypt_share(
            &ecdh_secret,
            new_ecdh_pk,
            session_id,
            old_index,
            *new_party,
            share_bytes,
        )?;

        p2p_msgs.push(ReshareRound2P2pMsg {
            from: old_index,
            to: *new_party,
            encrypted_share_nonce: nonce,
            encrypted_share_ciphertext: ciphertext,
        });
    }

    // ── 6. Broadcast mesajı oluştur ───────────────────────────────────────────
    let r1_msg = ReshareRound1Msg {
        from: old_index,
        new_vss_commitment: new_vss_output.commitment.clone(),
        dlog_proof,
    };

    let bundle = ReshareRound1Bundle {
        msg: r1_msg,
        old_ecdh_pk,
    };

    let state = ReshareRound1State {
        old_party_index: old_index,
        old_secret_share: local_key.secret_share,
        old_share_public,
        new_vss_output,
        ecdh_secret,
        session_id: *session_id,
        new_party_count,
        new_threshold,
        old_signers,
    };

    Ok((bundle, p2p_msgs, state))
}

// ─── DOĞRULAMA ────────────────────────────────────────────────────────────────

/// Yeni komite üyesi için Round 1 bundle'ını doğrula.
///
/// 1. VSS commitment boyutu `new_threshold` ile eşleşmeli
/// 2. DLog kanıtı: `old_share_public = C_{i,0}` için geçerli
/// 3. ECDH public key sıfır değil
pub fn verify_reshare_round1_bundle(
    bundle: &ReshareRound1Bundle,
    expected_public_key: &[u8; 33],
    old_signers: &[PartyIndex],
    session_id: &SessionId,
    new_threshold: u8,
) -> MpcResult<()> {
    let msg = &bundle.msg;

    // VSS boyutu doğrula
    if msg.new_vss_commitment.points.len() != new_threshold as usize {
        return Err(MpcError::InvalidShareCount {
            expected: new_threshold as usize,
            got: msg.new_vss_commitment.points.len(),
        });
    }

    // ECDH pk sıfır değil
    if bundle.old_ecdh_pk.compressed == [0u8; 33] {
        return Err(MpcError::InvalidPoint);
    }

    // C_{i,0} = eski payın public taahhüdü
    let c_i_0 = &msg.new_vss_commitment.points[0];

    // DLog kanıtı doğrula
    let valid = crate::dlog::dlog_verify(
        c_i_0,
        &msg.dlog_proof,
        "nova-reshare-old-share-dlog-v1",
        session_id.as_bytes(),
    )?;

    if !valid {
        return Err(MpcError::DlogProofInvalid { party: msg.from.0 });
    }

    // Commitment noktaları geçerli secp256k1 noktaları mı?
    for point in &msg.new_vss_commitment.points {
        crate::crypto_utils::point_from_bytes(point)
            .map_err(|_| MpcError::InvalidPoint)?;
    }

    Ok(())
}

/// Eski komite pay taahhütlerinin aggregate public key ile tutarlılığını doğrula.
///
/// `∑_{i∈old} λ_i · C_{i,0} == expected_public_key`
///
/// Bu doğrulama, yeni komitenin aynı gizli sırrı paylaştığını kanıtlar.
pub fn verify_reshare_public_key_consistency(
    bundles: &[ReshareRound1Bundle],
    old_signers: &[PartyIndex],
    expected_public_key: &[u8; 33],
    session_id: &SessionId,
) -> MpcResult<()> {
    use crate::crypto_utils::{point_add, scalar_mul_point, IDENTITY_BYTES};
    use crate::sign::online::compute_lagrange_coefficient;

    // Lagrange ağırlıklı ∑ λ_i * C_{i,0} hesapla
    let mut aggregate = IDENTITY_BYTES;
    for bundle in bundles {
        let lambda_i =
            compute_lagrange_coefficient(bundle.msg.from, old_signers)?;
        let c_i_0 = &bundle.msg.new_vss_commitment.points[0];
        let lambda_ci = scalar_mul_point(&lambda_i, c_i_0)?;
        aggregate = point_add(&aggregate, &lambda_ci)?;
    }

    if &aggregate != expected_public_key {
        return Err(MpcError::ResharePublicKeyMismatch);
    }

    Ok(())
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::{run_round1, run_round2, run_round3};
    use crate::session::{compute_keygen_ssid, compute_reshare_ssid};
    use crate::types::{KeygenRound1Msg, KeygenRound2BroadcastMsg, KeygenRound2P2pMsg};
    use nova_primitives::types::ChainId;

    const OLD_N: u32 = 3;
    const OLD_T: u8 = 2;
    const NEW_N: u32 = 4;
    const NEW_T: u8 = 3;

    fn run_keygen(n: u32, t: u8) -> (Vec<LocalKey>, SessionId) {
        let parties: Vec<PartyIndex> = (1..=n).map(PartyIndex).collect();
        let session = compute_keygen_ssid(ChainId(1984), &parties, &[0u8; 32]);

        let r1: Vec<_> = (1..=n)
            .map(|i| run_round1(PartyIndex(i), t, n, &session).unwrap())
            .collect();
        let r1_msgs: Vec<KeygenRound1Msg> = r1.iter().map(|(m, _)| m.clone()).collect();

        let r2: Vec<_> = r1
            .into_iter()
            .map(|(_, s)| run_round2(s, r1_msgs.clone()).unwrap())
            .collect();
        let broadcasts: Vec<KeygenRound2BroadcastMsg> = r2.iter().map(|(b, _, _)| b.clone()).collect();
        let all_p2p: Vec<Vec<KeygenRound2P2pMsg>> = r2.iter().map(|(_, p, _)| p.clone()).collect();

        let keys: Vec<_> = r2
            .into_iter()
            .enumerate()
            .map(|(idx, (_, _, r2_state))| {
                let my_idx = idx + 1;
                let my_p2p: Vec<_> = all_p2p
                    .iter()
                    .enumerate()
                    .filter_map(|(fi, plist)| {
                        if fi + 1 == my_idx { return None; }
                        plist.iter().find(|m| m.to == PartyIndex(my_idx as u32))
                            .map(|m| (m.from, m.encrypted_share_nonce, m.encrypted_share_ciphertext.clone()))
                    })
                    .collect();
                let (_, lk) = run_round3(r2_state, broadcasts.clone(), my_p2p).unwrap();
                lk
            })
            .collect();
        (keys, session)
    }

    fn setup_new_party_ecdh(n: u32) -> (Vec<(PartyIndex, EcdhPublicKey)>, Vec<EcdhSecret>) {
        let secrets: Vec<EcdhSecret> = (0..n).map(|_| EcdhSecret::generate()).collect();
        let pks: Vec<(PartyIndex, EcdhPublicKey)> = secrets
            .iter()
            .enumerate()
            .map(|(i, s)| (PartyIndex(i as u32 + 1), s.public_key()))
            .collect();
        (pks, secrets)
    }

    #[test]
    fn test_reshare_round1_basic() {
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)]; // threshold=2

        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            &old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        let (new_ecdh_pks, _) = setup_new_party_ecdh(NEW_N);

        let (bundle, p2p_msgs, _state) = run_reshare_round1(
            &old_keys[0],
            old_signers.clone(),
            NEW_N,
            NEW_T,
            &new_ecdh_pks,
            &reshare_session,
        ).unwrap();

        assert_eq!(bundle.msg.from, PartyIndex(1));
        assert_eq!(bundle.msg.new_vss_commitment.points.len(), NEW_T as usize);
        assert_ne!(bundle.old_ecdh_pk.compressed, [0u8; 33]);
        assert_eq!(p2p_msgs.len(), NEW_N as usize);

        // Her yeni parti için bir P2P mesajı olmalı
        for (i, msg) in p2p_msgs.iter().enumerate() {
            assert_eq!(msg.from, PartyIndex(1));
            assert_eq!(msg.to, PartyIndex(i as u32 + 1));
            assert!(!msg.encrypted_share_ciphertext.is_empty());
        }
    }

    #[test]
    fn test_reshare_round1_vss_commitment_size() {
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];

        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            &old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        let (new_ecdh_pks, _) = setup_new_party_ecdh(NEW_N);

        // Farklı threshold'lar için test
        for new_t in [2u8, 3, 4] {
            if new_t as u32 > NEW_N { continue; }
            let (bundle, _, _) = run_reshare_round1(
                &old_keys[0],
                old_signers.clone(),
                NEW_N,
                new_t,
                &new_ecdh_pks,
                &reshare_session,
            ).unwrap();
            assert_eq!(
                bundle.msg.new_vss_commitment.points.len(),
                new_t as usize,
                "VSS commitment boyutu threshold ile eşleşmeli"
            );
        }
    }

    #[test]
    fn test_reshare_round1_bundle_verification() {
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];

        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            &old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        let (new_ecdh_pks, _) = setup_new_party_ecdh(NEW_N);

        let (bundle, _, _) = run_reshare_round1(
            &old_keys[0],
            old_signers.clone(),
            NEW_N,
            NEW_T,
            &new_ecdh_pks,
            &reshare_session,
        ).unwrap();

        let pk = old_keys[0].public_key;
        assert!(
            verify_reshare_round1_bundle(
                &bundle, &pk, &old_signers, &reshare_session, NEW_T
            ).is_ok(),
            "geçerli bundle doğrulanmalı"
        );
    }

    #[test]
    fn test_reshare_round1_tampered_dlog_rejected() {
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];

        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            &old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        let (new_ecdh_pks, _) = setup_new_party_ecdh(NEW_N);

        let (mut bundle, _, _) = run_reshare_round1(
            &old_keys[0],
            old_signers.clone(),
            NEW_N,
            NEW_T,
            &new_ecdh_pks,
            &reshare_session,
        ).unwrap();

        // DLog kanıtını boz
        bundle.msg.dlog_proof.response[10] ^= 0xFF;

        let pk = old_keys[0].public_key;
        assert!(
            verify_reshare_round1_bundle(
                &bundle, &pk, &old_signers, &reshare_session, NEW_T
            ).is_err(),
            "bozuk DLog kanıtı reddedilmeli"
        );
    }

    #[test]
    fn test_reshare_invalid_threshold_rejected() {
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];

        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            &old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        let (new_ecdh_pks, _) = setup_new_party_ecdh(NEW_N);

        // Threshold > party count → hata
        let result = run_reshare_round1(
            &old_keys[0],
            old_signers.clone(),
            NEW_N,
            255, // geçersiz threshold
            &new_ecdh_pks,
            &reshare_session,
        );
        assert!(result.is_err(), "geçersiz threshold reddedilmeli");
    }
}
