//! Reshare Round 2 — Yeni Komite: Payları Topla ve Doğrula
//!
//! §5.4: "Epoch rotasyonu: yeni guardian seti aynı gizli sırı paylaşır."
//! §01-multi-party-ecdsa.md §4.5: Resharing yeni komite adımları.
//! §02-tss-lib.md §5: "New committee receive + verify + aggregate".
//!
//! ## Protokol
//!
//! Yeni komite üyesi P'_j:
//!
//! 1. Tüm eski komite bundle'larını (VSS commitment + DLog proof) topla
//! 2. Her bundle'ı doğrula:
//!    a. VSS boyutu new_threshold ile eşleşmeli
//!    b. DLog kanıtı: `C_{i,0} = x_i * G` geçerli
//! 3. P2P payları şifreden çöz: `f_i(j) = AES-GCM-Decrypt(...)`
//! 4. Her payı VSS ile doğrula: `f_i(j) * G == ∑_k C_{i,k} * j^k`
//! 5. Lagrange ağırlıklı yeni pay hesapla:
//!    `x'_j = ∑_{i∈old} λ_i(old) · f_i(j)`
//! 6. Public key tutarlılığını doğrula:
//!    `∑_{i∈old} λ_i(old) · C_{i,0} == expected_public_key`
//! 7. Yeni `LocalKey` üret
//!
//! ## Lagrange Kurtarma Kanıtı
//!
//! Yeni komite aynı gizli sırrı paylaşır çünkü:
//!
//! ```text
//! ∑_{j∈new} λ'_j · x'_j
//! = ∑_j λ'_j · ∑_i λ_i · f_i(j)
//! = ∑_i λ_i · ∑_j λ'_j · f_i(j)  [değiştirilme]
//! = ∑_i λ_i · f_i(0)               [Lagrange interpolasyonu]
//! = ∑_i λ_i · x_i                  [f_i(0) = x_i]
//! = s                               [orijinal sır] ✓
//! ```

use crate::crypto_utils::{
    scalar_add, scalar_from_bytes, scalar_mul, scalar_mul_generator,
    scalar_to_bytes, IDENTITY_BYTES,
};
use crate::errors::{MpcError, MpcResult};
use crate::p2p_encrypt::{decrypt_share, EcdhSecret};
use crate::reshare::round1::{
    verify_reshare_public_key_consistency, verify_reshare_round1_bundle, ReshareRound1Bundle,
};
use crate::sign::online::compute_lagrange_coefficient;
use crate::types::{
    LocalKey, PartyIndex, ReshareRound2P2pMsg, SessionId, VssCommitment,
};
use crate::vss::feldman_vss_verify;
use nova_primitives::types::Threshold;

// ─── ROUND 2 ÇALIŞTIRICISI ────────────────────────────────────────────────────

/// Reshare Round 2'yi çalıştır.
///
/// Yeni komite üyesi P'_j çalıştırır.
///
/// ## Parametreler
///
/// - `new_party_index`: Yeni komitedeki bu partinin indeksi.
/// - `new_threshold`: Yeni eşik parametresi.
/// - `new_party_count`: Yeni komite büyüklüğü.
/// - `new_ecdh_secret`: Bu yeni partinin ECDH gizli anahtarı (şifre çözme için).
/// - `all_r1_bundles`: Tüm eski komite üyelerinden gelen bundle'lar.
/// - `my_p2p_msgs`: Bu yeni partiye gönderilen P2P şifreli paylar.
/// - `old_signers`: Reshare'e katılan eski komite indeksleri.
/// - `expected_public_key`: Beklenen aggregate public key (değişmemeli).
/// - `session_id`: Bu reshare oturumunun SSID'si.
///
/// ## Döndürür
///
/// Yeni `LocalKey` — aynı aggregate public key, yeni secret share ile.
pub fn run_reshare_round2(
    new_party_index: PartyIndex,
    new_threshold: u8,
    new_party_count: u32,
    new_ecdh_secret: &EcdhSecret,
    all_r1_bundles: Vec<ReshareRound1Bundle>,
    my_p2p_msgs: Vec<ReshareRound2P2pMsg>,
    old_signers: &[PartyIndex],
    expected_public_key: &[u8; 33],
    session_id: &SessionId,
    old_keygen_session_id: &[u8; 32],
) -> MpcResult<LocalKey> {
    let old_count = old_signers.len();

    // ── 1. Bundle sayısı doğrula ──────────────────────────────────────────────
    if all_r1_bundles.len() != old_count {
        return Err(MpcError::InvalidShareCount {
            expected: old_count,
            got: all_r1_bundles.len(),
        });
    }

    // P2P mesaj sayısı doğrula
    if my_p2p_msgs.len() != old_count {
        return Err(MpcError::InvalidShareCount {
            expected: old_count,
            got: my_p2p_msgs.len(),
        });
    }

    // ── 2. Her bundle'ı doğrula (DLog + VSS boyutu) ───────────────────────────
    for bundle in &all_r1_bundles {
        verify_reshare_round1_bundle(
            bundle,
            expected_public_key,
            old_signers,
            session_id,
            new_threshold,
        )?;
    }

    // ── 3. Public key tutarlılığını doğrula ───────────────────────────────────
    // ∑_{i∈old} λ_i · C_{i,0} == expected_public_key
    verify_reshare_public_key_consistency(
        &all_r1_bundles,
        old_signers,
        expected_public_key,
        session_id,
    )?;

    // ── 4. P2P payları şifreden çöz + VSS doğrulama ──────────────────────────
    let mut decrypted_shares: Vec<(PartyIndex, [u8; 32])> = Vec::with_capacity(old_count);

    for p2p_msg in &my_p2p_msgs {
        // Alıcı kontrolü
        if p2p_msg.to != new_party_index {
            return Err(MpcError::UnexpectedMessage {
                round: 2,
                from: p2p_msg.from.0,
            });
        }

        // Bu eski partinin bundle'ını bul
        let bundle = all_r1_bundles
            .iter()
            .find(|b| b.msg.from == p2p_msg.from)
            .ok_or(MpcError::MissingMessage {
                round: 1,
                from: p2p_msg.from.0,
            })?;

        // Şifre çöz: f_i(j)
        let share = decrypt_share(
            new_ecdh_secret,
            &bundle.old_ecdh_pk,
            session_id,
            p2p_msg.from,
            new_party_index,
            &p2p_msg.encrypted_share_nonce,
            &p2p_msg.encrypted_share_ciphertext,
        )?;

        // VSS doğrulama: f_i(j) * G == ∑_k C_{i,k} * j^k
        let valid = feldman_vss_verify(
            &share,
            new_party_index,
            &bundle.msg.new_vss_commitment,
        )?;

        if !valid {
            return Err(MpcError::VssVerificationFailed {
                party: p2p_msg.from.0,
            });
        }

        decrypted_shares.push((p2p_msg.from, share));
    }

    // ── 5. Yeni gizli pay: x'_j = ∑_{i∈old} λ_i · f_i(j) ───────────────────
    let mut new_secret_share_scalar = k256::Scalar::ZERO;

    for (old_index, share_bytes) in &decrypted_shares {
        // λ_i = Lagrange katsayısı (eski komite için)
        let lambda_i = compute_lagrange_coefficient(*old_index, old_signers)?;

        // f_i(j) skaleri
        let f_i_j = scalar_from_bytes(share_bytes)?;

        // λ_i · f_i(j) ekle
        let contribution = scalar_mul(&lambda_i, &f_i_j);
        new_secret_share_scalar = new_secret_share_scalar + contribution;
    }

    let new_secret_share = scalar_to_bytes(&new_secret_share_scalar);

    // ── 6. Yeni VSS taahhütleri listesi ──────────────────────────────────────
    // Yeni LocalKey için tüm eski VSS taahhütlerini topla
    let new_vss_commitments: Vec<VssCommitment> = all_r1_bundles
        .into_iter()
        .map(|b| b.msg.new_vss_commitment)
        .collect();

    // ── 7. Yeni LocalKey oluştur ──────────────────────────────────────────────
    let new_local_key = LocalKey {
        party_index: new_party_index,
        threshold: Threshold { t: new_threshold, n: new_party_count as u8 },
        party_count: new_party_count,
        secret_share: new_secret_share,
        public_key: *expected_public_key,
        session_id: *session_id.as_bytes(),
        all_vss_commitments: new_vss_commitments,
    };

    Ok(new_local_key)
}

// ─── DOĞRULAMA ────────────────────────────────────────────────────────────────

/// Yeni LocalKey'in aggregate public key'i koruduğunu doğrula.
///
/// Reshare sonrası yeni komite aynı `X = s * G`'ye sahip olmalı.
/// Bu fonksiyon yeni LocalKey'i `expected_public_key` ile karşılaştırır.
pub fn verify_reshared_key_public(
    new_local_key: &LocalKey,
    expected_public_key: &[u8; 33],
) -> bool {
    &new_local_key.public_key == expected_public_key
}

/// Yeni LocalKey'in public payını VSS taahhütlerden türet ve doğrula.
///
/// `x'_j * G == ∑_i λ_i · ∑_k C_{i,k} · j^k`
pub fn verify_new_public_share_from_vss(
    new_local_key: &LocalKey,
    old_signers: &[PartyIndex],
) -> MpcResult<bool> {
    use crate::crypto_utils::point_add;

    let j = new_local_key.party_index;

    // ∑_{i∈old} λ_i · VSS_eval(C_i, j) hesapla
    let mut computed_public_share = IDENTITY_BYTES;

    for vss in &new_local_key.all_vss_commitments {
        let lambda_i = compute_lagrange_coefficient(vss.party, old_signers)?;
        let vss_eval = crate::vss::feldman_eval_point(vss, j)?;
        let lambda_vss = crate::crypto_utils::scalar_mul_point(&lambda_i, &vss_eval)?;
        computed_public_share = point_add(&computed_public_share, &lambda_vss)?;
    }

    // Bu, x'_j * G ile eşit olmalı
    let actual_public_share = scalar_mul_generator(
        &scalar_from_bytes(&new_local_key.secret_share)?
    );

    Ok(computed_public_share == actual_public_share)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::{run_round1, run_round2, run_round3};
    use crate::p2p_encrypt::EcdhSecret;
    use crate::reshare::round1::{run_reshare_round1, ReshareRound1Bundle};
    use crate::session::{compute_keygen_ssid, compute_reshare_ssid};
    use crate::types::{EcdhPublicKey, KeygenRound1Msg, KeygenRound2BroadcastMsg, KeygenRound2P2pMsg};
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
        let broadcasts: Vec<KeygenRound2BroadcastMsg> =
            r2.iter().map(|(b, _, _)| b.clone()).collect();
        let all_p2p: Vec<Vec<KeygenRound2P2pMsg>> =
            r2.iter().map(|(_, p, _)| p.clone()).collect();

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

    struct ReshareSetup {
        bundles: Vec<(ReshareRound1Bundle, Vec<ReshareRound2P2pMsg>)>,
        new_ecdh_secrets: Vec<EcdhSecret>,
        reshare_session: SessionId,
        old_keygen_session_id: [u8; 32],
        expected_pk: [u8; 33],
        old_signers: Vec<PartyIndex>,
    }

    fn setup_reshare(
        old_keys: &[LocalKey],
        old_session: &SessionId,
        old_signers: Vec<PartyIndex>,
    ) -> ReshareSetup {
        let new_parties: Vec<PartyIndex> = (1..=NEW_N).map(PartyIndex).collect();
        let reshare_session = compute_reshare_ssid(
            old_session, &old_signers, &new_parties, NEW_T, &[0u8; 32]
        );

        // Yeni parti ECDH keypairs
        let new_ecdh_secrets: Vec<EcdhSecret> =
            (0..NEW_N).map(|_| EcdhSecret::generate()).collect();
        let new_ecdh_pks: Vec<(PartyIndex, EcdhPublicKey)> = new_ecdh_secrets
            .iter()
            .enumerate()
            .map(|(i, s)| (PartyIndex(i as u32 + 1), s.public_key()))
            .collect();

        // Her eski imzacı için round1 çalıştır
        let bundles: Vec<_> = old_signers
            .iter()
            .map(|&old_p| {
                let key_idx = (old_p.0 - 1) as usize;
                run_reshare_round1(
                    &old_keys[key_idx],
                    old_signers.clone(),
                    NEW_N,
                    NEW_T,
                    &new_ecdh_pks,
                    &reshare_session,
                ).unwrap()
            })
            .map(|(bundle, p2p, _state)| (bundle, p2p))
            .collect();

        ReshareSetup {
            bundles,
            new_ecdh_secrets,
            reshare_session,
            old_keygen_session_id: *old_session.as_bytes(),
            expected_pk: old_keys[0].public_key,
            old_signers,
        }
    }

    #[test]
    fn test_full_reshare_2_of_3_to_3_of_4() {
        // 2/3 eski komiteden 3/4 yeni komiteye tam reshare
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)]; // t=2 yeterli

        let setup = setup_reshare(&old_keys, &old_session, old_signers.clone());

        let all_bundles: Vec<ReshareRound1Bundle> =
            setup.bundles.iter().map(|(b, _)| b.clone()).collect();

        // Her yeni parti için round2 çalıştır
        let new_keys: Vec<LocalKey> = setup.new_ecdh_secrets
            .iter()
            .enumerate()
            .map(|(j_idx, ecdh_secret)| {
                let new_party = PartyIndex(j_idx as u32 + 1);

                // Bu yeni partiye gönderilen P2P mesajları
                let my_p2p: Vec<ReshareRound2P2pMsg> = setup.bundles
                    .iter()
                    .flat_map(|(_, p2p_list)| p2p_list.iter())
                    .filter(|m| m.to == new_party)
                    .cloned()
                    .collect();

                run_reshare_round2(
                    new_party,
                    NEW_T,
                    NEW_N,
                    ecdh_secret,
                    all_bundles.clone(),
                    my_p2p,
                    &old_signers,
                    &setup.expected_pk,
                    &setup.reshare_session,
                    &setup.old_keygen_session_id,
                ).unwrap()
            })
            .collect();

        // Tüm yeni anahtarlar aynı aggregate public key'i göstermeli
        let expected_pk = &old_keys[0].public_key;
        for (i, key) in new_keys.iter().enumerate() {
            assert_eq!(
                &key.public_key, expected_pk,
                "yeni parti {} aynı public key'e sahip olmalı",
                i + 1
            );
        }

        // Yeni threshold ve parti sayısı doğru olmalı
        assert_eq!(new_keys[0].threshold.t, NEW_T);
        assert_eq!(new_keys[0].party_count, NEW_N);
    }

    #[test]
    fn test_reshare_public_key_preserved() {
        // Reshare sonrası public key değişmemeli
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_pk = old_keys[0].public_key;
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];

        let setup = setup_reshare(&old_keys, &old_session, old_signers.clone());
        let all_bundles: Vec<ReshareRound1Bundle> =
            setup.bundles.iter().map(|(b, _)| b.clone()).collect();

        let my_p2p: Vec<ReshareRound2P2pMsg> = setup.bundles
            .iter()
            .flat_map(|(_, p2p)| p2p.iter())
            .filter(|m| m.to == PartyIndex(1))
            .cloned()
            .collect();

        let new_key = run_reshare_round2(
            PartyIndex(1), NEW_T, NEW_N,
            &setup.new_ecdh_secrets[0],
            all_bundles, my_p2p,
            &old_signers, &old_pk,
            &setup.reshare_session,
            &setup.old_keygen_session_id,
        ).unwrap();

        assert_eq!(new_key.public_key, old_pk, "public key reshare sonrası değişmemeli");
        assert!(verify_reshared_key_public(&new_key, &old_pk));
    }

    #[test]
    fn test_reshare_tampered_share_rejected() {
        // Şifreli pay bozulursa reddedilmeli (AES-GCM MAC doğrulama başarısız)
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];
        let old_pk = old_keys[0].public_key;

        let setup = setup_reshare(&old_keys, &old_session, old_signers.clone());
        let all_bundles: Vec<ReshareRound1Bundle> =
            setup.bundles.iter().map(|(b, _)| b.clone()).collect();

        let mut my_p2p: Vec<ReshareRound2P2pMsg> = setup.bundles
            .iter()
            .flat_map(|(_, p2p)| p2p.iter())
            .filter(|m| m.to == PartyIndex(1))
            .cloned()
            .collect();

        // İlk payı boz
        if let Some(msg) = my_p2p.first_mut() {
            if let Some(b) = msg.encrypted_share_ciphertext.first_mut() {
                *b ^= 0xFF;
            }
        }

        let result = run_reshare_round2(
            PartyIndex(1), NEW_T, NEW_N,
            &setup.new_ecdh_secrets[0],
            all_bundles, my_p2p,
            &old_signers, &old_pk,
            &setup.reshare_session,
            &setup.old_keygen_session_id,
        );

        assert!(result.is_err(), "bozuk şifreli pay reddedilmeli");
    }

    #[test]
    fn test_reshare_wrong_old_signers_rejected() {
        // Yanlış old_signers → public key tutarsızlığı hatası
        let (old_keys, old_session) = run_keygen(OLD_N, OLD_T);
        let old_signers = vec![PartyIndex(1), PartyIndex(2)];
        let old_pk = old_keys[0].public_key;

        let setup = setup_reshare(&old_keys, &old_session, old_signers.clone());
        let all_bundles: Vec<ReshareRound1Bundle> =
            setup.bundles.iter().map(|(b, _)| b.clone()).collect();

        let my_p2p: Vec<ReshareRound2P2pMsg> = setup.bundles
            .iter()
            .flat_map(|(_, p2p)| p2p.iter())
            .filter(|m| m.to == PartyIndex(1))
            .cloned()
            .collect();

        // Yanlış old_signers — Lagrange katsayıları yanlış hesaplanacak
        let wrong_signers = vec![PartyIndex(1), PartyIndex(3)]; // P3 yokken

        let result = run_reshare_round2(
            PartyIndex(1), NEW_T, NEW_N,
            &setup.new_ecdh_secrets[0],
            all_bundles, my_p2p,
            &wrong_signers, &old_pk, // wrong signers
            &setup.reshare_session,
            &setup.old_keygen_session_id,
        );

        // Eksik bundle veya public key uyuşmazlığı hatası bekleniyor
        assert!(result.is_err(), "yanlış old_signers reddedilmeli");
    }
}
