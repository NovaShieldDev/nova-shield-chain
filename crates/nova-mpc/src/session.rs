//! MPC protokol oturum yönetimi ve Session ID hesaplama.
//!
//! §02-tss-lib.md §3.11: SSID tasarımından ilham alındı.
//!
//! ## Session ID (SSID) Neden Kritik?
//!
//! ZK kanıtlarına (Schnorr DLog, VSS doğrulama) SSID domain separator
//! olarak gömülür. Bu şu saldırıyı önler:
//!
//! - Saldırgan A ile B arasındaki bir protokolden geçerli bir kanıtı alır
//! - Aynı kanıtı C ile D arasındaki başka bir protokolde tekrar kullanmaya çalışır
//! - SSID farklı olduğu için kanıt geçersiz → saldırı başarısız
//!
//! Nova Shield ek olarak `session_nonce` kullanır → aynı katılımcılar
//! arasındaki iki farklı protokol de farklı SSID'ye sahip olur.

use crate::types::{PartyIndex, SessionId};
use blake3::Hasher;
use nova_primitives::types::ChainId;

/// Keygen oturumu için SSID hesapla.
///
/// `chain_id`: Nova Shield mainnet=1984
/// `parties`: Katılımcı parti listesi (sıralanmış)
/// `nonce`: Bu oturuma özgü rastgele 32 byte
pub fn compute_keygen_ssid(
    chain_id: ChainId,
    parties: &[PartyIndex],
    nonce: &[u8; 32],
) -> SessionId {
    let mut hasher = Hasher::new_derive_key("nova-shield-mpc-keygen-ssid-v1");
    // Zincir kimliği
    hasher.update(&chain_id.0.to_le_bytes());
    // Parti sayısı (değişken uzunluk koruması)
    hasher.update(&(parties.len() as u64).to_le_bytes());
    // Sıralanmış parti indeksleri
    let mut sorted = parties.to_vec();
    sorted.sort_by_key(|p| p.0);
    for p in &sorted {
        hasher.update(&p.0.to_le_bytes());
    }
    // Bu oturuma özgü nonce
    hasher.update(nonce);
    SessionId(*hasher.finalize().as_bytes())
}

/// Signing oturumu için SSID hesapla.
///
/// Keygen SSID'sini baz alır + mesaj hash'i + round numarası ekler.
/// → Farklı mesajların imzalanması farklı SSID verir.
pub fn compute_signing_ssid(
    keygen_session: &SessionId,
    message_hash: &[u8; 32],
    signers: &[PartyIndex],
    nonce: &[u8; 32],
) -> SessionId {
    let mut hasher = Hasher::new_derive_key("nova-shield-mpc-signing-ssid-v1");
    hasher.update(keygen_session.as_bytes());
    hasher.update(message_hash);
    // Sıralanmış imzacılar
    let mut sorted = signers.to_vec();
    sorted.sort_by_key(|p| p.0);
    hasher.update(&(sorted.len() as u64).to_le_bytes());
    for p in &sorted {
        hasher.update(&p.0.to_le_bytes());
    }
    hasher.update(nonce);
    SessionId(*hasher.finalize().as_bytes())
}

/// Pre-signing oturumu için SSID hesapla.
///
/// Pre-signing, mesaj bilinmeden önceden yapılır.
/// Bu yüzden mesaj hash yerine slot numarası ve parti listesi kullanılır.
pub fn compute_presign_ssid(
    keygen_session: &SessionId,
    signers: &[PartyIndex],
    valid_until_slot: u64,
    nonce: &[u8; 32],
) -> SessionId {
    let mut hasher = Hasher::new_derive_key("nova-shield-mpc-presign-ssid-v1");
    hasher.update(keygen_session.as_bytes());
    hasher.update(&valid_until_slot.to_le_bytes());
    let mut sorted = signers.to_vec();
    sorted.sort_by_key(|p| p.0);
    hasher.update(&(sorted.len() as u64).to_le_bytes());
    for p in &sorted {
        hasher.update(&p.0.to_le_bytes());
    }
    hasher.update(nonce);
    SessionId(*hasher.finalize().as_bytes())
}

/// Resharing oturumu için SSID hesapla.
pub fn compute_reshare_ssid(
    old_session: &SessionId,
    old_parties: &[PartyIndex],
    new_parties: &[PartyIndex],
    new_threshold: u8,
    nonce: &[u8; 32],
) -> SessionId {
    let mut hasher = Hasher::new_derive_key("nova-shield-mpc-reshare-ssid-v1");
    hasher.update(old_session.as_bytes());
    // Eski komite
    let mut old_sorted = old_parties.to_vec();
    old_sorted.sort_by_key(|p| p.0);
    hasher.update(&(old_sorted.len() as u64).to_le_bytes());
    for p in &old_sorted {
        hasher.update(&p.0.to_le_bytes());
    }
    // Yeni komite
    let mut new_sorted = new_parties.to_vec();
    new_sorted.sort_by_key(|p| p.0);
    hasher.update(&(new_sorted.len() as u64).to_le_bytes());
    for p in &new_sorted {
        hasher.update(&p.0.to_le_bytes());
    }
    hasher.update(&[new_threshold]);
    hasher.update(nonce);
    SessionId(*hasher.finalize().as_bytes())
}

/// Session ID doğrulama — sabit-zaman karşılaştırma.
///
/// Gelen mesajın Session ID'si beklenenle eşleşmeli.
/// Timing attack önlemek için `subtle::ConstantTimeEq` kullanılır.
pub fn verify_session_id(
    expected: &SessionId,
    got: &SessionId,
) -> crate::errors::MpcResult<()> {
    use subtle::ConstantTimeEq;
    let ok: bool = expected.as_bytes().ct_eq(got.as_bytes()).into();
    if ok {
        Ok(())
    } else {
        Err(crate::errors::MpcError::SessionMismatch {
            expected: expected.as_bytes().to_vec(),
            got: got.as_bytes().to_vec(),
        })
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::types::ChainId;

    fn test_parties() -> Vec<PartyIndex> {
        vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)]
    }

    fn zero_nonce() -> [u8; 32] {
        [0u8; 32]
    }

    #[test]
    fn test_keygen_ssid_deterministic() {
        let parties = test_parties();
        let nonce = zero_nonce();
        let s1 = compute_keygen_ssid(ChainId(1984), &parties, &nonce);
        let s2 = compute_keygen_ssid(ChainId(1984), &parties, &nonce);
        assert_eq!(s1, s2, "aynı girdi → aynı SSID");
    }

    #[test]
    fn test_keygen_ssid_different_nonce() {
        let parties = test_parties();
        let s1 = compute_keygen_ssid(ChainId(1984), &parties, &[0u8; 32]);
        let s2 = compute_keygen_ssid(ChainId(1984), &parties, &[1u8; 32]);
        assert_ne!(s1, s2, "farklı nonce → farklı SSID");
    }

    #[test]
    fn test_keygen_ssid_different_chain() {
        let parties = test_parties();
        let nonce = zero_nonce();
        let s1 = compute_keygen_ssid(ChainId(1984), &parties, &nonce);
        let s2 = compute_keygen_ssid(ChainId(1), &parties, &nonce);
        assert_ne!(s1, s2, "farklı chain ID → farklı SSID");
    }

    #[test]
    fn test_keygen_ssid_party_order_invariant() {
        // Parti sırası SSID'yi etkilememeli (sıralama yapılıyor)
        let parties_asc = vec![PartyIndex(1), PartyIndex(2), PartyIndex(3)];
        let parties_desc = vec![PartyIndex(3), PartyIndex(2), PartyIndex(1)];
        let nonce = zero_nonce();
        let s1 = compute_keygen_ssid(ChainId(1984), &parties_asc, &nonce);
        let s2 = compute_keygen_ssid(ChainId(1984), &parties_desc, &nonce);
        assert_eq!(s1, s2, "parti sırası SSID'yi etkilememeli");
    }

    #[test]
    fn test_different_protocol_different_ssid() {
        // Keygen, signing, presign SSID'leri birbirinden farklı
        let parties = test_parties();
        let nonce = zero_nonce();
        let keygen_ssid = compute_keygen_ssid(ChainId(1984), &parties, &nonce);

        let msg_hash = [42u8; 32];
        let signing_ssid = compute_signing_ssid(&keygen_ssid, &msg_hash, &parties, &nonce);

        assert_ne!(
            keygen_ssid, signing_ssid,
            "keygen ve signing SSID farklı olmalı"
        );
    }

    #[test]
    fn test_session_verify_success() {
        let s = SessionId([99u8; 32]);
        assert!(verify_session_id(&s, &s).is_ok(), "aynı session ID geçmeli");
    }

    #[test]
    fn test_session_verify_mismatch() {
        let s1 = SessionId([1u8; 32]);
        let s2 = SessionId([2u8; 32]);
        assert!(
            verify_session_id(&s1, &s2).is_err(),
            "farklı session ID reddedilmeli"
        );
    }
}
