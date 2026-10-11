//! nova-mpc temel veri tipleri.
//!
//! §5.1 — MPC-Native Hesap Modeli veri yapıları.
//! §5.9.2 — Non-interactive pre-signing için `PreSignShare`.
//! §6.2 — Validator/Guardian key share güvenliği: `Zeroize` her hassas tipte.
//!
//! # Güvenlik Notları
//!
//! - Tüm gizli veriler (`KeyShare`, `LocalKey`, `PreSignShare`)
//!   `ZeroizeOnDrop` uygular — bellek serbest bırakıldığında otomatik silinir.
//! - Party index'ler 1'den başlar (0 rezerv — sonsuz nokta gibi anlamlı sıfırı önler).

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};
use nova_primitives::types::Threshold;
use crate::errors::{MpcError, MpcResult};

// ─── PARTY INDEX ──────────────────────────────────────────────────────────────

/// Protokol katılımcı indeksi.
///
/// 1'den başlar; `n` parti varsa indeksler `1..=n`.
/// Sıfır geçersizdir ve kesinlikle kullanılmamalıdır
/// (Lagrange interpolasyonu için tehlikeli).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct PartyIndex(pub u32);

impl PartyIndex {
    /// Geçerli bir party index oluştur.
    ///
    /// `n`: Toplam parti sayısı.
    pub fn new(index: u32, n: u32) -> MpcResult<Self> {
        if index == 0 || index > n {
            return Err(MpcError::InvalidPartyIndex { index, max: n });
        }
        Ok(Self(index))
    }

    /// Ham değer (güvensiz — sadece iç kullanım için).
    #[inline]
    pub fn get(self) -> u32 {
        self.0
    }

    /// Lagrange interpolasyonu için scalar'a dönüştür.
    /// `party_index * G` hesaplamalarında kullanılır.
    pub fn to_scalar_bytes(self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[28..].copy_from_slice(&self.0.to_be_bytes());
        bytes
    }
}

impl std::fmt::Display for PartyIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P{}", self.0)
    }
}

// ─── SESSION ID ───────────────────────────────────────────────────────────────

/// Protokol oturum kimliği — cross-session replay saldırısına karşı koruma.
///
/// §02-tss-lib.md §3.11: "SSID = BLAKE3(curve_params || parties || public_keys ||
/// NTildes || H1s || H2s || round || nonce)"
///
/// Her keygen/signing/reshare oturumu benzersiz bir SessionId taşır.
/// ZK kanıtlarına domain separator olarak göm��lür → farklı oturumlarda
/// aynı kanıt geçersizdir.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
    Zeroize,
)]
pub struct SessionId(pub [u8; 32]);

impl SessionId {
    /// Verilen 32 byte'tan session ID oluştur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Byte slice olarak eriş.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hex string gösterimi.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session:{}", &hex::encode(self.0)[..16])
    }
}

// ─── VSS COMMITMENT ───────────────────────────────────────────────────────────

/// Feldman VSS taahhüt dizisi — bir partinin polinom katsayı taahhütleri.
///
/// Parti `i`'nin `t+1` dereceli polinomu `f_i(x) = a_0 + a_1*x + ... + a_t*x^t`
/// için taahhütler: `C_{i,j} = a_j * G` (sıkıştırılmış secp256k1 noktası, 33 byte).
///
/// Doğrulama: `f_i(k) * G == sum(C_{i,j} * k^j)` — herkes halka açık taahhütle
/// kendi payını doğrulayabilir.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct VssCommitment {
    /// Taahhüdü gönderen parti.
    pub party: PartyIndex,
    /// `t+1` adet sıkıştırılmış secp256k1 noktası (her biri 33 byte).
    /// `points[j]` = `a_j * G`
    #[serde(with = "crate::serde_helpers::vec_bytes_33")]
    pub points: Vec<[u8; 33]>,
}

impl VssCommitment {
    /// Kaç katsayı taahhüdü var? `t+1` olmalı.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Boş mu?
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Threshold değeri: `t = points.len() - 1`.
    pub fn threshold(&self) -> usize {
        self.points.len().saturating_sub(1)
    }
}

// ─── ECDH EPHEMERAL KEY ───────────────────────────────────────────────────────

/// Round 1'de yayımlanan ephemeral ECDH public key.
///
/// Round 2'de P2P VSS paylarını şifrelemek için kullanılır.
/// Her keygen oturumu için ayrı ephemeral key üretilir.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct EcdhPublicKey {
    /// Sıkıştırılmış secp256k1 public key (33 byte).
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub compressed: [u8; 33],
}

// ─── LOCAL KEY — KEYGEN ÇIKTISI ───────────────────────────────────────────────

/// Keygen tamamlandıktan sonra her partide saklanan tam anahtar verisi.
///
/// Bu yapı `ZeroizeOnDrop` uygular — bellekten düşünce tüm gizli veriler silinir.
///
/// ## İçerik
/// - `secret_share`: Bu partinin Shamir gizli payı (`x_i`) — ASLA paylaşılmaz
/// - `public_key`: Agregat secp256k1 public key (`X = sum(a_{k,0} * G)`)
/// - `all_vss_commitments`: Tüm partilerin VSS taahhütleri (kamuya açık)
///
/// ## Depolama
/// §6.3: Guardian cihazlarda Secure Enclave içinde saklanır.
#[derive(Debug, Zeroize, ZeroizeOnDrop)]
pub struct LocalKey {
    /// Bu partinin indeksi.
    #[zeroize(skip)]
    pub party_index: PartyIndex,

    /// Threshold parametresi (örn: t=3, n=5 → 3/5).
    #[zeroize(skip)]
    pub threshold: Threshold,

    /// Toplam parti sayısı.
    pub party_count: u32,

    /// Bu partinin gizli Shamir payı — `x_i`. ZEROIZE ON DROP.
    pub secret_share: [u8; 32],

    /// Agregat secp256k1 public key (sıkıştırılmış, 33 byte).
    /// `X = sum(U_k)` = tüm partilerin katkı toplamı.
    pub public_key: [u8; 33],

    /// Oturum kimliği — bu keygen'e özgü.
    pub session_id: [u8; 32],

    /// Tüm partilerin VSS taahhütleri.
    /// Resharing ve signing doğrulama için gerekli.
    #[zeroize(skip)]
    pub all_vss_commitments: Vec<VssCommitment>,
}

impl LocalKey {
    /// Bir partinin public contribution'ı: `X_j = sum_k(FeldmanEval(VSS_k, j))`.
    ///
    /// Bu, party `j`'nin kendi payının doğruluğunu herkesin doğrulayabileceği
    /// public bilgidir.
    pub fn compute_public_share(&self, j: PartyIndex) -> MpcResult<[u8; 33]> {
        use crate::vss::feldman_eval_point;
        let mut sum_point = crate::crypto_utils::IDENTITY_BYTES;
        for vss in &self.all_vss_commitments {
            let p = feldman_eval_point(vss, j)?;
            sum_point = crate::crypto_utils::point_add(&sum_point, &p)?;
        }
        Ok(sum_point)
    }
}

// ─── PRE-SIGN SHARE ───────────────────────────────────────────────────────────

/// Tek bir partinin pre-signing aşamasında ürettiği nonce payı.
///
/// §5.9.2: "Guardian'lar boşta zamanlarında pre-signature shares üretir."
/// "Bu parçalar şifreli olarak depolanır (henüz bir TX'e bağlı değil)."
///
/// Bu yapı `ZeroizeOnDrop` uygular — `nonce` (r_i) gizlidir.
#[derive(Debug, Zeroize, ZeroizeOnDrop)]
pub struct PreSignShare {
    /// Hangi parti bu payı üretti.
    #[zeroize(skip)]
    pub party_index: PartyIndex,

    /// Gizli nonce: `r_i` (scalar). ZEROIZE ON DROP.
    /// Signing sonrası mutlaka sıfırlanmalı — tekrar kullanım imzayı kırar.
    pub nonce_secret: [u8; 32],

    /// Public nonce noktası: `R_i = r_i * G` (sıkıştırılmış, 33 byte).
    pub nonce_point: [u8; 33],

    /// Bu pre-sign payının geçerli olduğu son slot (TTL).
    /// §5.9.2: Pre-signing önceden yapılır, belirli bir slot için kullanılır.
    pub valid_until_slot: u64,

    /// Üretildiği session ID.
    pub session_id: [u8; 32],
}

// ─── PRESIGN STATE — ONLINE AŞAMASINA HAZIR ──────────────────────────────────

/// Tüm tarafların pre-sign payları toplandıktan sonra oluşan birleşik durum.
///
/// Bu noktadan itibaren online aşama sadece `~10ms` sürer (§5.9.2).
///
/// # Güvenlik
/// `nonce_used` bayrağı nonce yeniden kullanımını (NOVA-RED-MPC-001) önler.
/// `create_partial_signature()` çağrıldıktan sonra bu state tekrar kullanılamaz.
#[derive(Debug, Zeroize, ZeroizeOnDrop)]
pub struct PreSignState {
    /// Kendi parti indeksimiz.
    #[zeroize(skip)]
    pub our_index: PartyIndex,

    /// Bu imzada yer alan parti listesi (threshold kadar — t adet).
    #[zeroize(skip)]
    pub signers: Vec<PartyIndex>,

    /// Kendi gizli nonce'umuz (`r_i`). ZEROIZE ON DROP.
    pub our_nonce_secret: [u8; 32],

    /// Birleşik nonce noktası: `R = sum(R_i)` (sıkıştırılmış, 33 byte).
    pub combined_nonce: [u8; 33],

    /// Bu imzanın aggregate public key'i (33 byte).
    pub public_key: [u8; 33],

    /// Session ID.
    pub session_id: [u8; 32],

    /// Nonce kullanım bayrağı — tek kullanımlık koruma (NOVA-RED-MPC-001).
    /// AtomicBool interior mutability sayesinde &self üzerinden set edilir.
    /// true ise bu PreSignState artık geçersiz — aynı nonce iki farklı mesaj
    /// için kullanılırsa gizli pay matematiksel olarak türetilebilir.
    #[zeroize(skip)]
    pub nonce_used: std::sync::atomic::AtomicBool,

    /// NOVA-RED-MPC-002: Her imzacı partinin nonce noktası R_i (sıkıştırılmış, 33 byte).
    /// `signers` listesiyle aynı sırada: signers[i] ↔ nonce_points[i].
    /// `aggregate_partial_signatures()` bu noktaları kullanarak her partial imzayı
    /// `verify_partial_signature()` ile doğrular.
    #[zeroize(skip)]
    pub nonce_points: Vec<[u8; 33]>,
}

// ─── PARTIAL SIGNATURE ───────────────────────────────────────────────────────

/// Online aşamada bir partinin ürettiği kısmi imza.
///
/// `s_i = r_i + challenge * x_i * lagrange_i`
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PartialSignature {
    /// İmzayı üreten parti.
    pub party_index: PartyIndex,

    /// Kısmi imza skaleri `s_i` (32 byte, big-endian).
    pub s_i: [u8; 32],

    /// Session ID (doğrulama için).
    pub session_id: [u8; 32],
}

// ─── THRESHOLD SIGNATURE — SON ÇIKTI ���────────────────────────────────────────

/// Threshold Schnorr imzası — tüm kısmi imzalar birleştirildikten sonra.
///
/// Doğrulama: `s * G == R + challenge * X`
/// Burada `challenge = BLAKE3("nova-sign-v1" || R || X || message)`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ThresholdSignature {
    /// Nonce noktası `R = sum(R_i)` (sıkıştırılmış secp256k1, 33 byte).
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub r_point: [u8; 33],

    /// Toplam imza skaleri `s = sum(s_i)` (32 byte, big-endian).
    pub s_scalar: [u8; 32],
}

impl ThresholdSignature {
    /// İmzayı byte dizisine serileştir (r || s, 65 byte).
    pub fn to_bytes(&self) -> [u8; 65] {
        let mut out = [0u8; 65];
        out[..33].copy_from_slice(&self.r_point);
        out[33..].copy_from_slice(&self.s_scalar);
        out
    }

    /// Byte dizisinden imza çıkar (65 byte).
    pub fn from_bytes(bytes: &[u8; 65]) -> Self {
        let mut r = [0u8; 33];
        let mut s = [0u8; 32];
        r.copy_from_slice(&bytes[..33]);
        s.copy_from_slice(&bytes[33..]);
        Self { r_point: r, s_scalar: s }
    }
}

// ─── KEYGEN ROUND MESAJLARI ───────────────────────────────────────────────────

/// Round 1 broadcast: taahhüt + ECDH public key.
///
/// VSS commitments henüz açıklanmaz — sadece hash'i gönderilir.
/// Bu, diğer partilerin mesajlarını görünce kendi mesajlarını değiştirmesini önler.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct KeygenRound1Msg {
    pub from: PartyIndex,
    /// `BLAKE3("nova-keygen-r1" || vss_commitment_bytes || session_id)`
    pub commitment_hash: [u8; 32],
    /// Schnorr DLog kanıtı — gizli katkı `u_i` bilindiğini kanıtlar.
    pub dlog_proof: DlogProofBytes,
    /// Ephemeral ECDH public key — Round 2 P2P şifreleme için.
    pub ecdh_pk: EcdhPublicKey,
}

/// Round 2 broadcast + P2P mesajları.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct KeygenRound2BroadcastMsg {
    pub from: PartyIndex,
    /// VSS taahhüt açılımı — Round 1 taahhüdüyle eşleşmeli.
    pub vss_commitment: VssCommitment,
}

/// Round 2 P2P mesajı — alıcıya özel VSS payı.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct KeygenRound2P2pMsg {
    pub from: PartyIndex,
    pub to: PartyIndex,
    /// AES-256-GCM ile şifrelenmiş `f_from(to)` skaleri (32 byte + nonce + tag).
    pub encrypted_share_nonce: [u8; 12],
    pub encrypted_share_ciphertext: Vec<u8>,
    // AAD: `session_id || from.get().to_le_bytes() || to.get().to_le_bytes()`
    // (şifreli değil, MAC'e dahil — mesaj kimliğini bağlar)
}

/// Round 3 broadcast — kendi public key katkısı + kanıt.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct KeygenRound3Msg {
    pub from: PartyIndex,
    /// Bu partinin final key share'inin public versiyonu: `X_i = x_i * G`.
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub public_share: [u8; 33],
    /// `X_i` için Schnorr DLog kanıtı.
    pub dlog_proof: DlogProofBytes,
}

// ─── SIGNING ROUND MESAJLARI ──────────────────────────────────────────────────

/// Pre-signing Round 1: nonce taahhüdü.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PresignRound1Msg {
    pub from: PartyIndex,
    /// `BLAKE3("nova-presign-r1" || R_i || session_id)`
    pub nonce_commitment: [u8; 32],
}

/// Pre-signing Round 2: nonce açılımı.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PresignRound2Msg {
    pub from: PartyIndex,
    /// `R_i = r_i * G` (sıkıştırılmış, 33 byte).
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub nonce_point: [u8; 33],
    /// Schnorr DLog kanıtı — `r_i` bilindiğini kanıtlar.
    pub dlog_proof: DlogProofBytes,
}

/// Online signing mesajı: kısmi imza.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct SigningMsg {
    pub partial: PartialSignature,
}

// ─── RESHARING MESAJLARI ──────────────────────────────────────────────────────

/// Resharing Round 1 — eski taraf, yeni komiteye payını VSS ile gönderir.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ReshareRound1Msg {
    pub from: PartyIndex,
    /// Yeni komite için VSS taahhütleri.
    pub new_vss_commitment: VssCommitment,
    /// `a_{i,0} = x_i` (kendi share'i) için DLog kanıtı.
    pub dlog_proof: DlogProofBytes,
}

/// Resharing Round 2 P2P — yeni parti `j`'ye şifreli pay.
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ReshareRound2P2pMsg {
    pub from: PartyIndex,
    pub to: PartyIndex,
    /// AES-GCM ile şifrelenmiş `f_i(j)`.
    pub encrypted_share_nonce: [u8; 12],
    pub encrypted_share_ciphertext: Vec<u8>,
}

// ─── DLOG PROOF BYTES ─────────────────────────────────────────────────────────

/// Schnorr DLog kanıtının serileştirilmiş hali.
/// Tam implementasyon `dlog.rs`'de.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DlogProofBytes {
    /// `R = r * G` — nonce noktası (sıkıştırılmış, 33 byte).
    #[serde(with = "crate::serde_helpers::bytes_33")]
    pub nonce_point: [u8; 33],
    /// `s = r + challenge * secret` — yanıt skaleri (32 byte).
    pub response: [u8; 32],
}
