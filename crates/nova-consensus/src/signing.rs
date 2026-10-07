//! BFT Signing — üretim BLS imzalama katmanı
//!
//! ## Mimari
//!
//! Nova Consensus'ta iki imzalama katmanı vardır:
//!
//! 1. **Mock katman** (test): `Vote::mock()` → blake3 tabanlı 64-byte imza
//!    - Tüm mevcut testler bu katmanı kullanır
//!    - `Vote.signature: [u8; 64]`
//!
//! 2. **Üretim katmanı** (bu modül): gerçek BLS imzaları
//!    - `SignedVote`: Vote + 96-byte BLS imzası (blst crate via nova-crypto)
//!    - `AggregateVote`: N adet SignedVote → tek BLS aggregate
//!    - `BftSigner`: validator keypair ile oy imzala
//!    - `BftVerifier`: tekil ve aggregate BLS imzalarını doğrula
//!
//! ## Mesaj formatı (Ethereum beacon chain pattern)
//!
//! ```text
//! vote_message = blake3("nova-vote-v1:" || block_hash || validator_id || height || slot)
//! ```
//!
//! Domain separation: her mesaj tipi ayrı domain separator ile.
//!
//! ## BLS aggregate
//!
//! Quorum Certificate oluşturmak için:
//! 1. Her validator kendi `SignedVote`'unu üretir
//! 2. Leader N adet SignedVote toplar
//! 3. `AggregateVote::build()` → tek BLS aggregate imzası (96 byte)
//! 4. `BftVerifier::verify_aggregate()` → tüm pubkey'ler ile doğrula
//!
//! ## Polkadot referansı
//! `sc-consensus-grandpa/src/communication/gossip.rs` → `Vote` + GRANDPA signature
//!
//! ## Ethereum/Lighthouse referansı
//! `lighthouse/src/consensus/types/src/attestation.rs` → BLS signature per validator

use crate::errors::ConsensusError;
use nova_crypto::{BlsKeypair, BlsVerifier, CryptoError};
use nova_primitives::{BlsPublicKey, BlsSignature, BlockHash, SlotId, ValidatorId};

// ─── Hata türü ────────────────────────────────────────────────────────────────

/// BFT imzalama hatası
#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("BLS keygen hatası: {0}")]
    KeyGen(String),

    #[error("BLS imzalama hatası: {0}")]
    Sign(String),

    #[error("BLS doğrulama hatası: {0}")]
    Verify(String),

    #[error("Aggregate hatası: {0}")]
    Aggregate(String),

    #[error("Validator public key bulunamadı: {0}")]
    MissingPublicKey(u64),

    #[error("Quorum yetersiz: {need} gerekli, {have} var")]
    InsufficientQuorum { need: usize, have: usize },
}

pub type SigningResult<T> = Result<T, SigningError>;

impl From<CryptoError> for SigningError {
    fn from(e: CryptoError) -> Self {
        SigningError::Verify(e.to_string())
    }
}

// ─── Vote mesaj formatı ───────────────────────────────────────────────────────

/// BLS imzası için standart oy mesajı üret
///
/// `blake3("nova-vote-v1:" || block_hash[32] || validator_id[8] || height[8] || slot[8])`
///
/// Domain separator: "nova-vote-v1:" → replay saldırılarını önler (farklı zincir/protokol)
pub fn vote_message(
    block_hash: BlockHash,
    validator_id: ValidatorId,
    height: u64,
    slot: SlotId,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"nova-vote-v1:");
    hasher.update(block_hash.as_bytes());
    hasher.update(&validator_id.0.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(&slot.0.to_le_bytes());
    *hasher.finalize().as_bytes()
}

// ─── SignedVote ───────────────────────────────────────────────────────────────

/// Oy + gerçek BLS imzası (96 byte)
///
/// Mock `Vote.signature: [u8; 64]`'ten farklı olarak bu struct
/// gerçek blst BLS imzasını taşır.
#[derive(Debug, Clone)]
pub struct SignedVote {
    /// Oy veren validator
    pub validator_id: ValidatorId,
    /// İmzalanan blok hash'i
    pub block_hash: BlockHash,
    /// Blok yüksekliği
    pub height: u64,
    /// Slot
    pub slot: SlotId,
    /// Gerçek BLS imzası (blst, 96 byte)
    pub bls_signature: BlsSignature,
    /// Validator'ın BLS public key'i (doğrulama için)
    pub bls_pubkey: BlsPublicKey,
}

impl SignedVote {
    /// İmzayı doğrula
    pub fn verify(&self) -> SigningResult<()> {
        let msg = vote_message(self.block_hash, self.validator_id, self.height, self.slot);
        BlsVerifier::verify(&msg, &self.bls_signature, &self.bls_pubkey)
            .map_err(|e| SigningError::Verify(e.to_string()))
    }
}

// ─── AggregateVote ────────────────────────────────────────────────────────────

/// N adet SignedVote koleksiyonu — batch BLS doğrulama
///
/// Her validator kendi mesajını (validator_id içeren) imzaladığından
/// "same-msg aggregate" mümkün değil. Bireysel imzalar saklanır ve
/// batch verify edilir.
///
/// Quorum Certificate oluştururken `signers.len() >= quorum_threshold(total)` kontrol edilir.
#[derive(Debug, Clone)]
pub struct AggregateVote {
    /// İmzalanan blok hash'i (tüm oylar aynı)
    pub block_hash: BlockHash,
    /// Blok yüksekliği
    pub height: u64,
    /// İmzacı validator'lar
    pub signers: Vec<ValidatorId>,
    /// Her imzacının bireysel BLS imzası (bireysel doğrulama için)
    pub signatures: Vec<BlsSignature>,
    /// Her imzacının public key'i (doğrulama için)
    pub pubkeys: Vec<BlsPublicKey>,
    /// İmza atılan slot
    pub slot: SlotId,
}

impl AggregateVote {
    /// N adet SignedVote → AggregateVote
    ///
    /// Tüm oyların aynı block_hash/height imzaladığı varsayılır.
    pub fn build(votes: &[SignedVote]) -> SigningResult<Self> {
        if votes.is_empty() {
            return Err(SigningError::Aggregate("oy listesi boş".into()));
        }

        let first = &votes[0];
        let block_hash = first.block_hash;
        let height = first.height;
        let slot = first.slot;

        // Tutarlılık kontrolü
        for v in votes {
            if v.block_hash != block_hash || v.height != height {
                return Err(SigningError::Aggregate(
                    "oylar farklı bloklara ait".into(),
                ));
            }
        }

        let signers = votes.iter().map(|v| v.validator_id).collect();
        let signatures = votes.iter().map(|v| v.bls_signature.clone()).collect();
        let pubkeys = votes.iter().map(|v| v.bls_pubkey.clone()).collect();

        Ok(Self { block_hash, height, signers, signatures, pubkeys, slot })
    }

    /// Tüm imzacılar için batch BLS doğrulama
    ///
    /// Her validator kendi validator_id'sini içeren mesajı imzaladı;
    /// bireysel imzaları `verify_aggregate` ile batch doğrula.
    pub fn verify(&self) -> SigningResult<()> {
        if self.signers.is_empty() {
            return Err(SigningError::Verify("imzacı yok".into()));
        }

        let pubkey_refs: Vec<&BlsPublicKey> = self.pubkeys.iter().collect();
        let sig_refs: Vec<&BlsSignature> = self.signatures.iter().collect();

        // Mesajlar: her validator kendi ID'sini içeriyor → hepsi farklı
        let messages: Vec<[u8; 32]> = self.signers.iter()
            .map(|&vid| vote_message(self.block_hash, vid, self.height, self.slot))
            .collect();
        let msg_refs: Vec<&[u8]> = messages.iter().map(|m| m.as_slice()).collect();

        BlsVerifier::verify_aggregate(&msg_refs, &sig_refs, &pubkey_refs)
            .map_err(|e| SigningError::Verify(e.to_string()))
    }

    /// İmzacı sayısı
    pub fn signer_count(&self) -> usize {
        self.signers.len()
    }

    /// Quorum kontrolü: 2/3 + 1 gerekli
    pub fn has_quorum(&self, total_validators: usize) -> bool {
        let quorum = crate::types::quorum_threshold(total_validators);
        self.signers.len() >= quorum
    }

}

// ─── BftSigner ────────────────────────────────────────────────────────────────

/// Validator BLS signer — validator'ın keypair'i ile oy imzalar
///
/// Polkadot `sc-keystore::LocalKeystore` pattern:
/// - IKM (Input Key Material): validator secret seed
/// - Her validator node başlangıçta bu signer'ı oluşturur
pub struct BftSigner {
    validator_id: ValidatorId,
    keypair: BlsKeypair,
}

impl BftSigner {
    /// IKM'den deterministik signer oluştur
    ///
    /// `ikm`: 32 byte secret seed (keystore'dan gelir)
    pub fn from_ikm(validator_id: ValidatorId, ikm: &[u8; 32]) -> SigningResult<Self> {
        let keypair = BlsKeypair::from_ikm(ikm)
            .map_err(|e| SigningError::KeyGen(e.to_string()))?;
        Ok(Self { validator_id, keypair })
    }

    /// Rastgele keypair oluştur (test ortamı)
    pub fn generate(validator_id: ValidatorId) -> SigningResult<Self> {
        let keypair = BlsKeypair::generate()
            .map_err(|e| SigningError::KeyGen(e.to_string()))?;
        Ok(Self { validator_id, keypair })
    }

    /// Blok hash'ini imzala → SignedVote üret
    pub fn sign_vote(
        &self,
        block_hash: BlockHash,
        height: u64,
        slot: SlotId,
    ) -> SigningResult<SignedVote> {
        let msg = vote_message(block_hash, self.validator_id, height, slot);
        let bls_sig = self.keypair.sign_raw(&msg);
        let bls_pubkey = self.keypair.bls_public_key();

        Ok(SignedVote {
            validator_id: self.validator_id,
            block_hash,
            height,
            slot,
            bls_signature: bls_sig,
            bls_pubkey,
        })
    }

    /// Bu validator'ın BLS public key'ini döndür
    pub fn public_key(&self) -> BlsPublicKey {
        self.keypair.bls_public_key()
    }

    /// Validator ID
    pub fn validator_id(&self) -> ValidatorId {
        self.validator_id
    }
}

// ─── BftVerifier ──────────────────────────────────────────────────────────────

/// BFT imza doğrulayıcı — tekil ve aggregate oyları doğrular
///
/// Polkadot `grandpa::GossipValidator` + Ethereum `BLSVerifier` pattern
pub struct BftVerifier;

impl BftVerifier {
    /// Tekil oy doğrula
    pub fn verify_vote(vote: &SignedVote) -> SigningResult<()> {
        vote.verify()
    }

    /// Aggregate oy doğrula
    pub fn verify_aggregate(agg: &AggregateVote) -> SigningResult<()> {
        agg.verify()
    }

    /// N adet SignedVote'u AggregateVote'a dönüştür ve doğrula
    pub fn aggregate_and_verify(
        votes: &[SignedVote],
        total_validators: usize,
        required_quorum: bool,
    ) -> SigningResult<AggregateVote> {
        let agg = AggregateVote::build(votes)?;
        agg.verify()?;

        if required_quorum && !agg.has_quorum(total_validators) {
            return Err(SigningError::InsufficientQuorum {
                need: crate::types::quorum_threshold(total_validators),
                have: agg.signer_count(),
            });
        }

        Ok(agg)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::BlockHash;

    fn block_hash(n: u8) -> BlockHash {
        BlockHash::from_bytes([n; 32])
    }

    fn vid(n: u64) -> ValidatorId {
        ValidatorId(n)
    }

    fn make_signer(n: u64) -> BftSigner {
        // Deterministik IKM: test için sabit seed
        let ikm = [(n as u8).wrapping_mul(37); 32];
        BftSigner::from_ikm(vid(n), &ikm).unwrap()
    }

    // ─── vote_message ─────────────────────────────────────────────────────────

    #[test]
    fn test_vote_message_deterministic() {
        let bh = block_hash(1);
        let m1 = vote_message(bh, vid(1), 10, SlotId(100));
        let m2 = vote_message(bh, vid(1), 10, SlotId(100));
        assert_eq!(m1, m2, "vote_message deterministik olmalı");
    }

    #[test]
    fn test_vote_message_domain_separation() {
        let bh = block_hash(1);
        // Aynı parametreler, farklı validator → farklı mesaj
        let m1 = vote_message(bh, vid(1), 10, SlotId(100));
        let m2 = vote_message(bh, vid(2), 10, SlotId(100));
        assert_ne!(m1, m2);

        // Aynı validator, farklı height → farklı mesaj
        let m3 = vote_message(bh, vid(1), 11, SlotId(100));
        assert_ne!(m1, m3);

        // Aynı parametreler, farklı slot → farklı mesaj
        let m4 = vote_message(bh, vid(1), 10, SlotId(101));
        assert_ne!(m1, m4);
    }

    // ─── BftSigner ────────────────────────────────────────────────────────────

    #[test]
    fn test_bft_signer_from_ikm() {
        let signer = make_signer(1);
        assert_eq!(signer.validator_id(), vid(1));
        // Public key deterministik
        let signer2 = make_signer(1);
        assert_eq!(
            signer.public_key().as_bytes(),
            signer2.public_key().as_bytes(),
            "Aynı IKM → aynı public key"
        );
    }

    #[test]
    fn test_bft_signer_generate() {
        let s1 = BftSigner::generate(vid(1)).unwrap();
        let s2 = BftSigner::generate(vid(2)).unwrap();
        // Rastgele üretilen key'ler farklı olmalı
        assert_ne!(
            s1.public_key().as_bytes(),
            s2.public_key().as_bytes(),
        );
    }

    // ─── SignedVote ───────────────────────────────────────────────────────────

    #[test]
    fn test_sign_vote_and_verify() {
        let signer = make_signer(1);
        let bh = block_hash(42);
        let signed = signer.sign_vote(bh, 5, SlotId(50)).unwrap();

        assert_eq!(signed.validator_id, vid(1));
        assert_eq!(signed.block_hash, bh);
        assert_eq!(signed.height, 5);

        // Doğrulama başarılı olmalı
        signed.verify().unwrap();
    }

    #[test]
    fn test_sign_vote_wrong_pubkey_fails() {
        let signer1 = make_signer(1);
        let signer2 = make_signer(2);
        let bh = block_hash(1);

        let mut signed = signer1.sign_vote(bh, 1, SlotId(10)).unwrap();
        // Yanlış public key ile doğrulama başarısız olmalı
        signed.bls_pubkey = signer2.public_key();
        let result = signed.verify();
        assert!(result.is_err(), "Yanlış pubkey ile doğrulama başarısız olmalı");
    }

    #[test]
    fn test_sign_vote_wrong_block_hash_fails() {
        let signer = make_signer(1);
        let bh = block_hash(1);

        let mut signed = signer.sign_vote(bh, 1, SlotId(10)).unwrap();
        // Farklı block_hash → doğrulama başarısız olmalı
        signed.block_hash = block_hash(2);
        let result = signed.verify();
        assert!(result.is_err(), "Yanlış block_hash ile doğrulama başarısız olmalı");
    }

    // ─── AggregateVote ────────────────────────────────────────────────────────

    #[test]
    fn test_aggregate_single_vote() {
        let signer = make_signer(1);
        let bh = block_hash(5);
        let signed = signer.sign_vote(bh, 10, SlotId(100)).unwrap();

        let agg = AggregateVote::build(&[signed]).unwrap();
        assert_eq!(agg.signer_count(), 1);
        agg.verify().unwrap();
    }

    #[test]
    fn test_aggregate_empty_fails() {
        let result = AggregateVote::build(&[]);
        assert!(result.is_err(), "Boş oy listesi hata vermeli");
    }

    #[test]
    fn test_aggregate_mismatched_blocks_fails() {
        let s1 = make_signer(1);
        let s2 = make_signer(2);

        let v1 = s1.sign_vote(block_hash(1), 1, SlotId(10)).unwrap();
        let v2 = s2.sign_vote(block_hash(2), 1, SlotId(10)).unwrap(); // farklı block

        let result = AggregateVote::build(&[v1, v2]);
        assert!(result.is_err(), "Farklı bloklar aggregate edilemez");
    }

    #[test]
    fn test_aggregate_quorum_check() {
        let bh = block_hash(1);
        let votes: Vec<SignedVote> = (1..=4)
            .map(|i| make_signer(i).sign_vote(bh, 1, SlotId(10)).unwrap())
            .collect();

        let agg = AggregateVote::build(&votes).unwrap();
        // 4 / 7 validator → quorum yok (quorum = ceil(14/3) = 5)
        assert!(!agg.has_quorum(7));
        // 4 / 5 validator → quorum var (quorum = ceil(10/3) = 4)
        assert!(agg.has_quorum(5));
    }

    // ─── BftVerifier ──────────────────────────────────────────────────────────

    #[test]
    fn test_bft_verifier_vote() {
        let signer = make_signer(5);
        let bh = block_hash(7);
        let signed = signer.sign_vote(bh, 3, SlotId(30)).unwrap();

        BftVerifier::verify_vote(&signed).unwrap();
    }

    #[test]
    fn test_bft_verifier_aggregate() {
        let bh = block_hash(10);
        let votes: Vec<SignedVote> = (1..=3)
            .map(|i| make_signer(i).sign_vote(bh, 5, SlotId(50)).unwrap())
            .collect();

        let agg = AggregateVote::build(&votes).unwrap();
        BftVerifier::verify_aggregate(&agg).unwrap();
    }

    #[test]
    fn test_bft_verifier_aggregate_and_verify_with_quorum() {
        let bh = block_hash(20);
        let total = 5;
        // Quorum = ceil(10/3) = 4 validator
        let votes: Vec<SignedVote> = (1..=4)
            .map(|i| make_signer(i).sign_vote(bh, 1, SlotId(10)).unwrap())
            .collect();

        let agg = BftVerifier::aggregate_and_verify(&votes, total, true).unwrap();
        assert_eq!(agg.signer_count(), 4);
    }

    #[test]
    fn test_bft_verifier_insufficient_quorum_fails() {
        let bh = block_hash(3);
        let total = 10; // quorum = ceil(20/3) = 7
        // Sadece 3 oy → quorum yok
        let votes: Vec<SignedVote> = (1..=3)
            .map(|i| make_signer(i).sign_vote(bh, 1, SlotId(10)).unwrap())
            .collect();

        let result = BftVerifier::aggregate_and_verify(&votes, total, true);
        assert!(result.is_err(), "Quorum yetersizse hata dönmeli");
    }

    #[test]
    fn test_signing_error_display() {
        let e = SigningError::KeyGen("test".into());
        assert!(!e.to_string().is_empty());

        let e2 = SigningError::InsufficientQuorum { need: 7, have: 3 };
        assert!(e2.to_string().contains("7"));
    }
}
