//! GÖREV 95: EIP-4844 Blob Transactions — Gerçek KZG Polynomial Commitment
//!
//! ## Mimari: Gerçek BLS12-381 KZG (arkworks)
//!
//! ### Ethereum EIP-4844
//! - Blob: 128 KB'lık veri birimi (KZG commitment ile)
//! - `max_fee_per_blob_gas`: ayrı blob gas piyasası
//! - `blob_versioned_hashes`: `0x01 || SHA-256(commitment)[1..]`
//! - Blob verisi konsensüs katmanında taşınır, ~1 ay sonra silinir
//!
//! ### Nova KZG Implementasyonu
//! - Polinom: blob verisi → BLS12-381 Fr katsayıları (31B chunk/katsayı)
//! - Commitment: `C = Σ coeff[i] * SRS[i]` → 48B compressed G1 noktası
//! - Dev SRS: deterministic τ = BLAKE3("nova-blob-kzg-dev-tau") (geliştirme ortamı)
//! - Mainnet: gerçek trusted setup seremonisinden SRS yüklenir
//! - Versioned hash: `0x01 || SHA-256(commitment_bytes)[1..]` (EIP-4844 uyumlu)

use blake3;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ark_bls12_381::{Fr, G1Affine, G1Projective};
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::PrimeField;
use ark_serialize::CanonicalSerialize;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Bir blob'un boyutu (EIP-4844: 4096 field elements * 32 bytes)
pub const BLOB_SIZE: usize = 128 * 1024; // 128 KB

/// Bir blokta maksimum blob sayısı (EIP-4844: TARGET=3, MAX=6)
pub const MAX_BLOBS_PER_BLOCK: usize = 6;

/// Bir blokta hedef blob sayısı (blob gas fiyat hesabı için)
pub const TARGET_BLOBS_PER_BLOCK: usize = 3;

/// Bir TX'de maksimum blob sayısı
pub const MAX_BLOBS_PER_TX: usize = 6;

/// Versioned hash sürüm byte'ı (EIP-4844: 0x01 = KZG)
pub const BLOB_COMMITMENT_VERSION: u8 = 0x01;

/// Minimum blob gas fee (wei benzeri)
pub const MIN_BLOB_BASE_FEE: u128 = 1;

/// Blob gas fiyat güncelleme faktörü (EIP-4844: 3338477)
pub const BLOB_BASE_FEE_UPDATE_FRACTION: u128 = 3_338_477;

/// Blob TTL: kaç slot sonra silinir (~4 hafta @ 400ms slot)
pub const BLOB_TTL_SLOTS: u64 = 60 * 24 * 7 * 4 * (1000 / 400); // ~6.04M slot

/// Gas birim: blob başına
pub const GAS_PER_BLOB: u64 = 2u64.pow(17); // 131_072

/// KZG polinom derecesi — dev SRS boyutu
/// EIP-4844: 4096; Nova dev: 128 (hızlı test + OnceLock cache)
/// Not: Büyük bloblar için ilk MAX_KZG_DEG katsayı kullanılır
const MAX_KZG_DEG: usize = 128;

/// BLS12-381 Fr'ye güvenle sığan chunk boyutu (Fr < 2^255, 31B ≤ 248 bit)
const FR_CHUNK_BYTES: usize = 31;

// ─── Dev SRS (Structured Reference String) ───────────────────────────────────

/// Geliştirme SRS: `SRS[i] = τ^i * G1` (deterministic τ)
///
/// Mainnet için: gerçek trusted setup seremonisinden (EIP-4844 KZG ceremony)
/// yüklenmesi gerekir. Bu SRS yalnızca geliştirme/test ortamı içindir.
static DEV_SRS: OnceLock<Vec<G1Affine>> = OnceLock::new();

fn dev_srs() -> &'static [G1Affine] {
    DEV_SRS.get_or_init(|| {
        // τ = BLAKE3("nova-blob-kzg-dev-tau") mod Fr
        // NOT: Bu "toxic waste" içerir — mainnet'te kullanılmamalı
        let tau_bytes = *blake3::hash(b"nova-blob-kzg-dev-tau").as_bytes();
        let tau = Fr::from_le_bytes_mod_order(&tau_bytes);

        let g1 = G1Affine::generator();
        let mut powers: Vec<G1Affine> = Vec::with_capacity(MAX_KZG_DEG);
        let mut current = Fr::from(1u64);

        for _ in 0..MAX_KZG_DEG {
            let point: G1Affine = (g1 * current).into();
            powers.push(point);
            current *= tau;
        }
        powers
    })
}

// ─── KZG Yardımcı Fonksiyonlar ────────────────────────────────────────────────

/// Blob verisini BLS12-381 Fr polinomuna dönüştür
///
/// Her 31 baytlık chunk → bir Fr katsayısı
/// EIP-4844: her field element < BLS12-381 scalar field sırası
fn blob_to_coefficients(data: &[u8]) -> Vec<Fr> {
    data.chunks(FR_CHUNK_BYTES)
        .map(|chunk| Fr::from_le_bytes_mod_order(chunk))
        .take(MAX_KZG_DEG)
        .collect()
}

/// KZG commitment hesapla: `C = Σ coeff[i] * SRS[i]`
///
/// Multi-scalar multiplication (MSM) — arkworks G1Projective
fn kzg_commit(coefficients: &[Fr]) -> G1Affine {
    let srs = dev_srs();
    let n = coefficients.len().min(srs.len());

    let result: G1Projective = coefficients[..n]
        .iter()
        .zip(srs[..n].iter())
        .map(|(c, g)| *g * c)
        .sum();

    result.into()
}

// ─── BlobVersionedHash ────────────────────────────────────────────────────────

/// EIP-4844 versioned hash: `0x01 || SHA-256(kzg_commitment)[1..]`
///
/// Ethereum: `kzg_commitment_to_versioned_hash(commitment) → bytes32`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlobVersionedHash(pub [u8; 32]);

impl BlobVersionedHash {
    /// KZG commitment'tan (48B) versioned hash (32B) üret
    ///
    /// EIP-4844: `SHA-256(commitment)` → ilk byte 0x01 ile üstüne yazılır
    pub fn from_commitment(commitment: &BlobCommitment) -> Self {
        let sha256 = Sha256::digest(&commitment.commitment);
        let mut vh = [0u8; 32];
        vh[0] = BLOB_COMMITMENT_VERSION;
        vh[1..].copy_from_slice(&sha256[1..]);
        Self(vh)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─── BlobCommitment ───────────────────────────────────────────────────────────

/// Blob KZG taahhüdü (EIP-4844: 48 bayt compressed BLS12-381 G1 noktası)
///
/// `C = Σ coeff[i] * τ^i * G1` — polinom değerlendirmesi G1 üzerinde
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobCommitment {
    /// 48 bayt compressed BLS12-381 G1 noktası (KZG commitment)
    pub commitment: [u8; 48],
    /// Blob boyutu
    pub data_len: usize,
}

impl BlobCommitment {
    /// Blob verisinden gerçek KZG commitment hesapla
    ///
    /// 1. Blob → Fr katsayıları (31B/chunk)
    /// 2. MSM: `C = Σ coeff[i] * SRS[i]`
    /// 3. G1 noktasını 48B compressed format'a serialize et
    pub fn commit(data: &[u8]) -> Self {
        let coeffs = blob_to_coefficients(data);
        let point = kzg_commit(&coeffs);

        let mut commitment = [0u8; 48];
        point.serialize_compressed(commitment.as_mut_slice())
            .expect("G1Affine serialize başarısız olmamalı");

        Self { commitment, data_len: data.len() }
    }

    /// Versioned hash üret (EIP-4844 uyumlu)
    pub fn versioned_hash(&self) -> BlobVersionedHash {
        BlobVersionedHash::from_commitment(self)
    }
}

// ─── BlobData ─────────────────────────────────────────────────────────────────

/// Blob verisi + KZG taahhüdü
///
/// Ethereum: `BlobBundle { blobs, commitments, proofs }`
#[derive(Debug, Clone)]
pub struct BlobData {
    /// Ham blob verisi
    pub data: Vec<u8>,
    /// KZG commitment (48B compressed G1)
    pub commitment: BlobCommitment,
    /// Versioned hash (TX'te referanslanır)
    pub versioned_hash: BlobVersionedHash,
}

impl BlobData {
    /// Yeni blob oluştur (KZG commitment otomatik hesaplanır)
    pub fn new(data: Vec<u8>) -> Self {
        let commitment = BlobCommitment::commit(&data);
        let versioned_hash = commitment.versioned_hash();
        Self { data, commitment, versioned_hash }
    }

    /// Geçerlilik doğrula: KZG commitment blob'a uyuyor mu?
    ///
    /// Commitment'ı yeniden hesaplar ve karşılaştırır
    pub fn verify(&self) -> bool {
        let expected = BlobCommitment::commit(&self.data);
        expected.commitment == self.commitment.commitment
    }
}

// ─── BlobTransaction ──────────────────────────────────────────────────────────

/// EIP-4844 Type 3 Transaction
///
/// Ethereum: `TxEIP4844 { chain_id, nonce, max_fee_per_gas, gas, to,
///            max_fee_per_blob_gas, blob_versioned_hashes, ... }`
#[derive(Debug, Clone)]
pub struct BlobTransaction {
    /// Gönderen
    pub sender: [u8; 20],
    /// Nonce
    pub nonce: u64,
    /// Hedef (contract address, blob tx'te to zorunlu)
    pub to: [u8; 20],
    /// Maksimum base fee (Gwei benzeri)
    pub max_fee_per_gas: u128,
    /// Priority fee
    pub max_priority_fee_per_gas: u128,
    /// Gas limiti
    pub gas_limit: u64,
    /// Maksimum blob fee
    pub max_fee_per_blob_gas: u128,
    /// Blob versioned hash'leri (TX'te sadece hash, blob ayrı taşınır)
    pub blob_versioned_hashes: Vec<BlobVersionedHash>,
    /// İmza (mock)
    pub signature: Vec<u8>,
}

impl BlobTransaction {
    /// Yeni blob TX oluştur
    pub fn new(
        sender: [u8; 20],
        nonce: u64,
        to: [u8; 20],
        blobs: &[&BlobData],
        max_fee_per_gas: u128,
        max_fee_per_blob_gas: u128,
    ) -> Self {
        let blob_versioned_hashes = blobs.iter()
            .map(|b| b.versioned_hash.clone())
            .collect();

        Self {
            sender,
            nonce,
            to,
            max_fee_per_gas,
            max_priority_fee_per_gas: 0,
            gas_limit: 21_000,
            max_fee_per_blob_gas,
            blob_versioned_hashes,
            signature: vec![0u8; 65],
        }
    }

    /// Blob gas kullanımı
    pub fn blob_gas_used(&self) -> u64 {
        self.blob_versioned_hashes.len() as u64 * GAS_PER_BLOB
    }

    /// TX geçerli mi?
    pub fn validate(&self, current_blob_base_fee: u128) -> BlobTxResult<()> {
        if self.blob_versioned_hashes.is_empty() {
            return Err(BlobTxError::NoBlobHashes);
        }
        if self.blob_versioned_hashes.len() > MAX_BLOBS_PER_TX {
            return Err(BlobTxError::TooManyBlobs(self.blob_versioned_hashes.len()));
        }
        if self.max_fee_per_blob_gas < current_blob_base_fee {
            return Err(BlobTxError::BlobFeeTooLow {
                provided: self.max_fee_per_blob_gas,
                required: current_blob_base_fee,
            });
        }
        Ok(())
    }
}

// ─── BlobTxError ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobTxError {
    NoBlobHashes,
    TooManyBlobs(usize),
    BlobFeeTooLow { provided: u128, required: u128 },
    BlobNotFound(BlobVersionedHash),
    CommitmentMismatch,
    BlobTooLarge(usize),
    Expired,
}

pub type BlobTxResult<T> = Result<T, BlobTxError>;

// ─── BlobBaseFeeOracle ────────────────────────────────────────────────────────

/// EIP-4844 blob gas piyasası
///
/// Ethereum: `calc_blob_fee(excess_blob_gas) = MIN_BLOB_BASE_FEE * e^(excess/UPDATE_FRACTION)`
/// Nova: lineer yaklaşım (exponential approx için daha büyük refactor gerekir)
#[derive(Debug, Clone)]
pub struct BlobBaseFeeOracle {
    /// Mevcut excess blob gas (blok başında birikir)
    excess_blob_gas: u64,
    /// Mevcut blob base fee
    current_fee: u128,
}

impl BlobBaseFeeOracle {
    pub fn new() -> Self {
        Self {
            excess_blob_gas: 0,
            current_fee: MIN_BLOB_BASE_FEE,
        }
    }

    /// Blok sonunda güncelle
    ///
    /// EIP-4844: `calc_excess_blob_gas(parent_excess_blob_gas, parent_blob_gas_used)`
    pub fn update(&mut self, blob_count: usize) {
        let blob_gas_used = blob_count as u64 * GAS_PER_BLOB;
        let target_gas = TARGET_BLOBS_PER_BLOCK as u64 * GAS_PER_BLOB;

        if blob_gas_used > target_gas {
            self.excess_blob_gas += blob_gas_used - target_gas;
        } else {
            self.excess_blob_gas = self.excess_blob_gas.saturating_sub(target_gas - blob_gas_used);
        }

        let excess = self.excess_blob_gas as u128;
        self.current_fee = MIN_BLOB_BASE_FEE.saturating_add(excess / BLOB_BASE_FEE_UPDATE_FRACTION);
        if self.current_fee < MIN_BLOB_BASE_FEE {
            self.current_fee = MIN_BLOB_BASE_FEE;
        }
    }

    pub fn current_fee(&self) -> u128 {
        self.current_fee
    }

    pub fn excess_blob_gas(&self) -> u64 {
        self.excess_blob_gas
    }
}

impl Default for BlobBaseFeeOracle {
    fn default() -> Self {
        Self::new()
    }
}

// ─── BlobStore ────────────────────────────────────────────────────────────────

/// Geçici blob depolama (slot bazlı TTL)
///
/// Ethereum: consensus layer blob store (Deneb/Cancun)
/// Blobs ~4 hafta saklanır, sonra prune edilir
pub struct BlobStore {
    inner: Mutex<BlobStoreInner>,
}

struct BlobStoreInner {
    /// versioned_hash → (blob_data, slot)
    blobs: HashMap<[u8; 32], (BlobData, u64)>,
    /// Toplam saklanan blob sayısı
    total_stored: u64,
    /// Toplam prune edilen blob sayısı
    total_pruned: u64,
}

impl BlobStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BlobStoreInner {
                blobs: HashMap::new(),
                total_stored: 0,
                total_pruned: 0,
            }),
        }
    }

    /// Blob ekle (slot = mevcut slot)
    pub fn store(&self, blob: BlobData, current_slot: u64) -> BlobTxResult<BlobVersionedHash> {
        if blob.data.len() > BLOB_SIZE {
            return Err(BlobTxError::BlobTooLarge(blob.data.len()));
        }
        if !blob.verify() {
            return Err(BlobTxError::CommitmentMismatch);
        }

        let hash = blob.versioned_hash.0;
        let mut inner = self.inner.lock().unwrap();
        let vh = blob.versioned_hash.clone();
        inner.blobs.insert(hash, (blob, current_slot));
        inner.total_stored += 1;
        Ok(vh)
    }

    /// Blob al
    pub fn get(&self, hash: &BlobVersionedHash) -> Option<BlobData> {
        self.inner.lock().unwrap()
            .blobs.get(&hash.0)
            .map(|(blob, _)| blob.clone())
    }

    /// TX'teki tüm blob hash'lerini doğrula
    pub fn verify_tx_blobs(&self, tx: &BlobTransaction, current_slot: u64) -> BlobTxResult<()> {
        let inner = self.inner.lock().unwrap();
        for vh in &tx.blob_versioned_hashes {
            match inner.blobs.get(&vh.0) {
                None => return Err(BlobTxError::BlobNotFound(vh.clone())),
                Some((blob, stored_slot)) => {
                    if current_slot.saturating_sub(*stored_slot) > BLOB_TTL_SLOTS {
                        return Err(BlobTxError::Expired);
                    }
                    if !blob.verify() {
                        return Err(BlobTxError::CommitmentMismatch);
                    }
                }
            }
        }
        Ok(())
    }

    /// Süresi dolmuş blob'ları temizle
    pub fn prune(&self, current_slot: u64) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.blobs.len();
        inner.blobs.retain(|_, (_, stored_slot)| {
            current_slot.saturating_sub(*stored_slot) <= BLOB_TTL_SLOTS
        });
        let pruned = before - inner.blobs.len();
        inner.total_pruned += pruned as u64;
        pruned
    }

    pub fn blob_count(&self) -> usize {
        self.inner.lock().unwrap().blobs.len()
    }

    pub fn total_stored(&self) -> u64 {
        self.inner.lock().unwrap().total_stored
    }

    pub fn total_pruned(&self) -> u64 {
        self.inner.lock().unwrap().total_pruned
    }
}

impl Default for BlobStore {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_blob(content: u8) -> BlobData {
        let data = vec![content; 1024]; // 1 KB test blob
        BlobData::new(data)
    }

    // 1. KZG commitment deterministik (aynı veri → aynı commitment)
    #[test]
    fn test_blob_commitment_deterministic() {
        let data = vec![0xABu8; 512];
        let c1 = BlobCommitment::commit(&data);
        let c2 = BlobCommitment::commit(&data);
        assert_eq!(c1.commitment, c2.commitment, "KZG commitment deterministik olmalı");
    }

    // 2. Farklı blob → farklı KZG commitment
    #[test]
    fn test_blob_commitment_different() {
        let c1 = BlobCommitment::commit(&[0u8; 512]);
        let c2 = BlobCommitment::commit(&[1u8; 512]);
        assert_ne!(c1.commitment, c2.commitment, "Farklı bloblar farklı commitment üretmeli");
    }

    // 3. KZG commitment 48 byte (compressed BLS12-381 G1)
    #[test]
    fn test_blob_commitment_size() {
        let c = BlobCommitment::commit(&[0x42u8; 256]);
        // 48 bayt compressed G1 noktası
        assert_eq!(c.commitment.len(), 48);
        // Geçerli compressed G1: ilk byte 0x80, 0x81, veya 0xC0 (flag bits)
        let first_byte = c.commitment[0];
        assert!(first_byte & 0x80 != 0, "compressed flag set olmalı");
    }

    // 4. Versioned hash version byte (EIP-4844: 0x01)
    #[test]
    fn test_versioned_hash_version_byte() {
        let blob = make_blob(0x42);
        assert_eq!(blob.versioned_hash.0[0], BLOB_COMMITMENT_VERSION,
            "Versioned hash ilk byte 0x01 olmalı");
    }

    // 5. Versioned hash 32 byte
    #[test]
    fn test_versioned_hash_size() {
        let blob = make_blob(0x10);
        assert_eq!(blob.versioned_hash.0.len(), 32);
    }

    // 6. BlobData verify
    #[test]
    fn test_blob_data_verify() {
        let blob = make_blob(0xFF);
        assert!(blob.verify(), "KZG commitment doğrulanmalı");
    }

    // 7. BlobData tampered verify fails
    #[test]
    fn test_blob_data_tampered_verify_fails() {
        let mut blob = make_blob(0x01);
        blob.data[0] ^= 0xFF; // veriyi boz
        assert!(!blob.verify(), "Bozuk blob doğrulama başarısız olmalı");
    }

    // 8. Farklı bloblar farklı versioned hash
    #[test]
    fn test_versioned_hash_unique() {
        let b1 = make_blob(0x01);
        let b2 = make_blob(0x02);
        assert_ne!(b1.versioned_hash, b2.versioned_hash);
    }

    // 9. BlobTransaction oluşturma
    #[test]
    fn test_blob_tx_creation() {
        let blob1 = make_blob(1);
        let blob2 = make_blob(2);
        let tx = BlobTransaction::new(
            [1u8; 20], 0, [2u8; 20],
            &[&blob1, &blob2],
            1_000_000, 100,
        );
        assert_eq!(tx.blob_versioned_hashes.len(), 2);
        assert_eq!(tx.blob_gas_used(), 2 * GAS_PER_BLOB);
    }

    // 10. BlobTransaction validasyon — başarılı
    #[test]
    fn test_blob_tx_validate_success() {
        let blob = make_blob(0xAA);
        let tx = BlobTransaction::new([0u8; 20], 0, [1u8; 20], &[&blob], 1000, 10);
        assert!(tx.validate(5).is_ok());
    }

    // 11. BlobTransaction validasyon — fee çok düşük
    #[test]
    fn test_blob_tx_validate_fee_too_low() {
        let blob = make_blob(0xBB);
        let tx = BlobTransaction::new([0u8; 20], 0, [1u8; 20], &[&blob], 1000, 1);
        let err = tx.validate(100).unwrap_err();
        assert!(matches!(err, BlobTxError::BlobFeeTooLow { .. }));
    }

    // 12. BlobTransaction validasyon — blob yok
    #[test]
    fn test_blob_tx_validate_no_blobs() {
        let tx = BlobTransaction::new([0u8; 20], 0, [1u8; 20], &[], 1000, 10);
        assert!(matches!(tx.validate(1), Err(BlobTxError::NoBlobHashes)));
    }

    // 13. BlobStore depolama ve alma
    #[test]
    fn test_blob_store_store_and_get() {
        let store = BlobStore::new();
        let blob = make_blob(0x77);
        let vh = blob.versioned_hash.clone();
        store.store(blob, 100).unwrap();

        let retrieved = store.get(&vh).unwrap();
        assert!(retrieved.verify());
        assert_eq!(store.blob_count(), 1);
    }

    // 14. BlobStore TTL prune
    #[test]
    fn test_blob_store_prune() {
        let store = BlobStore::new();
        store.store(make_blob(1), 0).unwrap();
        store.store(make_blob(2), 0).unwrap();
        store.store(make_blob(3), BLOB_TTL_SLOTS + 100).unwrap();

        let pruned = store.prune(BLOB_TTL_SLOTS + 50);
        assert_eq!(pruned, 2);
        assert_eq!(store.blob_count(), 1);
        assert_eq!(store.total_pruned(), 2);
    }

    // 15. BlobStore TX blob doğrulama
    #[test]
    fn test_blob_store_verify_tx_blobs() {
        let store = BlobStore::new();
        let blob1 = make_blob(0x11);
        let blob2 = make_blob(0x22);
        let vh1 = blob1.versioned_hash.clone();
        let vh2 = blob2.versioned_hash.clone();

        store.store(blob1, 10).unwrap();
        store.store(blob2, 10).unwrap();

        let tx = BlobTransaction {
            sender: [0u8; 20],
            nonce: 0,
            to: [1u8; 20],
            max_fee_per_gas: 1000,
            max_priority_fee_per_gas: 0,
            gas_limit: 21_000,
            max_fee_per_blob_gas: 100,
            blob_versioned_hashes: vec![vh1, vh2],
            signature: vec![0u8; 65],
        };

        assert!(store.verify_tx_blobs(&tx, 50).is_ok());
    }

    // 16. BlobBaseFeeOracle yakınsama
    #[test]
    fn test_blob_base_fee_oracle_convergence() {
        let mut oracle = BlobBaseFeeOracle::new();

        for _ in 0..10 {
            oracle.update(MAX_BLOBS_PER_BLOCK);
        }
        assert!(oracle.current_fee() > MIN_BLOB_BASE_FEE);

        for _ in 0..20 {
            oracle.update(0);
        }
        assert_eq!(oracle.current_fee(), MIN_BLOB_BASE_FEE);
    }

    // 17. BlobStore blob bulunamadı hatası
    #[test]
    fn test_blob_store_blob_not_found() {
        let store = BlobStore::new();
        let fake_blob = make_blob(0xDE);
        let tx = BlobTransaction::new([0u8; 20], 0, [1u8; 20], &[&fake_blob], 1000, 10);
        let err = store.verify_tx_blobs(&tx, 0).unwrap_err();
        assert!(matches!(err, BlobTxError::BlobNotFound(_)));
    }

    // 18. Dev SRS tutarlılığı — birden fazla çağrıda aynı sonuç
    #[test]
    fn test_dev_srs_consistency() {
        let srs1 = dev_srs();
        let srs2 = dev_srs();
        assert_eq!(srs1[0], srs2[0], "Dev SRS deterministik olmalı");
        assert_eq!(srs1.len(), MAX_KZG_DEG);
    }

    // 19. KZG commitment değişmeyen veriden değişmez
    #[test]
    fn test_kzg_commitment_stability() {
        let data = b"Nova Shield Chain KZG blob test verisi";
        let c1 = BlobCommitment::commit(data);
        let c2 = BlobCommitment::commit(data);
        assert_eq!(c1.commitment, c2.commitment);
        assert_eq!(c1.data_len, data.len());
    }

    // 20. Boş blob commitment geçerli G1 noktası
    #[test]
    fn test_empty_blob_commitment() {
        let c = BlobCommitment::commit(b"");
        // Sıfır katsayı → identity point (compressed: 0xC0...)
        let _vh = c.versioned_hash();
        assert_eq!(c.commitment[0] & 0x80, 0x80, "Geçerli compressed G1 flag");
    }
}
