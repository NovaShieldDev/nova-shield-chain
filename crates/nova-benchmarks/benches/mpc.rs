/// MPC threshold imzalama benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - compute_lagrange_coefficient()  — Lagrange katsayısı hesaplama (t=5)
/// - create_partial_signature()      — kısmi imza üretimi (2/3)
/// - aggregate_partial_signatures()  — imza birleştirme (2/3)
/// - verify_threshold_signature()    — threshold Schnorr doğrulama
/// - keygen_3_round_full()           — tam 3-round keygen (t=3, n=5)
/// - presign_2_round_full()          — tam 2-round presign (t=3, n=5)
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use nova_mpc::{
    keygen::{run_round1, run_round2, run_round3},
    sign::{
        aggregate_partial_signatures, compute_lagrange_coefficient, compute_sign_challenge,
        create_partial_signature, verify_threshold_signature,
        run_presign_round1, run_presign_round2, finalize_presign,
    },
    session::{compute_keygen_ssid, compute_presign_ssid},
    types::{
        KeygenRound1Msg, KeygenRound2BroadcastMsg, KeygenRound2P2pMsg,
        LocalKey, PartialSignature, PartyIndex, PreSignState, SessionId,
    },
};
use nova_primitives::types::ChainId;

// ─── Yardımcı: tam keygen çalıştır ───────────────────────────────────────────

fn run_full_keygen(n: u32, t: u8) -> (Vec<LocalKey>, SessionId) {
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
                        .map(|m| (m.from, m.encrypted_share_nonce, m.encrypted_share_ciphertext.clone()))
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
    keygen_session: &SessionId,
) -> Vec<PreSignState> {
    let presign_session = compute_presign_ssid(keygen_session, signers, 9000, &[0xBBu8; 32]);

    let r1_results: Vec<_> = signers
        .iter()
        .map(|&p| run_presign_round1(p, &presign_session, 99999).unwrap())
        .collect();
    let r1_msgs: Vec<_> = r1_results.iter().map(|(m, _)| m.clone()).collect();

    let r2_results: Vec<_> = r1_results
        .into_iter()
        .map(|(_, state)| run_presign_round2(state, r1_msgs.clone()).unwrap())
        .collect();
    let r2_msgs: Vec<_> = r2_results.iter().map(|(m, _, _)| m.clone()).collect();

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

// ─── Lagrange katsayısı ──────────────────────────────────────────────────────

fn bench_lagrange_coefficient(c: &mut Criterion) {
    let signer_sets: &[(u8, Vec<PartyIndex>)] = &[
        (2, vec![PartyIndex(1), PartyIndex(2)]),
        (3, vec![PartyIndex(1), PartyIndex(3), PartyIndex(5)]),
        (5, vec![PartyIndex(1), PartyIndex(2), PartyIndex(3), PartyIndex(4), PartyIndex(5)]),
    ];

    let mut group = c.benchmark_group("lagrange_coefficient");
    for (t, signers) in signer_sets {
        group.bench_with_input(
            BenchmarkId::new("threshold", t),
            signers,
            |b, s| {
                b.iter(|| compute_lagrange_coefficient(PartyIndex(1), s).unwrap())
            },
        );
    }
    group.finish();
}

// ─── Signing challenge hesaplama ─────────────────────────────────────────────

fn bench_sign_challenge(c: &mut Criterion) {
    let r_point = [0x02u8; 33];
    let public_key = [0x03u8; 33];
    let msg_hash = [0xDEu8; 32];

    c.bench_function("compute_sign_challenge", |b| {
        b.iter(|| compute_sign_challenge(&r_point, &public_key, &msg_hash).unwrap())
    });
}

// ─── Kısmi imza üretimi (2/3) ────────────────────────────────────────────────

fn bench_create_partial_signature(c: &mut Criterion) {
    // Gerçek keygen + presign
    let (local_keys, session) = run_full_keygen(3, 2);
    let signers = vec![PartyIndex(1), PartyIndex(2)];
    let presign_states = run_full_presign(&local_keys, &signers, &session);

    let msg_hash = [0xABu8; 32];
    let state_0 = &presign_states[0];
    let key_0 = &local_keys[0];

    c.bench_function("create_partial_signature_2_of_3", |b| {
        b.iter(|| {
            // PreSignState'i yeniden oluşturmak gerekiyor (create_partial_signature borrows)
            // Bench bağlamında clone yoktur, tekrar presign çalıştırıyoruz
            let states = run_full_presign(&local_keys, &signers, &session);
            create_partial_signature(&states[0], key_0, &msg_hash).unwrap()
        })
    });
}

// ─── İmza birleştirme (2/3) ──────────────────────────────────────────────────

fn bench_aggregate_partial_signatures(c: &mut Criterion) {
    let (local_keys, session) = run_full_keygen(3, 2);
    let signers = vec![PartyIndex(1), PartyIndex(2)];
    let msg_hash = [0xABu8; 32];

    // Hazır partial imzalar üret
    let states = run_full_presign(&local_keys, &signers, &session);
    let combined_nonce = states[0].combined_nonce;
    let session_id_bytes = states[0].session_id;

    let partial_sigs: Vec<PartialSignature> = states
        .iter()
        .enumerate()
        .map(|(i, s)| create_partial_signature(s, &local_keys[i], &msg_hash).unwrap())
        .collect();

    c.bench_function("aggregate_partial_signatures_2_of_3", |b| {
        b.iter(|| {
            aggregate_partial_signatures(
                &partial_sigs,
                &combined_nonce,
                &session_id_bytes,
                &signers,
            )
            .unwrap()
        })
    });
}

// ─── Threshold imza doğrulama ────────────────────────────────────────────────

fn bench_verify_threshold_signature(c: &mut Criterion) {
    let (local_keys, session) = run_full_keygen(3, 2);
    let signers = vec![PartyIndex(1), PartyIndex(2)];
    let msg_hash = [0xABu8; 32];

    let states = run_full_presign(&local_keys, &signers, &session);
    let combined_nonce = states[0].combined_nonce;
    let public_key = states[0].public_key;
    let session_id_bytes = states[0].session_id;

    let partial_sigs: Vec<PartialSignature> = states
        .iter()
        .enumerate()
        .map(|(i, s)| create_partial_signature(s, &local_keys[i], &msg_hash).unwrap())
        .collect();

    let sig = aggregate_partial_signatures(
        &partial_sigs,
        &combined_nonce,
        &session_id_bytes,
        &signers,
    )
    .unwrap();

    c.bench_function("verify_threshold_signature", |b| {
        b.iter(|| verify_threshold_signature(&sig, &public_key, &msg_hash).unwrap())
    });
}

// ─── Tam keygen (3-round, 3/5) ───────────────────────────────────────────────

fn bench_full_keygen(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_keygen");

    group.bench_function("t3_n5", |b| {
        b.iter(|| run_full_keygen(5, 3))
    });

    group.bench_function("t2_n3", |b| {
        b.iter(|| run_full_keygen(3, 2))
    });

    group.finish();
}

// ─── Tam presign (2-round, imzacı kümesi) ────────────────────────────────────

fn bench_full_presign(c: &mut Criterion) {
    let (keys_3, session_3) = run_full_keygen(3, 2);
    let (keys_5, session_5) = run_full_keygen(5, 3);

    let signers_2 = vec![PartyIndex(1), PartyIndex(2)];
    let signers_3 = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];

    let mut group = c.benchmark_group("full_presign");

    group.bench_function("t2_n3", |b| {
        b.iter(|| run_full_presign(&keys_3, &signers_2, &session_3))
    });

    group.bench_function("t3_n5", |b| {
        b.iter(|| run_full_presign(&keys_5, &signers_3, &session_5))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_lagrange_coefficient,
    bench_sign_challenge,
    bench_create_partial_signature,
    bench_aggregate_partial_signatures,
    bench_verify_threshold_signature,
    bench_full_keygen,
    bench_full_presign,
);
criterion_main!(benches);
