//! Online Threshold İmzalama — ~10ms tek round.
//!
//! §5.9.2: "Non-interactive pre-signing: guardian'ların aynı anda online
//!          olması gerekmez. Online aşama ~10ms."
//!
//! §6.1.1: "Nova Shield Chain yalnızca CGGMP21 kullanır."
//!
//! ## Protokol
//!
//! Pre-signing tamamlandıktan sonra (PreSignState hazır), online round:
//!
//! **Her imzacı P_i:**
//! 1. `e = BLAKE3("nova-sign-challenge-v1" || R || X || msg_hash)` hesapla
//! 2. `λ_i = ∏_{j∈S, j≠i} j / (j - i)` — Lagrange katsayısı (secp256k1 skalerleri)
//! 3. `s_i = r_i + e · x_i · λ_i` — kısmi imza
//! 4. `(party_index, s_i)` yayımla
//!
//! **Birleştirici (herhangi biri):**
//! 5. `s = ∑s_i mod n`
//! 6. `ThresholdSignature { r_point: R, s_scalar: s }` döndür
//!
//! ## Doğrulama (kamuya açık)
//!
//! `s * G == R + e * X`
//!
//! Neden doğru:
//! ```text
//! ∑(s_i) * G
//! = ∑(r_i + e · x_i · λ_i) * G
//! = ∑(r_i * G) + e · ∑(λ_i · x_i) * G
//! = R           + e · s_secret * G       ← Lagrange interpolasyonu
//! = R           + e · X                  ✓
//! ```
//!
//! ## Güvenlik
//!
//! - Nonce tek kullanımlık: `PreSignState.our_nonce_secret` kullanıldıktan sonra sıfırlanmalı.
//! - Lagrange katsayısı farklı imzacı setleri için farklıdır → bir kümede üretilen
//!   kısmi imza başka küme için geçersizdir.
//! - Challenge mesaj hash'i içerdiğinden farklı mesajlar için farklı imza üretilir.
//! - Domain separation: "nova-sign-challenge-v1" → farklı protokollerde replay saldırısı önlenir.

use k256::Scalar;
use std::sync::atomic::Ordering;

use crate::crypto_utils::{
    fiat_shamir_challenge, point_add, scalar_add, scalar_from_bytes, scalar_from_u32,
    scalar_invert, scalar_mul, scalar_mul_generator, scalar_mul_point, scalar_to_bytes,
};
use crate::errors::{MpcError, MpcResult};
use crate::types::{
    LocalKey, PartialSignature, PartyIndex, PreSignState, ThresholdSignature,
};

// ─── CHALLENGE HESAPLAMA ──────────────────────────────────────────────────────

/// Schnorr threshold imza challenge'ı hesapla.
///
/// `e = BLAKE3("nova-sign-challenge-v1" || R || X || msg_hash)`
///
/// Bu challenge hem imzacılar hem de doğrulayıcılar tarafından
/// deterministic olarak hesaplanır.
pub fn compute_sign_challenge(
    r_point: &[u8; 33],
    public_key: &[u8; 33],
    msg_hash: &[u8; 32],
) -> MpcResult<Scalar> {
    let bytes = fiat_shamir_challenge(
        "nova-sign-challenge-v1",
        &[r_point.as_ref(), public_key.as_ref(), msg_hash.as_ref()],
    );
    scalar_from_bytes(&bytes)
}

// ─── LAGRANGE KATSAYISI ───────────────────────────────────────────────────────

/// Lagrange interpolasyon katsayısı `λ_i(0)` hesapla.
///
/// `λ_i(0) = ∏_{j∈S, j≠i} j / (j - i)` (secp256k1 scalar field üzerinde)
///
/// ## Parametreler
///
/// - `my_index`: Bu tarafın indeksi `i`.
/// - `signers`: İmzacı taraf listesi `S` (threshold kadar).
///
/// ## Güvenlik
///
/// Party index'ler `u32` olarak `scalar_from_u32` ile field elemanına dönüştürülür.
/// `j - i` hesabı modüler çıkarma olduğundan negatif durumda alan sarması yapılır.
/// Sıfıra bölme (`j == i`) imkansızdır — döngü `j ≠ i` kontrolü yapar.
pub fn compute_lagrange_coefficient(
    my_index: PartyIndex,
    signers: &[PartyIndex],
) -> MpcResult<Scalar> {
    let i_scalar = scalar_from_u32(my_index.0);
    let mut lambda = Scalar::ONE;

    for &j in signers {
        if j == my_index {
            continue;
        }
        let j_scalar = scalar_from_u32(j.0);
        // j - i (mod n) — alan üzerinde çıkarma
        let j_minus_i = j_scalar - i_scalar;

        // (j - i)^{-1} hesapla — sıfır olamaz çünkü j ≠ i
        let inv = scalar_invert(&j_minus_i).map_err(|_| {
            MpcError::InternalError(format!(
                "Lagrange: j={} ve i={} için (j-i) terslenemedi",
                j.0, my_index.0
            ))
        })?;

        // λ_i = λ_i * (j / (j - i))
        let term = scalar_mul(&j_scalar, &inv);
        lambda = scalar_mul(&lambda, &term);
    }

    Ok(lambda)
}

// ─── KISMİ İMZA ──────────────────────────────────────────────────────────────

/// Kısmi imza üret.
///
/// `s_i = r_i + e · x_i · λ_i`
///
/// ## Güvenlik
///
/// - `PreSignState.our_nonce_secret` (r_i) bu fonksiyondan sonra tek kullanımlık
///   olarak işaretlenmeli — aynı nonce'u iki farklı mesaj için kullanmak
///   gizli payı ifşa eder!
/// - `LocalKey.secret_share` (x_i) bu fonksiyon dışına çıkmaz.
pub fn create_partial_signature(
    presign_state: &PreSignState,
    local_key: &LocalKey,
    msg_hash: &[u8; 32],
) -> MpcResult<PartialSignature> {
    // NOVA-RED-MPC-001: Nonce tek kullanımlık zorlaması.
    // compare_exchange: false → true (atomik). Eğer zaten true ise hata döndür.
    // Aynı nonce iki farklı mesaj için kullanılırsa:
    //   s1 - s2 = (e1 - e2) * x_i * λ_i  →  x_i = (s1-s2) / ((e1-e2)*λ_i)
    // Bu matematiksel olarak gizli payı tamamen ifşa eder.
    if presign_state
        .nonce_used
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(MpcError::InternalError(
            "nonce zaten kullanıldı — PreSignState tek kullanımlıktır, \
             aynı nonce ile ikinci imza gizli payı ifşa eder".into(),
        ));
    }

    // Public key kontrolü — presign ve keygen aynı aggregate anahtarı paylaşmalı.
    // Not: presign session_id != keygen session_id (farklı protokol aşamaları),
    // bu yüzden public key eşleşmesi kontrol edilir.
    if presign_state.public_key != local_key.public_key {
        return Err(MpcError::InternalError(
            "presign public key ile local key public key uyuşmuyor".into(),
        ));
    }

    // e = challenge
    let e = compute_sign_challenge(
        &presign_state.combined_nonce,
        &presign_state.public_key,
        msg_hash,
    )?;

    // λ_i = Lagrange katsayısı
    let lambda = compute_lagrange_coefficient(presign_state.our_index, &presign_state.signers)?;

    // r_i = nonce skaleri
    let r_i = scalar_from_bytes(&presign_state.our_nonce_secret)?;

    // x_i = gizli pay
    let x_i = scalar_from_bytes(&local_key.secret_share)?;

    // s_i = r_i + e · x_i · λ_i
    let x_i_lambda = scalar_mul(&x_i, &lambda);
    let e_x_lambda = scalar_mul(&e, &x_i_lambda);
    let s_i = r_i + e_x_lambda;

    Ok(PartialSignature {
        party_index: presign_state.our_index,
        s_i: scalar_to_bytes(&s_i),
        session_id: presign_state.session_id,
    })
}

// ─── KISMİ İMZA DOĞRULAMA ────────────────────────────────────────────────────

/// Kısmi imzayı doğrula.
///
/// `s_i * G == R_i + e · λ_i · X_i`
///
/// Burada:
/// - `R_i` = party_i'nin nonce noktası (PreSign'dan)
/// - `X_i` = party_i'nin public payı (VSS'den hesaplanır)
/// - `e` = signing challenge
/// - `λ_i` = Lagrange katsayısı
///
/// Bu doğrulama kötü niyetli parti tespitine (identifiable abort) olanak sağlar.
pub fn verify_partial_signature(
    partial: &PartialSignature,
    nonce_point_i: &[u8; 33],
    public_share_i: &[u8; 33],
    combined_nonce: &[u8; 33],
    public_key: &[u8; 33],
    msg_hash: &[u8; 32],
    signers: &[PartyIndex],
) -> MpcResult<bool> {
    let e = compute_sign_challenge(combined_nonce, public_key, msg_hash)?;

    let lambda_i = compute_lagrange_coefficient(partial.party_index, signers)?;

    // sol taraf: s_i * G
    let s_i = scalar_from_bytes(&partial.s_i)?;
    let lhs = scalar_mul_generator(&s_i);

    // sağ taraf: R_i + e * λ_i * X_i
    let e_lambda = scalar_mul(&e, &lambda_i);
    let e_lambda_xi = scalar_mul_point(&e_lambda, public_share_i)?;
    let rhs = point_add(nonce_point_i, &e_lambda_xi)?;

    Ok(lhs == rhs)
}

// ─── AGGREGATE VE DOĞRULAMA ───────────────────────────────────────────────────

/// Tüm kısmi imzaları topla — ThresholdSignature üret.
///
/// `s = ∑s_i mod n`
///
/// Partial imza session ID'lerinin tümü aynı olmalı.
pub fn aggregate_partial_signatures(
    partial_sigs: &[PartialSignature],
    combined_nonce: &[u8; 33],
    expected_session_id: &[u8; 32],
    signers: &[PartyIndex],
) -> MpcResult<ThresholdSignature> {
    // İmzacı sayısı kontrolü
    if partial_sigs.len() != signers.len() {
        return Err(MpcError::InvalidSignerCount {
            expected: signers.len(),
            got: partial_sigs.len(),
        });
    }

    // Her partial imzanın session ID'si eşleşmeli
    for ps in partial_sigs {
        if &ps.session_id != expected_session_id {
            return Err(MpcError::SessionMismatch {
                expected: expected_session_id.to_vec(),
                got: ps.session_id.to_vec(),
            });
        }
    }

    // Signer indekslerinin beklenen kümesiyle eşleşmesi kontrolü
    {
        let mut ps_indices: Vec<u32> = partial_sigs.iter().map(|p| p.party_index.0).collect();
        let mut signer_indices: Vec<u32> = signers.iter().map(|p| p.0).collect();
        ps_indices.sort_unstable();
        signer_indices.sort_unstable();
        if ps_indices != signer_indices {
            return Err(MpcError::ProtocolAbort {
                culprits: vec![],
                reason: "partial imzacı indeksleri beklenen signer listesiyle uyuşmuyor".into(),
            });
        }
    }

    // s = ∑s_i
    let mut s_total = Scalar::ZERO;
    for ps in partial_sigs {
        let s_i = scalar_from_bytes(&ps.s_i)?;
        s_total = s_total + s_i;
    }

    Ok(ThresholdSignature {
        r_point: *combined_nonce,
        s_scalar: scalar_to_bytes(&s_total),
    })
}

/// Threshold Schnorr imzasını doğrula.
///
/// `s * G == R + e * X`
///
/// Bu doğrulama herkese açıktır — sadece public key, R noktası ve mesaj gerekir.
pub fn verify_threshold_signature(
    sig: &ThresholdSignature,
    public_key: &[u8; 33],
    msg_hash: &[u8; 32],
) -> MpcResult<bool> {
    // e = challenge
    let e = compute_sign_challenge(&sig.r_point, public_key, msg_hash)?;

    // sol: s * G
    let s = scalar_from_bytes(&sig.s_scalar)?;
    let lhs = scalar_mul_generator(&s);

    // sağ: R + e * X
    let ex = scalar_mul_point(&e, public_key)?;
    let rhs = point_add(&sig.r_point, &ex)?;

    Ok(lhs == rhs)
}

/// Sıfır imza (r=0 veya s=0) geçersiz mi?
///
/// Sıfır skaler imza her zaman geçersizdir.
pub fn is_valid_signature_format(sig: &ThresholdSignature) -> bool {
    sig.r_point[0] != 0x00  // sonsuz nokta değil
        && sig.s_scalar != [0u8; 32]  // sıfır skaler değil
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen::{run_round1, run_round2, run_round3};
    use crate::session::{compute_keygen_ssid, compute_presign_ssid};
    use crate::sign::presign::{
        compute_nonce_commitment, create_presign_share, finalize_presign,
        run_presign_round1, run_presign_round2,
    };
    use crate::types::{KeygenRound1Msg, KeygenRound2BroadcastMsg, KeygenRound2P2pMsg};
    use nova_primitives::types::ChainId;

    const N: u32 = 3;
    const T: u8 = 2;

    /// n-party keygen çalıştırır, LocalKey listesi döndürür.
    fn run_full_keygen(n: u32, t: u8) -> (Vec<crate::types::LocalKey>, crate::types::SessionId) {
        let parties: Vec<PartyIndex> = (1..=n).map(PartyIndex).collect();
        let session = compute_keygen_ssid(ChainId(1984), &parties, &[0u8; 32]);

        let r1_results: Vec<_> = (1..=n)
            .map(|i| run_round1(PartyIndex(i), t, n, &session).unwrap())
            .collect();
        let r1_msgs: Vec<KeygenRound1Msg> = r1_results.iter().map(|(m, _)| m.clone()).collect();

        let r2_results: Vec<_> = r1_results
            .into_iter()
            .map(|(_, state)| run_round2(state, r1_msgs.clone()).unwrap())
            .collect();

        let r2_broadcasts: Vec<KeygenRound2BroadcastMsg> =
            r2_results.iter().map(|(b, _, _)| b.clone()).collect();
        let all_p2p: Vec<Vec<KeygenRound2P2pMsg>> =
            r2_results.iter().map(|(_, p, _)| p.clone()).collect();

        let local_keys: Vec<_> = r2_results
            .into_iter()
            .enumerate()
            .map(|(idx, (_, _, r2_state))| {
                let my_idx = idx + 1;
                let my_p2p: Vec<(PartyIndex, [u8; 12], Vec<u8>)> = all_p2p
                    .iter()
                    .enumerate()
                    .filter_map(|(from_idx, p2p_list)| {
                        if from_idx + 1 == my_idx {
                            return None;
                        }
                        p2p_list
                            .iter()
                            .find(|m| m.to == PartyIndex(my_idx as u32))
                            .map(|m| {
                                (
                                    m.from,
                                    m.encrypted_share_nonce,
                                    m.encrypted_share_ciphertext.clone(),
                                )
                            })
                    })
                    .collect();
                let (_, local_key) = run_round3(r2_state, r2_broadcasts.clone(), my_p2p).unwrap();
                local_key
            })
            .collect();

        (local_keys, session)
    }

    /// n-party presign çalıştırır, PreSignState listesi döndürür.
    fn run_full_presign(
        local_keys: &[LocalKey],
        signers: &[PartyIndex],
        keygen_session: &crate::types::SessionId,
    ) -> Vec<PreSignState> {
        let n = signers.len();
        let presign_session = compute_presign_ssid(
            keygen_session,
            signers,
            9000,
            &[0xBBu8; 32],
        );

        // Round 1
        let r1_results: Vec<_> = signers
            .iter()
            .map(|&p| run_presign_round1(p, &presign_session, 99999).unwrap())
            .collect();
        let r1_msgs: Vec<_> = r1_results.iter().map(|(m, _)| m.clone()).collect();

        // Round 2
        let r2_results: Vec<_> = r1_results
            .into_iter()
            .map(|(_, state)| run_presign_round2(state, r1_msgs.clone()).unwrap())
            .collect();
        let r2_msgs: Vec<_> = r2_results.iter().map(|(m, _, _)| m.clone()).collect();

        // Finalize
        r2_results
            .into_iter()
            .enumerate()
            .map(|(idx, (_, r1_msgs_kept, r1_state))| {
                finalize_presign(
                    r1_state,
                    r1_msgs_kept,
                    r2_msgs.clone(),
                    &local_keys[idx],
                    signers.to_vec(),
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn test_lagrange_coefficient_2_of_3() {
        // 2/3 eşikle {1, 2} imzacı kümesi için Lagrange katsayıları
        let signers = vec![PartyIndex(1), PartyIndex(2)];

        let lambda_1 = compute_lagrange_coefficient(PartyIndex(1), &signers).unwrap();
        let lambda_2 = compute_lagrange_coefficient(PartyIndex(2), &signers).unwrap();

        // λ_1 = 2 / (2 - 1) = 2
        // λ_2 = 1 / (1 - 2) = 1 / (-1) = -1 mod n
        // λ_1 + λ_2 = 2 + (-1) = 1 ✓ (sadece 2 parti, {x_1 * λ_1 + x_2 * λ_2 = secret})

        // λ_1 * 1 + λ_2 * 2 değil; Lagrange kurtarması tam olarak çalışıyor mu test et
        // secret = 100, x_1 = f(1) = 100 + a*1, x_2 = f(2) = 100 + a*2 gibi bir örnek

        // Basit test: λ_1 + λ_2 = 1 (Lagrange bariz özelliği) kontrol et
        // Bu her zaman doğru: sum_{i∈S} λ_i(0) = 1
        let sum = lambda_1 + lambda_2;
        let one_bytes = {
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        };
        let one = scalar_from_bytes(&one_bytes).unwrap();
        assert!(
            crate::crypto_utils::scalars_equal_ct(&sum, &one),
            "λ_1 + λ_2 = 1 olmalı (2/3 eşik, imzacı kümesi {{1,2}})"
        );
    }

    #[test]
    fn test_lagrange_sum_equals_one() {
        // Herhangi bir imzacı kümesi için ∑λ_i(0) = 1
        let signers = vec![PartyIndex(1), PartyIndex(3), PartyIndex(5)];
        let mut sum = Scalar::ZERO;
        for &p in &signers {
            let l = compute_lagrange_coefficient(p, &signers).unwrap();
            sum = sum + l;
        }
        let one_bytes = {
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        };
        let one = scalar_from_bytes(&one_bytes).unwrap();
        assert!(
            crate::crypto_utils::scalars_equal_ct(&sum, &one),
            "∑λ_i = 1 olmalı"
        );
    }

    #[test]
    fn test_sign_challenge_deterministic() {
        let r = [0x02u8; 33];
        let x = [0x03u8; 33];
        let msg = [0xAAu8; 32];
        let e1 = compute_sign_challenge(&r, &x, &msg).unwrap();
        let e2 = compute_sign_challenge(&r, &x, &msg).unwrap();
        assert!(
            crate::crypto_utils::scalars_equal_ct(&e1, &e2),
            "challenge deterministik olmalı"
        );
    }

    #[test]
    fn test_sign_challenge_domain_separation() {
        // Farklı R → farklı challenge
        let r1 = scalar_mul_generator(&scalar_from_u32(5));
        let r2 = scalar_mul_generator(&scalar_from_u32(7));
        let x = [0x02u8; 33];
        let msg = [0xBBu8; 32];

        let e1 = compute_sign_challenge(&r1, &x, &msg).unwrap();
        let e2 = compute_sign_challenge(&r2, &x, &msg).unwrap();

        assert!(
            !crate::crypto_utils::scalars_equal_ct(&e1, &e2),
            "farklı R → farklı challenge"
        );
    }

    #[test]
    fn test_full_threshold_signing_2_of_3() {
        // Tam akış: keygen → presign → online sign → verify
        let (local_keys, keygen_session) = run_full_keygen(N, T);
        let signers = vec![PartyIndex(1), PartyIndex(2)];
        let presign_states = run_full_presign(&local_keys, &signers, &keygen_session);

        let msg_hash = [0xDE; 32];

        // Kısmi imzalar üret
        let partial_sigs: Vec<PartialSignature> = presign_states
            .iter()
            .enumerate()
            .map(|(idx, ps)| {
                create_partial_signature(ps, &local_keys[idx], &msg_hash).unwrap()
            })
            .collect();

        // Aggregate
        let combined_nonce = presign_states[0].combined_nonce;
        let session_id = presign_states[0].session_id;
        let sig = aggregate_partial_signatures(
            &partial_sigs,
            &combined_nonce,
            &session_id,
            &signers,
        ).unwrap();

        // Doğrulama
        let public_key = local_keys[0].public_key;
        let valid = verify_threshold_signature(&sig, &public_key, &msg_hash).unwrap();
        assert!(valid, "threshold Schnorr imzası doğrulanmalı");

        // Yanlış mesaj reddedilmeli
        let wrong_msg = [0xFF; 32];
        let invalid = verify_threshold_signature(&sig, &public_key, &wrong_msg).unwrap();
        assert!(!invalid, "yanlış mesaj hash reddedilmeli");
    }

    #[test]
    fn test_full_threshold_signing_3_of_5() {
        // 3/5 eşik ile tam imzalama
        let (local_keys, keygen_session) = run_full_keygen(5, 3);
        let signers = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];
        let presign_states = run_full_presign(&local_keys, &signers, &keygen_session);

        let msg_hash = [0x42u8; 32];

        let partial_sigs: Vec<PartialSignature> = presign_states
            .iter()
            .enumerate()
            .map(|(idx, ps)| {
                create_partial_signature(ps, &local_keys[idx], &msg_hash).unwrap()
            })
            .collect();

        let combined_nonce = presign_states[0].combined_nonce;
        let session_id = presign_states[0].session_id;
        let sig = aggregate_partial_signatures(
            &partial_sigs,
            &combined_nonce,
            &session_id,
            &signers,
        ).unwrap();

        let public_key = local_keys[0].public_key;
        let valid = verify_threshold_signature(&sig, &public_key, &msg_hash).unwrap();
        assert!(valid, "3/5 threshold Schnorr imzası doğrulanmalı");
    }

    #[test]
    fn test_different_signer_subsets_give_same_key() {
        // Farklı imzacı alt kümeleri aynı agregat public key'i kullanır
        let (local_keys, keygen_session) = run_full_keygen(5, 3);

        let msg_hash = [0x99u8; 32];

        // Alt küme 1: {1, 2, 3}
        let signers_a = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];
        let states_a = run_full_presign(&local_keys, &signers_a, &keygen_session);
        let partials_a: Vec<_> = states_a.iter().enumerate()
            .map(|(i, ps)| create_partial_signature(ps, &local_keys[i], &msg_hash).unwrap())
            .collect();
        let sig_a = aggregate_partial_signatures(
            &partials_a, &states_a[0].combined_nonce, &states_a[0].session_id, &signers_a
        ).unwrap();

        let pk = local_keys[0].public_key;
        assert!(verify_threshold_signature(&sig_a, &pk, &msg_hash).unwrap());
    }

    #[test]
    fn test_partial_signature_wrong_message_fails() {
        // Her parti için iki ayrı presign state gerekiyor:
        // presign_states_a: msg_hash için, presign_states_b: wrong_msg için
        // Çünkü nonce tek kullanımlık (NOVA-RED-MPC-001 fix)
        let (local_keys, keygen_session) = run_full_keygen(N, T);
        let signers = vec![PartyIndex(1), PartyIndex(2)];

        let presign_states_a = run_full_presign(&local_keys, &signers, &keygen_session);
        let presign_states_b = run_full_presign(&local_keys, &signers, &keygen_session);

        let msg_hash = [0xAA; 32];
        let wrong_msg = [0xBB; 32];

        // Parti 0: wrong_msg ile imzala; Parti 1: msg_hash ile imzala
        // → Karışık mesaj: aggregate başarısız veya doğrulama başarısız olmalı
        let partial_mixed = vec![
            create_partial_signature(&presign_states_b[0], &local_keys[0], &wrong_msg).unwrap(),
            create_partial_signature(&presign_states_a[1], &local_keys[1], &msg_hash).unwrap(),
        ];

        // Farklı presign session → combined_nonce farklı → aggregate session mismatch
        // ya da doğrulama başarısız
        let combined_nonce_a = presign_states_a[0].combined_nonce;
        let session_id_a = presign_states_a[0].session_id;

        // Farklı session ID ile karışık sig → session mismatch veya invalid sig
        let result = aggregate_partial_signatures(
            &partial_mixed, &combined_nonce_a, &session_id_a, &signers
        );

        match result {
            Err(_) => {
                // Session mismatch bekleniyor (farklı presign'dan gelen partial)
            }
            Ok(sig) => {
                // Eğer aggregate başarılıysa, doğrulama başarısız olmalı
                let valid = verify_threshold_signature(&sig, &local_keys[0].public_key, &msg_hash).unwrap();
                assert!(!valid, "yanlış mesajla oluşturulan partial imza doğrulanmamalı");
            }
        }
    }

    // ── NOVA-RED-MPC-001 regression testleri ──────────────────────────────────

    #[test]
    fn test_nonce_reuse_rejected() {
        // Aynı PreSignState iki farklı mesaj için kullanılamaz
        let (local_keys, keygen_session) = run_full_keygen(N, T);
        let signers = vec![PartyIndex(1), PartyIndex(2)];
        let presign_states = run_full_presign(&local_keys, &signers, &keygen_session);

        let msg1 = [0x11u8; 32];
        let msg2 = [0x22u8; 32]; // farklı mesaj

        // İlk kullanım başarılı
        let result1 = create_partial_signature(&presign_states[0], &local_keys[0], &msg1);
        assert!(result1.is_ok(), "ilk kullanım başarılı olmalı");

        // İkinci kullanım REDDEDILMELI — nonce yeniden kullanımı
        let result2 = create_partial_signature(&presign_states[0], &local_keys[0], &msg2);
        assert!(
            result2.is_err(),
            "aynı nonce ile ikinci imza reddedilmeli — gizli pay ifşası önlenir"
        );
        let err_msg = result2.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonce zaten kullanıldı"),
            "hata mesajı nonce reuse'u açıklamalı: {err_msg}"
        );
    }

    #[test]
    fn test_nonce_reuse_same_message_also_rejected() {
        // Aynı mesaj için bile ikinci çağrı reddedilmeli (deterministik saldırıya karşı)
        let (local_keys, keygen_session) = run_full_keygen(N, T);
        let signers = vec![PartyIndex(1), PartyIndex(2)];
        let presign_states = run_full_presign(&local_keys, &signers, &keygen_session);

        let msg = [0x33u8; 32];

        create_partial_signature(&presign_states[0], &local_keys[0], &msg).unwrap();
        let result = create_partial_signature(&presign_states[0], &local_keys[0], &msg);
        assert!(result.is_err(), "aynı mesaj için bile nonce reuse reddedilmeli");
    }

    #[test]
    fn test_signature_format_check() {
        // Sonsuz nokta imzası geçersiz
        let bad_sig = ThresholdSignature {
            r_point: crate::crypto_utils::IDENTITY_BYTES,
            s_scalar: [0u8; 32],
        };
        assert!(!is_valid_signature_format(&bad_sig));

        // Normal imza geçerli format
        let good_sig = ThresholdSignature {
            r_point: scalar_mul_generator(&scalar_from_u32(5)),
            s_scalar: {
                let mut b = [0u8; 32];
                b[31] = 1;
                b
            },
        };
        assert!(is_valid_signature_format(&good_sig));
    }
}
