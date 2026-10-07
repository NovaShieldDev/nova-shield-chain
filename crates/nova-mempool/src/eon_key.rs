//! Eon Key Yönetimi — Rolling Shutter Keyper Protocol
//!
//! ## Araştırma Kaynağı: Rolling Shutter
//!
//! `research/libs/rolling-shutter/shuttermint/keyper/`:
//!
//! Rolling Shutter'da "Eon" (dönem), belirli bir blok aralığını kapsar.
//! Her eon'un kendi "Eon Public Key"i (EPK) vardır — threshold IBE şifreleme için kullanılır.
//!
//! Rolling Shutter Keyper protokolü:
//! 1. `EonKeyPublisher`: Eon public key'i broadcast eder
//! 2. `KeyperDecryptionKey`: Her keyper kendi decryption share'ini oluşturur
//! 3. Threshold sayıda share toplandığında tam decryption key elde edilir
//!
//! ```text
//! Rolling Shutter Go kodu referansları:
//!
//! // shuttermint/keyper/keyper.go:
//! type Keyper struct {
//!     Config       KeyperConfig
//!     EonKeyBroadcast chan *epochkg.EonPublicKey
//! }
//!
//! // epochkg/epochkg.go:
//! type EpochSecretKeyShare struct {
//!     Eon         uint64
//!     Epoch       *shcrypto.EpochID
//!     Share       *shcrypto.EpochSecretKeyShare
//! }
//!
//! // shcrypto/crypto.go:
//! type EonPublicKey struct {
//!     p *bls.G2
//! }
//! func (ek *EonPublicKey) GobEncode() ([]byte, error)
//! ```
//!
//! ## Nova Uyarlaması
//!
//! Rolling Shutter'da Boneh-Franklin IBE kullanılır; Nova'da ECDH-hybrid kullanılır.
//! Eon key kavramı aynı: her epoch'ta yeni threshold public key yayınlanır.
//!
//! - `EonId` → Epoch numarası (Rolling Shutter Eon analog)
//! - `EonPublicKey` → Epoch threshold public key (secp256k1, 33 byte)
//! - `BatchIndex` → Slot gruplandırması (Rolling Shutter BatchIndex)
//! - `KeyperDecryptionShare` → Bir validator'ın threshold decryption share'i
//! - `EonKeyPublisher` → Epoch public key yayınlama mekanizması
//! - `DecryptionKeyAggregator` → Threshold share'leri birleştirici

use nova_primitives::types::{EpochId, SlotId};
use serde::{Deserialize, Serialize};

// ─── SERDE YARDIMCIları (32 BYTE DİZİLER — Ristretto255) ─────────────────────
//
// BUG-9 düzeltmesi: Eon key ve decryption share'ler secp256k1 (33 byte) yerine
// Ristretto255 (32 byte) kullanmalı. IBE encrypt/decrypt curve25519_dalek ile
// Ristretto255 üzerinde çalışır; 33 byte secp256k1 ile uyumsuzdu.

mod serde_bytes_32 {
    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(val: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        val.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let v: Vec<u8> = Vec::deserialize(d)?;
        v.try_into().map_err(|v: Vec<u8>| {
            de::Error::invalid_length(v.len(), &"tam olarak 32 byte gerekli")
        })
    }
}

// ─── EON ID ──────────────────────────────────────────────────────────────────

/// Eon (dönem) kimliği — Rolling Shutter Eon analog.
///
/// Rolling Shutter Go: `uint64` Eon numarası.
/// Nova'da EpochId ile örtüşür — her epoch = 1 eon.
pub type EonId = EpochId;

// ─── BATCH INDEX (ROLLING SHUTTER BatchIndex ANALOG) ──────────────────────────

/// Batch kimliği — Rolling Shutter `BatchIndex` analog.
///
/// Rolling Shutter (`shuttermint/shcrypto/shcrypto.go`):
/// ```text
/// type EpochID struct {
///     Bytes []byte  // batch/epoch identifier
/// }
/// func ComputeEpochID(batchIndex uint64) *EpochID
/// ```
///
/// Nova'da her slot bir batch'tir. BatchIndex = (eon_id, slot_id) çifti.
/// Bu çift, şifreleme için domain separator olarak kullanılır:
/// - Aynı TX içeriği, farklı slot'ta farklı şifrelenmiş görünür
/// - Replay saldırısı imkansız
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct BatchIndex {
    /// Bu batch'in ait olduğu eon (epoch).
    pub eon_id: EonId,
    /// Slot numarası (batch = 1 slot).
    pub slot_id: SlotId,
}

impl BatchIndex {
    /// Yeni BatchIndex oluştur.
    pub fn new(eon_id: EonId, slot_id: SlotId) -> Self {
        Self { eon_id, slot_id }
    }

    /// BatchIndex'i 16 byte deterministic encoding'e dönüştür.
    /// Rolling Shutter: `ComputeEpochID(batchIndex)` analog.
    /// Domain separator olarak IBE şifrelemede kullanılır.
    pub fn to_epoch_id_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.eon_id.0.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.slot_id.0.to_le_bytes());
        bytes
    }
}

impl std::fmt::Display for BatchIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Batch(eon={}, slot={})", self.eon_id.0, self.slot_id.0)
    }
}

// ─── EON PUBLIC KEY (ROLLING SHUTTER EonPublicKey ANALOG) ─────────────────────

/// Epoch (Eon) threshold public key — Rolling Shutter `EonPublicKey` analog.
///
/// Rolling Shutter Go (`shcrypto/shcrypto.go`):
/// ```text
/// type EonPublicKey struct {
///     p *bls.G2  // BLS12-381 G2 nokta
/// }
/// ```
///
/// Rolling Shutter'da BLS12-381 G2 kullanılır (Boneh-Franklin IBE için).
/// Nova'da Ristretto255 compressed pubkey kullanılır (ECDH hybrid IBE şema).
///
/// ## BUG-9 DÜZELTMESİ
/// Eski implementasyon secp256k1 33-byte kullanıyordu; ama IBE encrypt/decrypt
/// curve25519_dalek ile Ristretto255 32-byte noktalar üzerinde çalışır.
/// `nova-mempool/src/ibe/decrypt.rs` Ristretto255 bekler — tip uyumsuzluğu giderildi.
///
/// Güvenlik: Eon public key zincirimizin "threshold_pk"si.
/// Tüm mempool şifreleme bu key ile yapılır.
/// Eon sonunda (epoch döndüğünde) yeni key üretilir.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EonPublicKey {
    /// Eon numarası.
    pub eon_id: EonId,
    /// Threshold public key (Ristretto255 compressed, 32 byte).
    /// IBE encrypt/decrypt ile uyumlu — curve25519_dalek::RistrettoPoint.compress()
    #[serde(with = "serde_bytes_32")]
    pub key_bytes: [u8; 32],
    /// Bu key'in geçerli olduğu slot aralığı.
    pub valid_from_slot: SlotId,
    pub valid_until_slot: SlotId,
    /// Kaç validator bu key'i imzaladı (threshold doğrulama).
    pub threshold_confirmations: u32,
    /// Bu eon'un threshold parametresi (t).
    pub threshold_t: u32,
    /// Bu eon'un toplam keyper sayısı (n).
    pub keyper_count: u32,
}

impl EonPublicKey {
    /// Yeni EonPublicKey oluştur.
    /// `key_bytes`: Ristretto255 compressed public key (32 byte)
    pub fn new(
        eon_id: EonId,
        key_bytes: [u8; 32],
        valid_from_slot: SlotId,
        valid_until_slot: SlotId,
        threshold_t: u32,
        keyper_count: u32,
    ) -> Self {
        Self {
            eon_id,
            key_bytes,
            valid_from_slot,
            valid_until_slot,
            threshold_confirmations: 0,
            threshold_t,
            keyper_count,
        }
    }

    /// Bu slot için geçerli mi?
    pub fn is_valid_for_slot(&self, slot: SlotId) -> bool {
        slot.0 >= self.valid_from_slot.0 && slot.0 < self.valid_until_slot.0
    }

    /// Threshold doğrulandı mı? (t onay alındı mı?)
    pub fn is_threshold_confirmed(&self) -> bool {
        self.threshold_confirmations >= self.threshold_t
    }

    /// Onay ekle.
    pub fn add_confirmation(&mut self) {
        self.threshold_confirmations = self.threshold_confirmations.saturating_add(1);
    }
}

// ─── KEYPER DECRYPTION SHARE (ROLLING SHUTTER EpochSecretKeyShare ANALOG) ──────

/// Bir keyper/validator'ın decryption key share'i.
///
/// Rolling Shutter Go (`epochkg/epochkg.go`):
/// ```text
/// type EpochSecretKeyShare struct {
///     Eon   uint64
///     Epoch *shcrypto.EpochID
///     Share *shcrypto.EpochSecretKeyShare
/// }
/// ```
///
/// Nova'da: Her validator `D_i = sk_share_i * E` hesaplar.
/// (E = şifreli TX'in ephemeral Ristretto255 public key'i, 32 byte)
/// t adet D_i Lagrange interpolasyonuyla birleştirilir → S = sk_threshold * E
///
/// ## BUG-9 DÜZELTMESİ
/// `share_point` eski implementasyonda secp256k1 33-byte'tı.
/// D_i = sk_i * E bir Ristretto255 noktasıdır (32 byte); düzeltildi.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyperDecryptionShare {
    /// Hangi eon için decryption share.
    pub eon_id: EonId,
    /// Hangi slot/batch için decryption share.
    pub batch: BatchIndex,
    /// Bu share'i üreten validator'ın indeksi.
    pub validator_index: u32,
    /// Decryption share: D_i = sk_share_i * E (Ristretto255 point, 32 byte).
    #[serde(with = "serde_bytes_32")]
    pub share_point: [u8; 32],
    /// Lagrange katsayısı (hesaplama için validator indeksi kullanılır).
    pub lagrange_index: u32,
}

impl KeyperDecryptionShare {
    /// Yeni decryption share oluştur.
    /// `share_point`: D_i = sk_share_i * E (Ristretto255 compressed, 32 byte)
    pub fn new(
        eon_id: EonId,
        batch: BatchIndex,
        validator_index: u32,
        share_point: [u8; 32],
    ) -> Self {
        Self {
            eon_id,
            batch,
            validator_index,
            share_point,
            lagrange_index: validator_index,
        }
    }
}

// ─── EON KEY PUBLISHER (ROLLING SHUTTER EonKeyPublisher ANALOG) ───────────────

/// Eon public key yayıncısı — Rolling Shutter `EonKeyBroadcast` analog.
///
/// Rolling Shutter'da her eon başında keyper'lar yeni EPK üretip broadcast eder.
/// Nova'da: Epoch başında validator seti yeni threshold public key üretir (MPC DKG).
///
/// Görevleri:
/// 1. Mevcut epoch'un EonPublicKey'ini sakla
/// 2. Epoch dönüşümünde yeni key için onayları topla
/// 3. Threshold onay sayısına ulaşınca key'i "aktif" ilan et
/// 4. Mempool'un şifrelemede kullanacağı geçerli EonPublicKey'i ver
#[derive(Debug)]
pub struct EonKeyPublisher {
    /// Mevcut aktif EonPublicKey.
    current_key: Option<EonPublicKey>,
    /// Onay bekleyen aday key.
    pending_key: Option<EonPublicKey>,
    /// İşlenmiş onaylar (validator_index → bool).
    pending_confirmations: std::collections::HashSet<u32>,
    /// Slots per epoch (kaç slot = 1 eon).
    slots_per_epoch: u64,
}

impl EonKeyPublisher {
    /// Yeni EonKeyPublisher.
    pub fn new(slots_per_epoch: u64) -> Self {
        Self {
            current_key: None,
            pending_key: None,
            pending_confirmations: std::collections::HashSet::new(),
            slots_per_epoch,
        }
    }

    /// Belirli bir slot için geçerli EonPublicKey'i döndür.
    ///
    /// Rolling Shutter: `GetCurrentEonPublicKey(blockNumber)` analog.
    pub fn get_active_key(&self, slot: SlotId) -> Option<&EonPublicKey> {
        if let Some(key) = &self.current_key {
            if key.is_valid_for_slot(slot) {
                return Some(key);
            }
        }
        None
    }

    /// Yeni aday EonPublicKey öner (epoch başında).
    ///
    /// Rolling Shutter: `ProposeEonKey` analog.
    pub fn propose_new_key(
        &mut self,
        eon_id: EonId,
        key_bytes: [u8; 32],
        valid_from_slot: SlotId,
        threshold_t: u32,
        keyper_count: u32,
    ) {
        let valid_until = SlotId(valid_from_slot.0 + self.slots_per_epoch);
        let new_key = EonPublicKey::new(
            eon_id, key_bytes, valid_from_slot, valid_until, threshold_t, keyper_count,
        );
        self.pending_key = Some(new_key);
        self.pending_confirmations.clear();
    }

    /// Validator onayı ekle.
    ///
    /// Rolling Shutter: `KeyperConfirmEonKey` analog.
    /// Threshold onay sayısına ulaşıldığında key aktive edilir.
    pub fn add_confirmation(&mut self, validator_index: u32) -> bool {
        if self.pending_key.is_none() {
            return false;
        }
        self.pending_confirmations.insert(validator_index);

        let threshold = self.pending_key.as_ref().map(|k| k.threshold_t).unwrap_or(0);
        let confirmed = self.pending_confirmations.len() as u32 >= threshold;

        if confirmed {
            if let Some(mut key) = self.pending_key.take() {
                key.threshold_confirmations = self.pending_confirmations.len() as u32;
                self.current_key = Some(key);
                self.pending_confirmations.clear();
            }
            true
        } else {
            false
        }
    }

    /// Bekleyen aday key var mı?
    pub fn has_pending_key(&self) -> bool {
        self.pending_key.is_some()
    }

    /// Mevcut onay sayısı.
    pub fn pending_confirmation_count(&self) -> usize {
        self.pending_confirmations.len()
    }

    /// Aktif key var mı?
    pub fn has_active_key(&self) -> bool {
        self.current_key.is_some()
    }
}

// ─── DECRYPTION KEY AGGREGATOR ────────────────────────────────────────────────

/// Threshold decryption share toplayıcı.
///
/// Rolling Shutter Go (`keyper/keyper.go`):
/// ```text
/// type DecryptionTrigger struct {
///     BlockNumber uint64
///     Shares      []*epochkg.EpochSecretKeyShare
/// }
/// ```
///
/// Nova: t adet share toplandığında Lagrange interpolasyonu ile
/// tam shared secret S elde edilir.
///
/// `S = sum_i( lambda_i * D_i )` (secp256k1 nokta toplamı + Lagrange ağırlık)
/// Bu S, AES decryption için kullanılır.
#[derive(Debug)]
pub struct DecryptionKeyAggregator {
    /// Hangi batch için share toplanıyor.
    pub batch: BatchIndex,
    /// Toplanan share'ler (validator_index → share_point).
    shares: std::collections::HashMap<u32, [u8; 32]>,
    /// Threshold (kaç share gerekli).
    threshold: u32,
    /// Toplam validator sayısı (Lagrange için).
    total_validators: u32,
}

impl DecryptionKeyAggregator {
    /// Yeni aggregator.
    pub fn new(batch: BatchIndex, threshold: u32, total_validators: u32) -> Self {
        Self {
            batch,
            shares: std::collections::HashMap::new(),
            threshold,
            total_validators,
        }
    }

    /// Share ekle.
    ///
    /// Rolling Shutter: `AppendDecryptionShare` analog.
    ///
    /// BUG-3 FIX: Threshold dolunca yeni share kabul etme.
    /// Fazla share Lagrange interpolasyonuna girerse geçersiz/sahte share
    /// sonucu bozabilir. Tam olarak `threshold` share ile aggregate edilmeli.
    pub fn add_share(&mut self, share: KeyperDecryptionShare) {
        if self.is_threshold_met() {
            return; // Yeterli share var, daha fazlasını kabul etme
        }
        self.shares.insert(share.validator_index, share.share_point);
    }

    /// Threshold karşılandı mı?
    pub fn is_threshold_met(&self) -> bool {
        self.shares.len() as u32 >= self.threshold
    }

    /// Kaç share toplandı?
    pub fn share_count(&self) -> usize {
        self.shares.len()
    }

    /// Toplanan validator indeksleri.
    pub fn contributor_indices(&self) -> Vec<u32> {
        let mut indices: Vec<u32> = self.shares.keys().copied().collect();
        indices.sort();
        indices
    }

    /// Tüm share noktaları (Lagrange interpolasyonu için).
    pub fn all_shares(&self) -> Vec<(u32, [u8; 32])> {
        let mut result: Vec<(u32, [u8; 32])> = self.shares.iter()
            .map(|(&idx, &pt)| (idx, pt))
            .collect();
        result.sort_by_key(|(idx, _)| *idx);
        result
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::types::EpochId;

    fn make_eon_id(n: u64) -> EonId {
        EpochId(n)
    }

    fn make_slot(n: u64) -> SlotId {
        SlotId(n)
    }

    /// Rolling Shutter BatchIndex: eon + slot kombinasyonu.
    #[test]
    fn test_batch_index_epoch_id_bytes() {
        let batch = BatchIndex::new(make_eon_id(5), make_slot(1000));
        let bytes = batch.to_epoch_id_bytes();

        // eon_id=5 first 8 bytes
        let eon_bytes = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        assert_eq!(eon_bytes, 5);

        // slot_id=1000 next 8 bytes
        let slot_bytes = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        assert_eq!(slot_bytes, 1000);
    }

    #[test]
    fn test_batch_index_deterministic() {
        let b1 = BatchIndex::new(make_eon_id(3), make_slot(500));
        let b2 = BatchIndex::new(make_eon_id(3), make_slot(500));
        assert_eq!(b1.to_epoch_id_bytes(), b2.to_epoch_id_bytes());
    }

    #[test]
    fn test_batch_index_unique_per_slot() {
        let b1 = BatchIndex::new(make_eon_id(1), make_slot(100));
        let b2 = BatchIndex::new(make_eon_id(1), make_slot(101));
        assert_ne!(b1.to_epoch_id_bytes(), b2.to_epoch_id_bytes());
    }

    /// Rolling Shutter EonPublicKey: geçerlilik kontrolü.
    #[test]
    fn test_eon_public_key_validity() {
        let key = EonPublicKey::new(
            make_eon_id(1),
            [0x02u8; 32],
            make_slot(0),
            make_slot(216_000),
            3,
            5,
        );

        assert!(key.is_valid_for_slot(make_slot(0)));
        assert!(key.is_valid_for_slot(make_slot(215_999)));
        assert!(!key.is_valid_for_slot(make_slot(216_000)));
        assert!(!key.is_valid_for_slot(make_slot(300_000)));
    }

    #[test]
    fn test_eon_public_key_threshold_confirmation() {
        let mut key = EonPublicKey::new(
            make_eon_id(1),
            [0x02u8; 32],
            make_slot(0),
            make_slot(216_000),
            3,
            5,
        );

        assert!(!key.is_threshold_confirmed());
        key.add_confirmation();
        key.add_confirmation();
        assert!(!key.is_threshold_confirmed());
        key.add_confirmation(); // 3/3 threshold
        assert!(key.is_threshold_confirmed());
    }

    /// Rolling Shutter EonKeyPublisher: key proposal ve onay akışı.
    #[test]
    fn test_eon_key_publisher_full_flow() {
        let mut publisher = EonKeyPublisher::new(216_000);
        assert!(!publisher.has_active_key());

        // Yeni key öner
        publisher.propose_new_key(
            make_eon_id(1),
            [0x02u8; 32],
            make_slot(0),
            3,  // threshold = 3
            5,  // keypers = 5
        );

        assert!(publisher.has_pending_key());
        assert!(!publisher.has_active_key());

        // 2 onay — threshold karşılanmadı
        assert!(!publisher.add_confirmation(1));
        assert!(!publisher.add_confirmation(2));
        assert!(!publisher.has_active_key());

        // 3. onay — threshold karşılandı → key aktive oldu
        assert!(publisher.add_confirmation(3));
        assert!(publisher.has_active_key());
        assert!(!publisher.has_pending_key());
    }

    #[test]
    fn test_eon_key_publisher_get_active_key() {
        let mut publisher = EonKeyPublisher::new(216_000);
        publisher.propose_new_key(make_eon_id(1), [0x03u8; 32], make_slot(0), 1, 3);
        publisher.add_confirmation(1); // 1/1 threshold

        // Geçerli slot → key döner
        let key = publisher.get_active_key(make_slot(100));
        assert!(key.is_some());

        // Geçersiz slot → None
        let key_after = publisher.get_active_key(make_slot(216_001));
        assert!(key_after.is_none());
    }

    /// Rolling Shutter DecryptionKeyAggregator: share toplama.
    #[test]
    fn test_decryption_key_aggregator_threshold() {
        let batch = BatchIndex::new(make_eon_id(1), make_slot(100));
        let mut agg = DecryptionKeyAggregator::new(batch, 3, 5);

        assert!(!agg.is_threshold_met());
        assert_eq!(agg.share_count(), 0);

        agg.add_share(KeyperDecryptionShare::new(
            make_eon_id(1), batch, 1, [0x02u8; 32]
        ));
        agg.add_share(KeyperDecryptionShare::new(
            make_eon_id(1), batch, 2, [0x03u8; 32]
        ));
        assert!(!agg.is_threshold_met()); // 2/3

        agg.add_share(KeyperDecryptionShare::new(
            make_eon_id(1), batch, 3, [0x04u8; 32]
        ));
        assert!(agg.is_threshold_met()); // 3/3 ✓
    }

    #[test]
    fn test_decryption_key_aggregator_duplicate_share() {
        let batch = BatchIndex::new(make_eon_id(1), make_slot(200));
        let mut agg = DecryptionKeyAggregator::new(batch, 2, 5);

        // Aynı validator 2 kez share gönderir — sadece 1 sayılır
        agg.add_share(KeyperDecryptionShare::new(
            make_eon_id(1), batch, 1, [0x02u8; 32]
        ));
        agg.add_share(KeyperDecryptionShare::new(
            make_eon_id(1), batch, 1, [0x03u8; 32] // farklı değer ama aynı index
        ));
        assert_eq!(agg.share_count(), 1, "Aynı validator duplicate sayılmaz");
    }

    #[test]
    fn test_decryption_key_aggregator_contributor_order() {
        let batch = BatchIndex::new(make_eon_id(1), make_slot(300));
        let mut agg = DecryptionKeyAggregator::new(batch, 3, 5);

        // Sırasız ekle
        agg.add_share(KeyperDecryptionShare::new(make_eon_id(1), batch, 3, [0x04u8; 32]));
        agg.add_share(KeyperDecryptionShare::new(make_eon_id(1), batch, 1, [0x02u8; 32]));
        agg.add_share(KeyperDecryptionShare::new(make_eon_id(1), batch, 2, [0x03u8; 32]));

        let indices = agg.contributor_indices();
        assert_eq!(indices, vec![1, 2, 3], "Sıralı döner");
    }

    #[test]
    fn test_keyper_decryption_share_creation() {
        let batch = BatchIndex::new(make_eon_id(2), make_slot(500));
        let share = KeyperDecryptionShare::new(
            make_eon_id(2), batch, 4, [0x02u8; 32]
        );

        assert_eq!(share.validator_index, 4);
        assert_eq!(share.lagrange_index, 4);
        assert_eq!(share.share_point, [0x02u8; 32]);
    }

    /// EonKeyPublisher: aynı validator çift onay kabul etmez.
    #[test]
    fn test_eon_key_publisher_duplicate_confirmation() {
        let mut publisher = EonKeyPublisher::new(216_000);
        publisher.propose_new_key(
            make_eon_id(1), [0x02u8; 32], make_slot(0), 3, 5
        );

        publisher.add_confirmation(1);
        publisher.add_confirmation(1); // duplicate
        publisher.add_confirmation(1); // duplicate

        // Set kullandığımız için duplicate sayılmaz
        assert_eq!(publisher.pending_confirmation_count(), 1);
        assert!(!publisher.has_active_key(), "Threshold karşılanmamış");
    }
}
