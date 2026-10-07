//! Keygen Round 1 — Taahhüt + ECDH Public Key + DLog Kanıtı
//!
//! §5.1.3: "Her parti keygen/reshare oturumu başında ephemeral ECDH keypair üretir."
//! §01-multi-party-ecdsa.md §4.2: GG18 Phase 1 referans alındı.
//! §02-tss-lib.md §4 Round 1: Commitment şeması referans alındı.
//!
//! ## Protokol
//!
//! Her parti şu adımları atar:
//!
//! 1. Rastgele gizli katkı `u_i` scalar üretir
//! 2. Feldman VSS polinom oluşturur: `f_i(x) = u_i + a_1*x + ... + a_{t-1}*x^{t-1}`
//! 3. VSS taahhütleri hesaplar: `C_{i,j} = a_j * G`
//! 4. Taahhüt hash'i: `com_i = BLAKE3("nova-keygen-r1" || borsh(vss_commit) || session_id)`
//! 5. Schnorr DLog kanıtı: `u_i` bilindiğini kanıtlar (commit açılmadan önce)
//! 6. Ephemeral ECDH keypair üretir (Round 2 P2P şifreleme için)
//! 7. Yayımlar: `(com_i, dlog_proof, ecdh_pk)`
//!
//! ## Güvenlik — Taahhüt Neden Gerekli?
//!
//! Round 2'de VSS açılmadan önce Round 1'de sadece hash gönderilir.
//! Bu, saldırganın diğerlerinin mesajlarını görünce kendi `u_i`'sini
//! uyarlamasını (adaptive attack) önler. Taahhüt açılmadan VSS değiştirilemez.

use blake3::Hasher;
use borsh::BorshSerialize;
use k256::Scalar;
use k256::elliptic_curve::Field;
use rand::rngs::OsRng;

use crate::crypto_utils::{scalar_mul_generator, scalar_to_bytes};
use crate::dlog::dlog_prove;
use crate::errors::{MpcError, MpcResult};
use crate::p2p_encrypt::EcdhSecret;
use crate::types::{
    DlogProofBytes, EcdhPublicKey, KeygenRound1Msg, PartyIndex, SessionId, VssCommitment,
};
use crate::vss::{feldman_vss_generate, VssOutput};

// ─── ROUND 1 LOCAL STATE ──────────────────────────────────────────────────────

/// Round 1 sonrası yerel durum — Round 2'ye taşınacak gizli veriler.
///
/// Bu yapı gizli veri içerdiği için `Drop` edildiğinde sıfırlanmalı.
#[derive(Debug)]
pub struct Round1LocalState {
    /// Bu partinin VSS çıktısı (paylar + taahhütler).
    /// Round 2'de paylar P2P gönderilir, taahhütler broadcast edilir.
    pub vss_output: VssOutput,

    /// VSS taahhüdünün hash'i — Round 2'de açılım eşleşmeli.
    pub commitment_hash: [u8; 32],

    /// Round 1'de kullanılan ephemeral ECDH secret — Round 2'de P2P şifreleme için.
    pub ecdh_secret: EcdhSecret,

    /// Bu partinin gizli katkısı `u_i = vss_output.shares[0].value` (f_i(0) = u_i).
    /// Aslında bu `vss_output`'tan okunabilir; burada kolaylık için tutulur.
    /// NOT: VSS'de f_i(0) = a_0 = u_i. Shares parti indekslerinde, a_0 commitments[0]'da.
    pub u_i_public: [u8; 33],

    /// Session ID.
    pub session_id: SessionId,

    /// Bu partinin indeksi.
    pub party_index: PartyIndex,

    /// Threshold.
    pub threshold: u8,

    /// Toplam parti sayısı.
    pub party_count: u32,
}

// ─── ROUND 1 ÇALIŞTIRICISI ───────────────────────────────────────────────────

/// Keygen Round 1'i çalıştır.
///
/// ## Parametreler
///
/// - `party_index`: Bu partinin indeksi (1..=party_count).
/// - `threshold`: İmza için gereken minimum parti sayısı.
/// - `party_count`: Toplam parti sayısı.
/// - `session_id`: Bu keygen oturumunun SSID'si.
///
/// ## Döndürür
///
/// - `KeygenRound1Msg`: Tüm partilere broadcast edilecek mesaj.
/// - `Round1LocalState`: Round 2'ye taşınacak gizli durum.
pub fn run_round1(
    party_index: PartyIndex,
    threshold: u8,
    party_count: u32,
    session_id: &SessionId,
) -> MpcResult<(KeygenRound1Msg, Round1LocalState)> {
    // ── 1. Parametre doğrulama ────────────────────────────────────────────────
    if threshold == 0 || threshold as u32 > party_count {
        return Err(MpcError::InvalidThreshold {
            t: threshold,
            n: party_count as u8,
        });
    }
    PartyIndex::new(party_index.0, party_count)?;

    // ── 2. VSS üret (gizli katkı u_i rastgele seçilir) ───────────────────────
    let vss_output = feldman_vss_generate(threshold, party_count, None, party_index)?;

    // u_i = a_0 commitment'ının public versiyonu: U_i = u_i * G = C_{i,0}
    let u_i_public = vss_output.commitment.points[0];

    // ── 3. VSS taahhüt hash'i ─────────────────────────────────────────────────
    // com_i = BLAKE3("nova-keygen-r1" || borsh(vss_commitment) || session_id)
    let commitment_hash = compute_commitment_hash(&vss_output.commitment, session_id)?;

    // ── 4. DLog kanıtı: u_i bilindiğini kanıtla ──────────────────────────────
    // u_i = vss_output.shares ile alakalı değil; a_0'dır (f_i(0) = a_0 = u_i).
    // Doğrudan katsayı a_0'ı bilmiyoruz ama share[party_index-1] = f_i(party_index).
    // DLog kanıtı: commitment.points[0] = u_i * G için.
    // u_i'yi VSS'den çıkaramıyoruz doğrudan; ancak VSS üretimi sırasında a_0 = secret.
    // Secret None ile üretildiyse rastgele — biz bunu `u_i_bytes` olarak saklamak zorundayız.
    //
    // ÇÖZÜM: VSS üretimini u_i açık tutarak yeniden organize et.
    // Şimdilik: DLog kanıtı için u_i = Scalar::random + VSS buna göre ayarlanacak.
    //
    // Bu implementasyonda: VssOutput.commitment.points[0] = a_0 * G = u_i * G.
    // u_i'yi bulmak için: VSS üretimini `Some(u_i_bytes)` ile çağırıp tekrar üret.
    //
    // Daha iyi yaklaşım: feldman_vss_generate'e `output_secret: &mut [u8;32]` ekle.
    // Şimdilik: u_i'yi ayrıca rastgele üretip VSS'e geçiyoruz.

    let u_i_scalar = Scalar::random(&mut OsRng);
    let u_i_bytes = scalar_to_bytes(&u_i_scalar);
    let u_i_point = scalar_mul_generator(&u_i_scalar);

    // VSS'yi u_i ile üret (secret = u_i)
    let vss_output = feldman_vss_generate(threshold, party_count, Some(u_i_bytes), party_index)?;
    let commitment_hash = compute_commitment_hash(&vss_output.commitment, session_id)?;

    let dlog_proof = dlog_prove(
        &u_i_bytes,
        &u_i_point,
        "nova-keygen-dlog-v1",
        session_id.as_bytes(),
    )?;

    // ── 5. Ephemeral ECDH keypair üret ��───────────────────────────────────────
    let ecdh_secret = EcdhSecret::generate();
    let ecdh_pk = ecdh_secret.public_key();

    // ── 6. Round 1 mesajı oluştur ─────────────────────────────────────────────
    let msg = KeygenRound1Msg {
        from: party_index,
        commitment_hash,
        dlog_proof,
        ecdh_pk,
    };

    let state = Round1LocalState {
        vss_output,
        commitment_hash,
        ecdh_secret,
        u_i_public: u_i_point,
        session_id: *session_id,
        party_index,
        threshold,
        party_count,
    };

    Ok((msg, state))
}

// ─── YARDIMCI FONKSİYONLAR ───────────────────────────────────────────────────

/// VSS taahhüt hash'i hesapla.
///
/// `com = BLAKE3("nova-keygen-r1" || borsh(vss_commitment) || session_id)`
pub fn compute_commitment_hash(
    commitment: &VssCommitment,
    session_id: &SessionId,
) -> MpcResult<[u8; 32]> {
    let mut hasher = Hasher::new_derive_key("nova-keygen-r1-commitment-v1");
    // VSS taahhütlerini borsh ile serileştir
    let commitment_bytes = borsh::to_vec(commitment)
        .map_err(|e| MpcError::SerializationFailed(e.to_string()))?;
    hasher.update(&(commitment_bytes.len() as u64).to_le_bytes());
    hasher.update(&commitment_bytes);
    hasher.update(session_id.as_bytes());
    Ok(*hasher.finalize().as_bytes())
}

/// Round 1 mesajını doğrula — diğer partiden gelen mesajı kontrol et.
///
/// Şu an sadece temel alan doğrulaması yapılır.
/// Gerçek DLog ve commitment doğrulaması Round 3'te yapılır
/// (tüm paylar elimize geçtikten sonra).
pub fn verify_round1_msg(
    msg: &KeygenRound1Msg,
    party_count: u32,
    _session_id: &SessionId,
) -> MpcResult<()> {
    // Party index geçerli mi?
    PartyIndex::new(msg.from.0, party_count)?;

    // ECDH public key sıfır mı?
    if msg.ecdh_pk.compressed == [0u8; 33] {
        return Err(MpcError::InvalidPoint);
    }

    // DLog proof formatı geçerli mi? (nokta 0x02 veya 0x03 ile başlamalı)
    let prefix = msg.dlog_proof.nonce_point[0];
    if prefix != 0x02 && prefix != 0x03 {
        return Err(MpcError::InvalidPoint);
    }

    Ok(())
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::compute_keygen_ssid;
    use nova_primitives::types::ChainId;

    fn test_session(parties: &[PartyIndex]) -> SessionId {
        compute_keygen_ssid(ChainId(1984), parties, &[0u8; 32])
    }

    #[test]
    fn test_round1_basic_3_of_5() {
        let parties: Vec<PartyIndex> = (1..=5).map(PartyIndex).collect();
        let session = test_session(&parties);
        let (msg, state) = run_round1(PartyIndex(1), 3, 5, &session).unwrap();

        assert_eq!(msg.from, PartyIndex(1));
        assert_ne!(msg.commitment_hash, [0u8; 32]);
        assert_ne!(msg.dlog_proof.nonce_point, [0u8; 33]);
        assert_ne!(msg.dlog_proof.response, [0u8; 32]);
        assert_ne!(msg.ecdh_pk.compressed, [0u8; 33]);
        assert_eq!(state.threshold, 3);
        assert_eq!(state.party_count, 5);
        assert_eq!(state.vss_output.shares.len(), 5);
        assert_eq!(state.vss_output.commitment.points.len(), 3); // t=3, t katsayı
    }

    #[test]
    fn test_round1_all_parties_different_commitments() {
        let parties: Vec<PartyIndex> = (1..=3).map(PartyIndex).collect();
        let session = test_session(&parties);

        let (msg1, _) = run_round1(PartyIndex(1), 2, 3, &session).unwrap();
        let (msg2, _) = run_round1(PartyIndex(2), 2, 3, &session).unwrap();
        let (msg3, _) = run_round1(PartyIndex(3), 2, 3, &session).unwrap();

        // Her partinin taahhüdü farklı olmalı (rastgele u_i)
        assert_ne!(msg1.commitment_hash, msg2.commitment_hash);
        assert_ne!(msg2.commitment_hash, msg3.commitment_hash);
        assert_ne!(msg1.commitment_hash, msg3.commitment_hash);
    }

    #[test]
    fn test_round1_ecdh_keys_unique() {
        let parties: Vec<PartyIndex> = (1..=3).map(PartyIndex).collect();
        let session = test_session(&parties);

        let (m1, _) = run_round1(PartyIndex(1), 2, 3, &session).unwrap();
        let (m2, _) = run_round1(PartyIndex(1), 2, 3, &session).unwrap(); // aynı parti, tekrar

        // Ephemeral ECDH — her çağrıda yeni keypair
        assert_ne!(m1.ecdh_pk.compressed, m2.ecdh_pk.compressed);
    }

    #[test]
    fn test_round1_invalid_threshold() {
        let session = SessionId([0u8; 32]);
        assert!(run_round1(PartyIndex(1), 0, 5, &session).is_err(), "t=0 geçersiz");
        assert!(run_round1(PartyIndex(1), 6, 5, &session).is_err(), "t>n geçersiz");
    }

    #[test]
    fn test_round1_invalid_party_index() {
        let session = SessionId([0u8; 32]);
        assert!(run_round1(PartyIndex(0), 3, 5, &session).is_err(), "indeks 0 geçersiz");
        assert!(run_round1(PartyIndex(6), 3, 5, &session).is_err(), "indeks > n geçersiz");
    }

    #[test]
    fn test_round1_commitment_hash_deterministic_given_vss() {
        // Aynı VSS ile iki kez çağrılınca aynı commitment hash üretilmeli
        let vss = crate::vss::feldman_vss_generate(2, 3, Some([42u8; 32]), PartyIndex(1)).unwrap();
        let session = SessionId([0u8; 32]);
        let h1 = compute_commitment_hash(&vss.commitment, &session).unwrap();
        let h2 = compute_commitment_hash(&vss.commitment, &session).unwrap();
        assert_eq!(h1, h2, "deterministic hash");
    }

    #[test]
    fn test_verify_round1_valid_msg() {
        let parties: Vec<PartyIndex> = (1..=3).map(PartyIndex).collect();
        let session = test_session(&parties);
        let (msg, _) = run_round1(PartyIndex(1), 2, 3, &session).unwrap();
        assert!(verify_round1_msg(&msg, 3, &session).is_ok());
    }

    #[test]
    fn test_verify_round1_invalid_party() {
        let parties: Vec<PartyIndex> = (1..=3).map(PartyIndex).collect();
        let session = test_session(&parties);
        let (mut msg, _) = run_round1(PartyIndex(1), 2, 3, &session).unwrap();
        msg.from = PartyIndex(99); // geçersiz
        assert!(verify_round1_msg(&msg, 3, &session).is_err());
    }
}
