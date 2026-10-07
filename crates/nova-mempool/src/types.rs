//! nova-mempool temel veri tipleri.
//!
//! §5.2 — Şifreli Mempool veri yapıları:
//! - [`RawTransaction`] — kullanıcının gönderdiği düz TX
//! - [`EncryptedTx`] — threshold IBE ile şifrelenmiş TX (mempool'a gönderilir)
//! - [`DecryptedTx`] — threshold açılma sonrası elde edilen TX
//! - [`MempoolEntry`] — havuzda saklanan kayıt
//! - [`SlotBatch`] — tek bir slot için bekleyen tüm TX'ler
//! - [`RevealedBatch`] — threshold decryption sonrası açılan TX'ler (batch auction'a gider)
//! - [`TxDecryptionShare`] — bir validator'ın tek TX için decryption share'i
//!
//! ## Şifreleme Şeması (ECDH-tabanlı Threshold, §6.1.4)
//!
//! ```text
//! Şifreleme:
//!   e    = random scalar (ephemeral secret)
//!   E    = e * G              (32 byte, ephemeral pubkey = C1)
//!   S    = e * threshold_pk  (ECDH shared secret point)
//!   K    = BLAKE3("nova-mempool-aes-key" || S.compress())  [32 byte AES anahtarı]
//!   aes_nonce  = random [12 byte]
//!   padded_body = pad_to_512(borsh_serialize(raw_tx))
//!   aes_cipher  = AES-GCM(K, nonce=aes_nonce, aad=slot_bytes, msg=padded_body)
//!   commitment  = BLAKE3(E || aes_cipher || aes_nonce || slot_le_bytes)
//!
//! Açılma (Threshold Decryption):
//!   D_i  = sk_share_i * E   (her validator'ın decryption share'i)
//!   S    = Lagrange_interpolate(D_1..D_t)  = sk_threshold * E = e * threshold_pk
//!   K    = BLAKE3("nova-mempool-aes-key" || S.compress())
//!   body = AES-GCM_decrypt(K, aes_nonce, slot_bytes, aes_cipher)
//!   assert BLAKE3(E || aes_cipher || aes_nonce || slot_le_bytes) == commitment
//! ```

use borsh::{BorshDeserialize, BorshSerialize};
use nova_primitives::{
    constants::{MAX_ENCRYPTED_TX_BYTES, MAX_ENCRYPTION_PADDING_BYTES},
    types::{Address, Balance, Nonce, SlotId},
};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ─── TX HASH ─────────────────────────────────────────────────────────────────

/// TX'in kriptografik kimliği — commitment hash'i. §5.2.2
/// BLAKE3(E || aes_cipher || aes_nonce || slot_le_bytes)
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct TxHash([u8; 32]);

impl TxHash {
    /// Ham byte'lardan oluştur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Sıfır hash (test yardımcısı).
    pub const ZERO: Self = Self([0u8; 32]);
}

impl std::fmt::Debug for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TxHash(0x{}...)", &self.to_hex()[..16])
    }
}

impl std::fmt::Display for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", self.to_hex())
    }
}

// ─── TX TİPİ ─────────────────────────────────────────────────────────────────

/// İşlem türü. §5.2 — farklı TX türleri farklı işleme tabi tutulur.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum TxType {
    /// Değer transferi (NSC gönderi).
    Transfer = 0,
    /// Akıllı kontrat çağrısı.
    ContractCall = 1,
    /// Akıllı kontrat dağıtımı.
    ContractDeploy = 2,
    /// Heartbeat mesajı — deadman switch sıfırlama, ücretsiz. §5.4.9
    Heartbeat = 3,
    /// Guardian operasyonu (guardian atama/değiştirme). §5.1.5
    GuardianOp = 4,
    /// Hesap dondurma/çözme. §5.5
    FreezeOp = 5,
    /// Miras protokolü operasyonu. §5.4
    InheritanceOp = 6,
    /// Bilinmiyor / ileride eklenecek.
    Unknown = 255,
}

impl TxType {
    /// Bu TX türü gas fee'den muaf mı? §8.3
    pub fn is_fee_exempt(&self) -> bool {
        matches!(self, TxType::Heartbeat)
    }

    /// Bu TX türü guardian değişikliği içeriyor mu?
    pub fn is_guardian_op(&self) -> bool {
        matches!(self, TxType::GuardianOp)
    }
}

// ─── HAM İŞLEM (PLAIN TX) ───────────────────────────────────────────────────

/// Kullanıcının şifrelemeden önce oluşturduğu düz TX yapısı.
///
/// Bu yapı borsh ile serialize edilip şifrelenerek [`EncryptedTx`] haline gelir.
/// §5.2.2 — "Kullanıcı işlemi oluşturur: TX = {gönderen, alıcı, miktar, veri}"
#[derive(
    Debug, Clone,
    Serialize, Deserialize,
    BorshSerialize, BorshDeserialize,
)]
pub struct RawTransaction {
    /// Gönderen adres (MPC public key'den türetilir). §5.1
    pub from: Address,

    /// Alıcı adres. `None` = kontrat deploy.
    pub to: Option<Address>,

    /// Transfer miktarı (NSC base unit = 10^-18 NSC).
    pub value: Balance,

    /// Hesap nonce'u — replay koruması. §5.1.3
    pub nonce: Nonce,

    /// İşlem tipi.
    pub tx_type: TxType,

    /// Gas limiti (compute unit).
    pub gas_limit: u64,

    /// Maksimum gas fiyatı (NSC base unit / gas).
    pub max_fee_per_gas: Balance,

    /// Öncelik fee'si (validator'a doğrudan giden kısım).
    pub max_priority_fee_per_gas: Balance,

    /// İşlem verisi (kontrat calldata veya özel protokol verisi).
    pub data: Vec<u8>,

    /// Chain ID — replay saldırısı koruması. §5.12
    pub chain_id: u64,
}

impl RawTransaction {
    /// Yeni bir transfer TX oluştur.
    pub fn new_transfer(
        from: Address,
        to: Address,
        value: Balance,
        nonce: Nonce,
        chain_id: u64,
    ) -> Self {
        Self {
            from,
            to: Some(to),
            value,
            nonce,
            tx_type: TxType::Transfer,
            gas_limit: 21_000,
            max_fee_per_gas: Balance(1_000_000_000), // 1 Gwei
            max_priority_fee_per_gas: Balance(100_000_000), // 0.1 Gwei
            data: Vec::new(),
            chain_id,
        }
    }

    /// Heartbeat TX oluştur — ücretsiz, deadman switch sıfırlar. §5.4.9
    pub fn new_heartbeat(from: Address, nonce: Nonce, chain_id: u64) -> Self {
        Self {
            from,
            to: None,
            value: Balance::ZERO,
            nonce,
            tx_type: TxType::Heartbeat,
            gas_limit: 0,
            max_fee_per_gas: Balance::ZERO,
            max_priority_fee_per_gas: Balance::ZERO,
            data: Vec::new(),
            chain_id,
        }
    }

    /// Tahmini gas maliyeti (base fee hariç).
    pub fn estimated_gas_cost(&self) -> Balance {
        let gas = self.gas_limit as u128;
        let price = self.max_fee_per_gas.0;
        Balance(gas.saturating_mul(price))
    }

    /// Borsh ile byte'lara dönüştür.
    pub fn to_bytes(&self) -> Result<Vec<u8>, borsh::io::Error> {
        borsh::to_vec(self)
    }

    /// Byte'lardan deserialize et.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }
}

// ─── ŞİFRELİ İŞLEM ───────────────────────────────────────────────────────────

/// Kullanıcının threshold IBE ile şifrelenmiş TX'i. §5.2.2
///
/// Mempool bu yapıyı depolar. Validator'lar commit fazında yalnızca
/// `commitment` ve `sender_address` alanlarını görür; TX içeriğini göremez.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct EncryptedTx {
    /// TX kriptografik kimliği: BLAKE3(E || aes_cipher || aes_nonce || slot_le_bytes). §5.2.2
    pub commitment: TxHash,

    /// Ephemeral pubkey E = e*G (Ristretto255, 32 byte). §6.1.4
    /// Validators bu ile kendi sk_share * E'yi hesaplar.
    pub ephemeral_pubkey: [u8; 32],

    /// AES-256-GCM ile şifrelenmiş TX içeriği (512 byte sabit — padded). §6.4.2
    pub aes_ciphertext: Vec<u8>,

    /// AES-GCM nonce (12 byte).
    pub aes_nonce: [u8; 12],

    /// Bu TX'in geçerli olduğu slot numarası. §5.2.2
    pub target_slot: SlotId,

    /// Gönderici adresi (şifresiz — mempool'un nonce sırasını takip etmesi için).
    pub sender: Address,

    /// Commitment üzerine göndericinin MPC imzası. §5.1.3
    /// `sign(commitment.as_bytes(), sender_mpc_key)`
    pub signature: Vec<u8>,

    /// TX boyutu bilgisi (debugging/metrics).
    pub plaintext_size_hint: u32,
}

impl EncryptedTx {
    /// Şifreli içerik boyutunun güvenlik sınırı içinde olup olmadığını kontrol et. §6.4.2
    pub fn check_size(&self) -> bool {
        self.aes_ciphertext.len() <= MAX_ENCRYPTED_TX_BYTES
    }

    /// Padding sonrası beklenen boyut: MAX_ENCRYPTION_PADDING_BYTES + AES-GCM overhead (16 byte tag).
    pub fn expected_padded_size() -> usize {
        MAX_ENCRYPTION_PADDING_BYTES + 16 // AES-GCM tag
    }
}

// ─── AÇILMIŞ İŞLEM ───────────────────────────────────────────────────────────

/// Threshold decryption sonrası elde edilen açık TX. §5.2.2
///
/// Bu yapı batch auction'a gider. Herkes tarafından görülebilir
/// (blok kapandıktan sonra) — zincire yazılır.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DecryptedTx {
    /// Orijinal commitment hash (eşleşme doğrulaması için).
    pub commitment: TxHash,

    /// Açık TX içeriği.
    pub raw_tx: RawTransaction,

    /// Hangi slot'ta açıldı.
    pub revealed_in_slot: SlotId,

    /// Batch içindeki sıra (commitment hash'e göre lexicographic sort). §5.2.2
    pub ordering_index: u32,
}

// ─── MEMPOOL GİRİŞİ ──────────────────────────────────────────────────────────

/// Mempool havuzunda saklanan kayıt.
///
/// Her şifreli TX için bir `MempoolEntry` tutulur.
/// Threshold reveal sonrası `decrypted` alanı doldurulur.
#[derive(Debug)]
pub struct MempoolEntry {
    /// Şifreli TX verisi.
    pub encrypted: EncryptedTx,

    /// Açılma sonrası (None = henüz açılmadı).
    pub decrypted: Option<DecryptedTx>,

    /// TX havuza ne zaman eklendi (yerel saat).
    pub received_at: Instant,

    /// TX durumu.
    pub status: TxStatus,

    /// Toplanan decryption share sayısı.
    pub collected_shares: u8,
}

impl MempoolEntry {
    /// Yeni giriş oluştur.
    pub fn new(encrypted: EncryptedTx) -> Self {
        Self {
            encrypted,
            decrypted: None,
            received_at: Instant::now(),
            status: TxStatus::Pending,
            collected_shares: 0,
        }
    }

    /// Açılmış mı?
    pub fn is_revealed(&self) -> bool {
        self.decrypted.is_some()
    }
}

// ─── TX DURUMU ───────────────────────────────────────────────────────────────

/// Bir TX'in yaşam döngüsü boyunca aldığı durum. §5.2.2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxStatus {
    /// Şifreli, decryption bekleniyor.
    Pending,
    /// Yeterli decryption share toplandı, açılma başlıyor.
    Revealing,
    /// Threshold decryption başarılı, TX açıldı.
    Revealed,
    /// Batch auction'da işlendi (execute edildi).
    Executed,
    /// Hata ile başarısız oldu.
    Failed(TxFailureReason),
    /// Slot geçti, havuzdan temizlendi.
    Expired,
}

/// TX başarısızlık nedeni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxFailureReason {
    /// Commitment doğrulama başarısız.
    CommitmentMismatch,
    /// Şifre çözme başarısız.
    DecryptionFailed,
    /// Geçersiz TX içeriği (deserialize hatası).
    InvalidContent,
    /// Yetersiz bakiye.
    InsufficientBalance,
    /// Nonce uyuşmazlığı.
    NonceMismatch,
    /// Gas limiti aşıldı.
    GasLimitExceeded,
}

// ─── TX DECRYPTION SHARE ─────────────────────────────────────────────────────

/// Bir validator'ın tek bir TX için hesapladığı decryption share. §6.1.4
///
/// D_i = sk_share_i * E  (Ristretto255 noktası, 32 byte)
///
/// t adet share Lagrange interpolasyonuyla birleştirilince
/// ECDH shared secret S = sk_threshold * E elde edilir.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct TxDecryptionShare {
    /// Hangi TX için (commitment hash = TX hash). §5.2.2
    pub tx_hash: TxHash,

    /// Hangi validator'dan (1-indexed, n'e kadar).
    pub validator_index: u8,

    /// D_i = sk_share_i * E  (Ristretto255 compressed, 32 byte).
    pub d_i: [u8; 32],

    /// Hangi slot için.
    pub slot: SlotId,
}

// ─── SLOT BATCH ──────────────────────────────────────────────────────────────

/// Tek bir slot için bekleyen şifreli TX koleksiyonu. §5.2.2 + §5.3
///
/// Commit fazı (300ms) boyunca TX'ler buraya eklenir.
/// Reveal fazı (100ms) başlayınca toplu açılır.
#[derive(Debug)]
pub struct SlotBatch {
    /// Slot numarası.
    pub slot: SlotId,

    /// Şifreli TX'ler (commitment hash sırasıyla). §5.2.2
    /// Bu sıralama MEV'i engeller — bot sıralamayı manipüle edemez.
    pub transactions: Vec<EncryptedTx>,

    /// Bu slot için toplanan decryption share'ler.
    /// Dış yapı: tx_hash → [share_index → TxDecryptionShare]
    pub decryption_shares: std::collections::HashMap<TxHash, Vec<TxDecryptionShare>>,

    /// Commit fazı kapandı mı?
    pub commit_phase_closed: bool,

    /// Reveal fazı başladı mı?
    pub reveal_phase_started: bool,
}

impl SlotBatch {
    /// Boş slot batch oluştur.
    pub fn new(slot: SlotId) -> Self {
        Self {
            slot,
            transactions: Vec::new(),
            decryption_shares: std::collections::HashMap::new(),
            commit_phase_closed: false,
            reveal_phase_started: false,
        }
    }

    /// TX sayısı.
    pub fn tx_count(&self) -> usize {
        self.transactions.len()
    }

    /// Bu slot için belirli bir TX'in decryption share sayısı.
    pub fn share_count(&self, tx_hash: &TxHash) -> usize {
        self.decryption_shares
            .get(tx_hash)
            .map_or(0, |v| v.len())
    }

    /// Decryption share ekle.
    pub fn add_share(&mut self, share: TxDecryptionShare) {
        let entry = self.decryption_shares.entry(share.tx_hash).or_default();
        // Aynı validator'dan ikinci share gelirse ignore et
        if !entry.iter().any(|s| s.validator_index == share.validator_index) {
            entry.push(share);
        }
    }

    /// Tüm TX'ler için yeterli share var mı?
    pub fn can_reveal(&self, threshold: u8) -> bool {
        self.transactions.iter().all(|tx| {
            let count = self.share_count(&tx.commitment);
            count >= threshold as usize
        })
    }
}

// ─── AÇILMIŞ BATCH ───────────────────────────────────────────────────────────

/// Threshold decryption sonrası açılan TX batch'i. §5.3
///
/// Bu yapı batch auction execution motoruna (nova-auction) geçer.
/// Sıralama commitment hash'e göre yapılmış — MEV-free. §5.2.2
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct RevealedBatch {
    /// Slot numarası.
    pub slot: SlotId,

    /// Açılmış TX'ler (commitment hash sıralı — MEV-free). §5.2.2
    pub transactions: Vec<DecryptedTx>,

    /// Açılamayan TX hash'leri (decrypt başarısız).
    pub failed_hashes: Vec<TxHash>,

    /// Bu batch'in Merkle root'u (blok header'a yazılır).
    pub batch_merkle_root: [u8; 32],
}

impl RevealedBatch {
    /// Başarılı TX sayısı.
    pub fn success_count(&self) -> usize {
        self.transactions.len()
    }

    /// Başarısız TX sayısı.
    pub fn failure_count(&self) -> usize {
        self.failed_hashes.len()
    }

    /// Toplam TX sayısı.
    pub fn total_count(&self) -> usize {
        self.success_count() + self.failure_count()
    }

    /// Boş batch mı?
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty() && self.failed_hashes.is_empty()
    }
}

// ─── MEMPOOL KONFİGÜRASYONU ─────────────────────────────────────────────────

/// Mempool yapılandırması.
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maksimum toplam TX kapasitesi.
    pub max_total_txs: usize,

    /// Slot başına maksimum TX. §5.9.1 (MAX_TX_PER_SLOT = 10_000)
    pub max_txs_per_slot: usize,

    /// Bir TX için maksimum boyut (byte). §6.4.2
    pub max_tx_bytes: usize,

    /// Threshold parametresi (2n/3 + 1). §6.1.4
    pub decryption_threshold: u8,

    /// Kaç slot geçmişe kadar TX kabul edilir?
    pub max_slot_lag: u64,

    /// Kaç slot ileriden TX kabul edilir?
    pub max_slot_advance: u64,

    /// Minimum gas fiyatı.
    pub min_gas_price: Balance,

    /// Chain ID (replay koruması).
    pub chain_id: u64,

    /// Gönderici ECDSA imzasını doğrula (NOVA-RED-MEMPOOL-001).
    /// Üretimde `true`, testlerde `false` set edilebilir.
    pub verify_sender_signature: bool,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        use nova_primitives::constants::MAX_TX_PER_SLOT;
        Self {
            max_total_txs: 100_000,
            max_txs_per_slot: MAX_TX_PER_SLOT as usize,
            max_tx_bytes: MAX_ENCRYPTED_TX_BYTES,
            decryption_threshold: 134, // ceil(200 * 2/3) + 1 = 134
            max_slot_lag: 2,
            max_slot_advance: 5,
            min_gas_price: Balance(1_000_000), // 0.001 Gwei
            chain_id: nova_primitives::constants::MAINNET_CHAIN_ID,
            verify_sender_signature: true,
        }
    }
}

// ─── MEMPOOL İSTATİSTİKLERİ ──────────────────────────────────────────────────

/// Mempool anlık istatistikleri.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MempoolStats {
    /// Toplam bekleyen TX.
    pub pending_count: usize,

    /// Toplam açılmış TX.
    pub revealed_count: usize,

    /// Toplam başarısız TX.
    pub failed_count: usize,

    /// Toplam expire edilmiş TX.
    pub expired_count: usize,

    /// Havuzda aktif olan slot sayısı.
    pub active_slots: usize,

    /// Toplam kabul edilen TX (ömür boyu).
    pub total_submitted: u64,

    /// Toplam açılan TX (ömür boyu).
    pub total_revealed: u64,
}

impl MempoolStats {
    /// Kullanım oranı (%0-100).
    pub fn usage_pct(&self, max_txs: usize) -> f64 {
        if max_txs == 0 {
            return 100.0;
        }
        (self.pending_count as f64 / max_txs as f64) * 100.0
    }
}

// ─── GEÇİCİ AES ANAHTARI ─────────────────────────────────────────────────────

/// Şifreleme sırasında kullanılan geçici AES anahtarı (heap'te sıfırlanır).
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct EphemeralAesKey(pub [u8; 32]);

impl EphemeralAesKey {
    /// Sıfırdan oluştur.
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};

    fn test_address(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    #[test]
    fn test_tx_hash_display() {
        let hash = TxHash::from_bytes([0xde; 32]);
        let s = hash.to_string();
        assert!(s.starts_with("0x"), "0x prefix olmalı");
        assert_eq!(s.len(), 66, "0x + 64 hex karakter");
    }

    #[test]
    fn test_tx_hash_zero() {
        let z = TxHash::ZERO;
        assert_eq!(z.to_hex(), "0".repeat(64));
    }

    #[test]
    fn test_raw_transaction_transfer() {
        let from = test_address(0x01);
        let to = test_address(0x02);
        let tx = RawTransaction::new_transfer(
            from,
            to,
            Balance(1_000_000_000_000_000_000u128), // 1 NSC
            Nonce(0),
            1984,
        );
        assert_eq!(tx.tx_type, TxType::Transfer);
        assert!(!tx.tx_type.is_fee_exempt(), "Transfer fee muafiyeti yok");
        assert_eq!(tx.chain_id, 1984);
    }

    #[test]
    fn test_heartbeat_tx() {
        let from = test_address(0x03);
        let tx = RawTransaction::new_heartbeat(from, Nonce(5), 1984);
        assert_eq!(tx.tx_type, TxType::Heartbeat);
        assert!(tx.tx_type.is_fee_exempt(), "Heartbeat fee muafiyetli olmalı");
        assert_eq!(tx.value, Balance::ZERO);
        assert_eq!(tx.gas_limit, 0);
    }

    #[test]
    fn test_raw_transaction_borsh_roundtrip() {
        let from = test_address(0x11);
        let to = test_address(0x22);
        let tx = RawTransaction::new_transfer(
            from,
            to,
            Balance(500_000_000_000_000_000u128), // 0.5 NSC
            Nonce(7),
            1984,
        );
        let bytes = tx.to_bytes().expect("serialize edilmeli");
        let restored = RawTransaction::from_bytes(&bytes).expect("deserialize edilmeli");
        assert_eq!(restored.from, tx.from);
        assert_eq!(restored.to, tx.to);
        assert_eq!(restored.value, tx.value);
        assert_eq!(restored.nonce, tx.nonce);
        assert_eq!(restored.chain_id, tx.chain_id);
    }

    #[test]
    fn test_raw_transaction_estimated_gas() {
        let from = test_address(0x01);
        let to = test_address(0x02);
        let tx = RawTransaction {
            from,
            to: Some(to),
            value: Balance::ZERO,
            nonce: Nonce(0),
            tx_type: TxType::Transfer,
            gas_limit: 21_000,
            max_fee_per_gas: Balance(1_000_000_000u128), // 1 Gwei
            max_priority_fee_per_gas: Balance::ZERO,
            data: Vec::new(),
            chain_id: 1984,
        };
        let cost = tx.estimated_gas_cost();
        assert_eq!(cost, Balance(21_000 * 1_000_000_000u128));
    }

    #[test]
    fn test_tx_type_variants() {
        assert!(!TxType::Transfer.is_fee_exempt());
        assert!(!TxType::ContractCall.is_fee_exempt());
        assert!(TxType::Heartbeat.is_fee_exempt());
        assert!(TxType::GuardianOp.is_guardian_op());
        assert!(!TxType::Transfer.is_guardian_op());
    }

    #[test]
    fn test_tx_status_equality() {
        assert_eq!(TxStatus::Pending, TxStatus::Pending);
        assert_eq!(TxStatus::Revealed, TxStatus::Revealed);
        assert_ne!(TxStatus::Pending, TxStatus::Revealed);
    }

    #[test]
    fn test_slot_batch_new() {
        let batch = SlotBatch::new(SlotId(42));
        assert_eq!(batch.slot, SlotId(42));
        assert_eq!(batch.tx_count(), 0);
        assert!(!batch.commit_phase_closed);
        assert!(!batch.reveal_phase_started);
    }

    #[test]
    fn test_slot_batch_can_reveal_empty() {
        let batch = SlotBatch::new(SlotId(1));
        // Boş batch — tüm TX'ler yeterli share'e sahip (0 TX = hepsi hazır)
        assert!(batch.can_reveal(3), "Boş batch her zaman reveal edilebilir");
    }

    #[test]
    fn test_slot_batch_share_deduplication() {
        let mut batch = SlotBatch::new(SlotId(10));
        let tx_hash = TxHash::from_bytes([0xAA; 32]);

        let share1 = TxDecryptionShare {
            tx_hash,
            validator_index: 1,
            d_i: [0x01; 32],
            slot: SlotId(10),
        };
        let share1_dup = TxDecryptionShare {
            tx_hash,
            validator_index: 1, // Aynı validator
            d_i: [0x99; 32],    // Farklı değer (sahte)
            slot: SlotId(10),
        };
        let share2 = TxDecryptionShare {
            tx_hash,
            validator_index: 2,
            d_i: [0x02; 32],
            slot: SlotId(10),
        };

        batch.add_share(share1);
        batch.add_share(share1_dup); // Yinelenen — ignore edilmeli
        batch.add_share(share2);

        assert_eq!(
            batch.share_count(&tx_hash), 2,
            "Yinelenen share sayılmamalı: 2 farklı validator var"
        );
    }

    #[test]
    fn test_mempool_config_default() {
        let config = MempoolConfig::default();
        assert_eq!(config.max_txs_per_slot, 10_000);
        assert_eq!(config.chain_id, 1984);
        assert!(config.decryption_threshold > 0);
    }

    #[test]
    fn test_mempool_stats_usage_pct() {
        let mut stats = MempoolStats::default();
        stats.pending_count = 5_000;
        let pct = stats.usage_pct(10_000);
        assert!((pct - 50.0).abs() < 0.001, "5000/10000 = %50 olmalı");
    }

    #[test]
    fn test_mempool_stats_full() {
        let mut stats = MempoolStats::default();
        stats.pending_count = 10_000;
        let pct = stats.usage_pct(10_000);
        assert!((pct - 100.0).abs() < 0.001, "Tam dolu = %100");
    }

    #[test]
    fn test_revealed_batch_counts() {
        let batch = RevealedBatch {
            slot: SlotId(5),
            transactions: vec![],
            failed_hashes: vec![TxHash::ZERO],
            batch_merkle_root: [0u8; 32],
        };
        assert_eq!(batch.success_count(), 0);
        assert_eq!(batch.failure_count(), 1);
        assert_eq!(batch.total_count(), 1);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_ephemeral_aes_key_zeroize() {
        let key = EphemeralAesKey([0xFF; 32]);
        drop(key); // ZeroizeOnDrop devreye girer
        // Sadece drop etmek yeterli (bellek sanitizer ile kontrol edilir)
    }

    #[test]
    fn test_encrypted_tx_size_check() {
        let tx = EncryptedTx {
            commitment: TxHash::ZERO,
            ephemeral_pubkey: [0u8; 32],
            aes_ciphertext: vec![0u8; 500], // Geçerli boyut
            aes_nonce: [0u8; 12],
            target_slot: SlotId(1),
            sender: test_address(0x01),
            signature: vec![0u8; 64],
            plaintext_size_hint: 100,
        };
        assert!(tx.check_size(), "500 byte şifreli içerik geçerli olmalı");
    }

    #[test]
    fn test_encrypted_tx_size_check_too_large() {
        let tx = EncryptedTx {
            commitment: TxHash::ZERO,
            ephemeral_pubkey: [0u8; 32],
            aes_ciphertext: vec![0u8; 5000], // MAX_ENCRYPTED_TX_BYTES = 4096
            aes_nonce: [0u8; 12],
            target_slot: SlotId(1),
            sender: test_address(0x01),
            signature: vec![0u8; 64],
            plaintext_size_hint: 100,
        };
        assert!(!tx.check_size(), "5000 byte geçersiz olmalı");
    }

    #[test]
    fn test_tx_hash_ordering() {
        let h1 = TxHash::from_bytes([0x00; 32]);
        let h2 = TxHash::from_bytes([0xFF; 32]);
        let h3 = TxHash::from_bytes([0x80; 32]);

        assert!(h1 < h2, "0x00... < 0xFF...");
        assert!(h3 < h2, "0x80... < 0xFF...");
        assert!(h1 < h3, "0x00... < 0x80...");
    }

    #[test]
    fn test_mempool_entry_new() {
        let tx = EncryptedTx {
            commitment: TxHash::ZERO,
            ephemeral_pubkey: [0u8; 32],
            aes_ciphertext: vec![],
            aes_nonce: [0u8; 12],
            target_slot: SlotId(1),
            sender: test_address(0x01),
            signature: vec![],
            plaintext_size_hint: 0,
        };
        let entry = MempoolEntry::new(tx);
        assert_eq!(entry.status, TxStatus::Pending);
        assert!(!entry.is_revealed());
        assert_eq!(entry.collected_shares, 0);
    }
}
