//! Viewing key hiyerarşisi — Penumbra FVK/IVK/OVK pattern'inden ilham
//!
//! FullViewingKey: hem gelen hem giden note'ları görüntüler (regülatör için)
//! IncomingViewingKey: sadece gelen note'ları tespit eder
//! OutgoingViewingKey: gönderenin kendi gönderilerini görüntüler

use crate::keys::spending::SpendingKey;
use crate::utxo::note::NoteCommitment;
use crate::utxo::nullifier::NullifierKey;
use serde::{Deserialize, Serialize};

// ─── IncomingViewingKey ───────────────────────────────────────────────────────

/// IVK — gelen note'ları tespit etmek için paylaşılabilir
/// Penumbra: ivk = crh(ak, nk)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingViewingKey(pub [u8; 32]);

impl IncomingViewingKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Gelen şifreli not sahibine ait mi? (şifreli not çözme simülasyonu)
    /// Gerçekte ECDH ile şifre çözme; burada BLAKE3 kontrolü
    pub fn can_detect_note(&self, commitment: &NoteCommitment) -> bool {
        // Mock: commitment'ın IVK ile türetilen "tagging key" ile eşleşmesi
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-shield:ivk-tag:v1!!!!!!!!!!!");
        hasher.update(&self.0);
        let tag = hasher.finalize();
        // Gerçekte: şifreli not payload'u çöz ve commitment'ı doğrula
        // Mock: ilk 2 byte kontrol
        tag.as_bytes()[..2] == commitment.as_bytes()[..2]
    }
}

// ─── OutgoingViewingKey ───────────────────────────────────────────────────────

/// OVK — gönderenin kendi gönderilerini görüntülemesi için
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingViewingKey(pub [u8; 32]);

impl OutgoingViewingKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─── OVK WRAPPED KEY ─────────────────────────────────────────────────────────

/// AŞAMA 9 Görev 66: OVK wrapped key — gönderen kendi notunu okuyabilir.
///
/// ## Zcash/Penumbra Referansı
///
/// Zcash Sapling (ZIP 308): Her transfer'da OVK ile şifreli "wrapped key" üretilir.
/// Bu sayede gönderen, kendi OVK'sına sahip olduğu sürece
/// gönderi ayrıntılarını (alıcı, miktar, randomness) her zaman çözebilir.
///
/// ```text
/// K_enc = BLAKE3("OVK_WRAP" || ovk || cm || cv)
/// C_enc = K_enc XOR (memo_key || esk)
/// ```
///
/// ## Nova Uyarlaması
///
/// `OvkWrappedKey` = BLAKE3("nova-shield:ovk-wrap:v1!" || ovk || cm) XOR note_bytes
///
/// - `cm`: Note commitment (public, zincirde)
/// - `note_bytes`: [value(16) || asset_id(32) || randomness(32)] — gizli
/// - Sonuç 80 byte şifreli blob — zincirde veya memo'da saklanır
pub const OVK_WRAPPED_SIZE: usize = 80;

/// OVK ile sarılmış (wrapped) not verisi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OvkWrappedKey(pub [u8; OVK_WRAPPED_SIZE]);

impl serde::Serialize for OvkWrappedKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}
impl<'de> serde::Deserialize<'de> for OvkWrappedKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = OvkWrappedKey;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "80 byte OvkWrappedKey")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<OvkWrappedKey, E> {
                if v.len() != OVK_WRAPPED_SIZE {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut b = [0u8; OVK_WRAPPED_SIZE];
                b.copy_from_slice(v);
                Ok(OvkWrappedKey(b))
            }
        }
        d.deserialize_bytes(V)
    }
}

impl OutgoingViewingKey {
    /// Not verilerini OVK ile şifrele (gönderen için kayıt).
    ///
    /// `cm`: Note commitment (public)
    /// `value`: Gönderilen miktar
    /// `asset_id`: Token türü
    /// `randomness`: Note randomness
    pub fn wrap_note(
        &self,
        cm: &NoteCommitment,
        value: u128,
        asset_id: [u8; 32],
        randomness: [u8; 32],
    ) -> OvkWrappedKey {
        // Şifreleme anahtarı = BLAKE3(domain || ovk || cm)
        let mut key_hasher = blake3::Hasher::new();
        key_hasher.update(b"nova-shield:ovk-wrap:v1!!!!");
        key_hasher.update(&self.0);
        key_hasher.update(cm.as_bytes());
        let key = *key_hasher.finalize().as_bytes(); // 32 byte

        // Düz metin: value(16) || asset_id(32) || randomness(32) = 80 byte
        let mut plaintext = [0u8; OVK_WRAPPED_SIZE];
        plaintext[..16].copy_from_slice(&value.to_le_bytes());
        plaintext[16..48].copy_from_slice(&asset_id);
        plaintext[48..80].copy_from_slice(&randomness);

        // XOR şifreleme (key'i genişlet — stream cipher simülasyonu)
        let mut encrypted = [0u8; OVK_WRAPPED_SIZE];
        for i in 0..OVK_WRAPPED_SIZE {
            encrypted[i] = plaintext[i] ^ key[i % 32] ^ (i as u8);
        }

        OvkWrappedKey(encrypted)
    }

    /// Wrapped key'i çöz.
    ///
    /// Gönderenin OVK'sı ve commitment biliniyorsa nota erişilebilir.
    pub fn unwrap_note(
        &self,
        wrapped: &OvkWrappedKey,
        cm: &NoteCommitment,
    ) -> (u128, [u8; 32], [u8; 32]) {
        // Aynı anahtar türetimi
        let mut key_hasher = blake3::Hasher::new();
        key_hasher.update(b"nova-shield:ovk-wrap:v1!!!!");
        key_hasher.update(&self.0);
        key_hasher.update(cm.as_bytes());
        let key = *key_hasher.finalize().as_bytes();

        // XOR çözme
        let mut plaintext = [0u8; OVK_WRAPPED_SIZE];
        for i in 0..OVK_WRAPPED_SIZE {
            plaintext[i] = wrapped.0[i] ^ key[i % 32] ^ (i as u8);
        }

        let value = u128::from_le_bytes(plaintext[..16].try_into().unwrap());
        let asset_id: [u8; 32] = plaintext[16..48].try_into().unwrap();
        let randomness: [u8; 32] = plaintext[48..80].try_into().unwrap();

        (value, asset_id, randomness)
    }
}

// ─── ZIP 212 EPK INTEGRITY CHECK ─────────────────────────────────────────────

/// AŞAMA 9 Görev 67: ZIP 212 epk integrity check — re-encryption koruması.
///
/// ## Zcash ZIP 212
///
/// Zcash Sapling'de saldırı: kötü niyetli taraf meşru `epk` yerine
/// kendi `epk`'sını koyarak alıcının notunu ele geçirebilir.
///
/// ZIP 212 çözümü: not şifrelemesinin içine `epk`'nın hash'ini gömme.
///
/// ```text
/// // ZIP 212 note plaintext:
/// leadByte = 0x02  (ZIP 212 versiyonu)
/// rseed = PRF_expand(esk, [0x04]) — not randomness
/// rcv = PRF_expand(esk, [0x05]) — value commitment
/// memo[0..511] = şifreli memo
/// ```
///
/// ## Nova Uyarlaması
///
/// `epk_integrity_check(epk, cm, encrypted_note)`:
/// - Encrypted note içinde `epk_hash = BLAKE3("epk-bind" || epk)` bulunmalı
/// - Bu sayede başka bir epk ile re-encryption tespit edilir

pub const EPK_BIND_TAG: &[u8] = b"nova-shield:epk-bind:v1!!!!!!!!";

/// Ephemeral public key (Diffie-Hellman için kullanılır).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EphemeralPublicKey(pub [u8; 32]);

impl EphemeralPublicKey {
    pub fn new(bytes: [u8; 32]) -> Self { Self(bytes) }

    /// ZIP 212: EPK'yı nota commitment'ı ile bağlayan integrity tag üret.
    ///
    /// Bu tag şifreli nota içine eklenir.
    /// Doğrulamada: aynı epk + cm ile aynı tag oluşturulur → eşleşmeli.
    pub fn integrity_tag(&self, cm: &NoteCommitment) -> [u8; 16] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(EPK_BIND_TAG);
        hasher.update(&self.0);
        hasher.update(cm.as_bytes());
        let hash = hasher.finalize();
        hash.as_bytes()[..16].try_into().unwrap()
    }

    /// ZIP 212 integrity kontrol — re-encryption saldırısını tespit et.
    ///
    /// `expected_tag`: Şifreli nota içindeki tag (ilk 16 byte)
    /// `cm`: Public note commitment
    ///
    /// Returns: `true` = geçerli (orijinal epk), `false` = re-encryption saldırısı
    pub fn verify_integrity(&self, expected_tag: &[u8; 16], cm: &NoteCommitment) -> bool {
        let computed = self.integrity_tag(cm);
        // Sabit zamanlı karşılaştırma (timing attack önlemi)
        expected_tag.iter().zip(computed.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// Şifreli nota başlığı (ZIP 212 uyumlu).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedNoteHeader {
    /// Ephemeral public key (DH için).
    pub epk: EphemeralPublicKey,
    /// ZIP 212 integrity tag (epk + cm'ye bağlı).
    pub epk_integrity_tag: [u8; 16],
    /// Şifreli not yükü.
    pub ciphertext: Vec<u8>,
}

impl EncryptedNoteHeader {
    /// Yeni şifreli nota başlığı oluştur.
    pub fn new(epk: EphemeralPublicKey, cm: &NoteCommitment, ciphertext: Vec<u8>) -> Self {
        let epk_integrity_tag = epk.integrity_tag(cm);
        Self { epk, epk_integrity_tag, ciphertext }
    }

    /// ZIP 212 integrity doğrula.
    pub fn verify_epk_integrity(&self, cm: &NoteCommitment) -> bool {
        self.epk.verify_integrity(&self.epk_integrity_tag, cm)
    }
}

// ─── FullViewingKey ───────────────────────────────────────────────────────────

/// FVK — hem gelen hem giden işlemleri izleme yeteneği
/// Penumbra FVK = (ak, nk) pair — Nova: basitleştirilmiş 3-tuple
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FullViewingKey {
    pub incoming: IncomingViewingKey,
    pub outgoing: OutgoingViewingKey,
    pub nullifier_key: NullifierKey,
}

impl FullViewingKey {
    /// SpendingKey'den deterministik türet (Penumbra pattern)
    pub fn from_spending_key(sk: &SpendingKey) -> Self {
        let sk_bytes = sk.to_bytes();

        // IVK = BLAKE3("nova-ivk" || sk_bytes)
        let ivk_bytes = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:incoming-viewing-key!");
            h.update(&sk_bytes);
            *h.finalize().as_bytes()
        };

        // OVK = BLAKE3("nova-ovk" || sk_bytes)
        let ovk_bytes = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:outgoing-viewing-key!");
            h.update(&sk_bytes);
            *h.finalize().as_bytes()
        };

        Self {
            incoming: IncomingViewingKey(ivk_bytes),
            outgoing: OutgoingViewingKey(ovk_bytes),
            nullifier_key: sk.nullifier_key(),
        }
    }

    /// Ödeme adresi üret (diversified address)
    pub fn payment_address(&self, index: u64) -> super::diversified::DiversifiedAddress {
        super::diversified::DiversifiedAddress::new(&self.incoming, index)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn sk_from_seed(seed: u64) -> SpendingKey {
        let mut rng = StdRng::seed_from_u64(seed);
        SpendingKey::generate(&mut rng)
    }

    #[test]
    fn test_fvk_from_spending_key_deterministic() {
        let sk = sk_from_seed(42);
        let fvk1 = FullViewingKey::from_spending_key(&sk);
        let fvk2 = FullViewingKey::from_spending_key(&sk);
        assert_eq!(fvk1.incoming, fvk2.incoming);
        assert_eq!(fvk1.outgoing, fvk2.outgoing);
        assert_eq!(fvk1.nullifier_key, fvk2.nullifier_key);
    }

    #[test]
    fn test_fvk_different_for_different_spending_keys() {
        let sk1 = sk_from_seed(1);
        let sk2 = sk_from_seed(2);
        let fvk1 = FullViewingKey::from_spending_key(&sk1);
        let fvk2 = FullViewingKey::from_spending_key(&sk2);
        assert_ne!(fvk1.incoming, fvk2.incoming);
        assert_ne!(fvk1.outgoing, fvk2.outgoing);
    }

    #[test]
    fn test_ivk_nk_from_same_spending_key_consistent() {
        let sk = sk_from_seed(7);
        let fvk = FullViewingKey::from_spending_key(&sk);
        // NullifierKey FVK'dan ve SK'dan türetilen aynı olmalı
        assert_eq!(fvk.nullifier_key, sk.nullifier_key());
    }

    #[test]
    fn test_payment_address_different_per_index() {
        let sk = sk_from_seed(10);
        let fvk = FullViewingKey::from_spending_key(&sk);
        let addr0 = fvk.payment_address(0);
        let addr1 = fvk.payment_address(1);
        assert_ne!(addr0.transmission_key(), addr1.transmission_key());
    }

    #[test]
    fn test_payment_address_deterministic() {
        let sk = sk_from_seed(10);
        let fvk = FullViewingKey::from_spending_key(&sk);
        let addr_a = fvk.payment_address(5);
        let addr_b = fvk.payment_address(5);
        assert_eq!(addr_a.transmission_key(), addr_b.transmission_key());
    }

    // ─── OVK Wrapped Key Testleri ─────────────────────────────────────────────

    #[test]
    fn test_ovk_wrap_unwrap_roundtrip() {
        let sk = sk_from_seed(99);
        let fvk = FullViewingKey::from_spending_key(&sk);
        let ovk = &fvk.outgoing;

        let cm = NoteCommitment([0xABu8; 32]);
        let value = 1_000_000_000u128;
        let asset_id = [0x01u8; 32];
        let randomness = [0x42u8; 32];

        let wrapped = ovk.wrap_note(&cm, value, asset_id, randomness);
        let (v2, a2, r2) = ovk.unwrap_note(&wrapped, &cm);

        assert_eq!(v2, value);
        assert_eq!(a2, asset_id);
        assert_eq!(r2, randomness);
    }

    #[test]
    fn test_ovk_wrong_key_cannot_decrypt() {
        let sk1 = sk_from_seed(1);
        let sk2 = sk_from_seed(2);
        let fvk1 = FullViewingKey::from_spending_key(&sk1);
        let fvk2 = FullViewingKey::from_spending_key(&sk2);

        let cm = NoteCommitment([0xCDu8; 32]);
        let value = 5_000u128;
        let asset_id = [0x02u8; 32];
        let randomness = [0x77u8; 32];

        let wrapped = fvk1.outgoing.wrap_note(&cm, value, asset_id, randomness);
        // Farklı OVK ile çöz → yanlış sonuç
        let (wrong_value, _, _) = fvk2.outgoing.unwrap_note(&wrapped, &cm);
        assert_ne!(wrong_value, value);
    }

    #[test]
    fn test_ovk_deterministic_wrap() {
        let sk = sk_from_seed(7);
        let fvk = FullViewingKey::from_spending_key(&sk);
        let cm = NoteCommitment([0x11u8; 32]);
        let w1 = fvk.outgoing.wrap_note(&cm, 100, [0u8; 32], [1u8; 32]);
        let w2 = fvk.outgoing.wrap_note(&cm, 100, [0u8; 32], [1u8; 32]);
        assert_eq!(w1, w2);
    }

    // ─── ZIP 212 EPK Integrity Testleri ──────────────────────────────────────

    #[test]
    fn test_epk_integrity_valid() {
        let epk = EphemeralPublicKey::new([0x55u8; 32]);
        let cm = NoteCommitment([0xAAu8; 32]);
        let tag = epk.integrity_tag(&cm);
        assert!(epk.verify_integrity(&tag, &cm));
    }

    #[test]
    fn test_epk_integrity_wrong_epk_detected() {
        let epk1 = EphemeralPublicKey::new([0x55u8; 32]);
        let epk2 = EphemeralPublicKey::new([0x66u8; 32]); // farklı epk
        let cm = NoteCommitment([0xAAu8; 32]);

        let tag = epk1.integrity_tag(&cm);
        // epk2 ile doğrula → re-encryption saldırısı tespit edilmeli
        assert!(!epk2.verify_integrity(&tag, &cm));
    }

    #[test]
    fn test_epk_integrity_wrong_cm_detected() {
        let epk = EphemeralPublicKey::new([0x55u8; 32]);
        let cm1 = NoteCommitment([0xAAu8; 32]);
        let cm2 = NoteCommitment([0xBBu8; 32]);

        let tag = epk.integrity_tag(&cm1);
        assert!(!epk.verify_integrity(&tag, &cm2));
    }

    #[test]
    fn test_encrypted_note_header_roundtrip() {
        let epk = EphemeralPublicKey::new([0x33u8; 32]);
        let cm = NoteCommitment([0x44u8; 32]);
        let ciphertext = vec![0xFFu8; 100];

        let header = EncryptedNoteHeader::new(epk, &cm, ciphertext);
        assert!(header.verify_epk_integrity(&cm));
        // Farklı cm ile doğrula → başarısız
        let wrong_cm = NoteCommitment([0x55u8; 32]);
        assert!(!header.verify_epk_integrity(&wrong_cm));
    }
}
