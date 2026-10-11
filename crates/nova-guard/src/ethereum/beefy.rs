//! BEEFY light client — Polkadot bridges pattern
//!
//! Polkadot `bridges/modules/beefy/src/lib.rs`:
//! - `BeefyCommitment`: block_number, payload (MMR root), validator_set_id
//! - `SignedCommitment`: commitment + signatures
//! - `BeefyAuthoritySet`: id, len, keyset_commitment
//! - `ImportedCommitment`: best_block_number, next_block_number_index
//! - `BeefyLightClient`: initialize, submit_signed_commitment, verify
//!
//! Nova Shield Guard Ethereum köprüsü için:
//! Nova Shield Chain state'i Ethereum'a kanıtlamak amacıyla kullanılır

use crate::errors::{GuardError, GuardResult};
use crate::types::blake3_hash_many;
use serde::{Deserialize, Serialize};
use hex;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Maksimum işlenecek taahhüt sayısı — Polkadot: `CommitmentsToKeep`
pub const MAX_COMMITMENTS: usize = 256;

/// Maksimum istek sayısı — Polkadot: `MaxRequests`
pub const MAX_REQUESTS: u32 = 50;

// ─── BeefyPayload ─────────────────────────────────────────────────────────────

/// BEEFY taahhüt yükü (payload)
/// Polkadot: `Payload = BoundedVec<(PayloadId, BoundedVec<u8>)>`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BeefyPayload {
    /// MMR root hash (nova-shield state'i)
    pub mmr_root: [u8; 32],
    /// Ek veri
    pub extra: Vec<u8>,
}

impl BeefyPayload {
    pub fn new(mmr_root: [u8; 32]) -> Self {
        Self { mmr_root, extra: vec![] }
    }

    pub fn hash(&self) -> [u8; 32] {
        blake3_hash_many(&[b"beefy-payload", &self.mmr_root, &self.extra])
    }
}

// ─── BeefyCommitment ──────────────────────────────────────────────────────────

/// BEEFY taahhüdü — Polkadot `BeefyCommitment`
/// `Commitment { payload, block_number, validator_set_id }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeefyCommitment {
    /// Blok numarası (Nova Shield slot)
    pub block_number: u64,
    /// Validator set ID
    pub validator_set_id: u64,
    /// Payload (MMR root vs.)
    pub payload: BeefyPayload,
}

impl BeefyCommitment {
    pub fn new(block_number: u64, validator_set_id: u64, mmr_root: [u8; 32]) -> Self {
        Self {
            block_number,
            validator_set_id,
            payload: BeefyPayload::new(mmr_root),
        }
    }

    /// Taahhüt hash'i (imzalanacak mesaj)
    pub fn commitment_hash(&self) -> [u8; 32] {
        blake3_hash_many(&[
            b"nova-beefy-commitment-v1",
            &self.block_number.to_le_bytes(),
            &self.validator_set_id.to_le_bytes(),
            &self.payload.hash(),
        ])
    }
}

// ─── BeefySignature ───────────────────────────────────────────────────────────

/// BEEFY imzası — Polkadot: ECDSA (secp256k1) veya BLS
/// Mock: BLAKE3 tabanlı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeefySignature {
    pub validator_index: u32,
    pub signature: [u8; 32],
}

impl BeefySignature {
    pub fn mock_sign(validator_index: u32, commitment_hash: &[u8; 32], key: &[u8]) -> Self {
        let sig = blake3_hash_many(&[b"beefy-sign", commitment_hash, key, &[validator_index as u8]]);
        Self { validator_index, signature: sig }
    }
}

// ─── BeefySignedCommitment ────────────────────────────────────────────────────

/// İmzalanmış BEEFY taahhüdü — Polkadot `SignedCommitment`
/// `SignedCommitment { commitment, signatures: Vec<Option<Signature>> }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeefySignedCommitment {
    pub commitment: BeefyCommitment,
    pub signatures: Vec<BeefySignature>,
}

impl BeefySignedCommitment {
    pub fn new(commitment: BeefyCommitment) -> Self {
        Self { commitment, signatures: vec![] }
    }

    pub fn add_signature(&mut self, sig: BeefySignature) {
        self.signatures.push(sig);
    }

    pub fn signature_count(&self) -> usize {
        self.signatures.len()
    }
}

// ─── BeefyAuthoritySet ────────────────────────────────────────────────────────

/// BEEFY validator set — Polkadot `BeefyAuthoritySet`
/// `BeefyAuthoritySet { id, len, keyset_commitment }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeefyAuthoritySet {
    /// Set kimliği
    pub id: u64,
    /// Validator sayısı
    pub len: u32,
    /// Merkle root (tüm validator public key'lerinin)
    pub keyset_commitment: [u8; 32],
    /// Validator public key'leri (mock)
    pub validators: Vec<[u8; 32]>,
}

impl BeefyAuthoritySet {
    pub fn new(id: u64, validators: Vec<[u8; 32]>) -> Self {
        let len = validators.len() as u32;
        // Keyset commitment: BLAKE3 Merkle mock
        let flat: Vec<u8> = validators.iter().flat_map(|v| v.iter().copied()).collect();
        let keyset_commitment = blake3_hash_many(&[b"beefy-keyset", &(id.to_le_bytes()), &flat]);
        Self { id, len, keyset_commitment, validators }
    }

    pub fn quorum(&self) -> u32 {
        // 2/3 + 1 quorum
        (self.len * 2 / 3) + 1
    }
}

// ─── ImportedCommitment ───────────────────────────────────────────────────────

/// İçe aktarılmış taahhüt — Polkadot `ImportedCommitment`
/// Ethereum tarafında depolanan doğrulanmış Nova Shield state kanıtı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedCommitment {
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub mmr_root: [u8; 32],
    pub validator_set_id: u64,
    pub imported_at: u64,
}

// ─── MmrLeaf ──────────────────────────────────────────────────────────────────

/// MMR yaprağı — Polkadot `BeefyMmrLeaf`
/// `MmrLeaf { version, parent_number_and_hash, beefy_next_authority_set, leaf_extra }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmrLeaf {
    pub block_number: u64,
    pub parent_hash: [u8; 32],
    pub mmr_root: [u8; 32],
    pub beefy_next_authority_set_id: u64,
    pub extra: Vec<u8>,
}

impl MmrLeaf {
    pub fn hash(&self) -> [u8; 32] {
        blake3_hash_many(&[
            b"nova-mmr-leaf-v1",
            &self.block_number.to_le_bytes(),
            &self.parent_hash,
            &self.mmr_root,
            &self.beefy_next_authority_set_id.to_le_bytes(),
        ])
    }
}

// ─── BeefyLightClient ─────────────────────────────────────────────────────────

/// BEEFY light client — Polkadot `pallet_beefy` bridge pallet pattern
/// Ethereum veya başka bir zincirde Nova Shield state'ini doğrular
pub struct BeefyLightClient {
    /// Geçerli validator set
    current_authority_set: BeefyAuthoritySet,
    /// Sonraki validator set (otorite değişimi sırasında)
    next_authority_set: Option<BeefyAuthoritySet>,
    /// En son doğrulanmış blok numarası
    best_block_number: u64,
    /// İçe aktarılmış taahhütler (halka buffer)
    imported: Vec<ImportedCommitment>,
    /// İstek sayacı — Polkadot: RequestCount
    request_count: u32,
    /// NOVA-RED-GUARD-006: Yetkili güncelleme yapıcı (32-byte validator public key).
    /// None ise `update_authority_set` her zaman reddeder.
    authorized_updater: Option<[u8; 32]>,
}

impl BeefyLightClient {
    /// Light client'ı başlat — Polkadot: `initialize(origin, init_data)`
    pub fn initialize(genesis_authority_set: BeefyAuthoritySet) -> Self {
        Self {
            current_authority_set: genesis_authority_set,
            next_authority_set: None,
            best_block_number: 0,
            imported: vec![],
            request_count: 0,
            authorized_updater: None,
        }
    }

    /// Yetkili güncelleme yapıcı kaydet (genesis'te veya governance kararıyla)
    pub fn set_authorized_updater(&mut self, updater_pubkey: [u8; 32]) {
        self.authorized_updater = Some(updater_pubkey);
    }

    /// Validator set güncelle — NOVA-RED-GUARD-006 düzeltmesi.
    ///
    /// Polkadot: `update_authority_set(origin, ...)` yalnızca `T::BridgeOwner` origin'i
    /// tarafından çağrılabilir. Burada `caller_pubkey` ile authorized_updater karşılaştırılır.
    ///
    /// # Hata
    /// `GuardError::UnauthorizedAuthoritySetUpdate` — caller yetkili değilse.
    pub fn update_authority_set(
        &mut self,
        caller_pubkey: &[u8; 32],
        new_set: BeefyAuthoritySet,
    ) -> GuardResult<()> {
        // NOVA-RED-GUARD-006: Yetkisiz caller authority set'i değiştiremez.
        // Eğer authorized_updater ayarlanmamışsa tamamen reddedilir.
        match &self.authorized_updater {
            Some(authorized) if authorized == caller_pubkey => {}
            _ => {
                return Err(GuardError::UnauthorizedAuthoritySetUpdate {
                    caller: hex::encode(caller_pubkey),
                });
            }
        }
        self.next_authority_set = Some(new_set);
        Ok(())
    }

    /// İmzalanmış taahhüdü gönder — Polkadot: `submit_commitment`
    pub fn submit_signed_commitment(
        &mut self,
        signed: BeefySignedCommitment,
    ) -> GuardResult<ImportedCommitment> {
        // İstek limitini kontrol et
        if self.request_count >= MAX_REQUESTS {
            return Err(GuardError::BeefyCommitmentInvalid {
                reason: "request limit aşıldı".into(),
            });
        }

        // Blok numarası ilerlemeli
        if signed.commitment.block_number <= self.best_block_number {
            return Err(GuardError::BeefyCommitmentInvalid {
                reason: format!(
                    "blok numarası geri gidemez: {} <= {}",
                    signed.commitment.block_number, self.best_block_number
                ),
            });
        }

        // Quorum kontrolü
        let quorum = self.current_authority_set.quorum();
        let sig_count = signed.signatures.len() as u32;
        if sig_count < quorum {
            return Err(GuardError::BeefyCommitmentInvalid {
                reason: format!("yetersiz imza: {}/{}", sig_count, quorum),
            });
        }

        // NOVA-RED-GUARD-005: İmzaları validator public key'e göre doğrula.
        // Sıfır kontrolü yerine BLAKE3(commitment_hash, validator_key, index) ile eşleştir.
        let commitment_hash = signed.commitment.commitment_hash();
        for sig in &signed.signatures {
            let idx = sig.validator_index as usize;
            if idx >= self.current_authority_set.validators.len() {
                return Err(GuardError::BeefyCommitmentInvalid {
                    reason: format!("geçersiz validator indeksi: {}", sig.validator_index),
                });
            }
            let validator_key = &self.current_authority_set.validators[idx];
            // BeefySignature::mock_sign ile tutarlı doğrulama
            let expected = blake3_hash_many(&[
                b"beefy-sign",
                &commitment_hash,
                validator_key.as_slice(),
                &[sig.validator_index as u8],
            ]);
            if sig.signature != expected {
                return Err(GuardError::BeefyCommitmentInvalid {
                    reason: format!("geçersiz imza: validator_index={}", sig.validator_index),
                });
            }
        }

        // İçe aktar
        let block_hash = blake3_hash_many(&[
            b"nova-block-hash",
            &signed.commitment.block_number.to_le_bytes(),
            &commitment_hash,
        ]);

        let imported = ImportedCommitment {
            block_number: signed.commitment.block_number,
            block_hash,
            mmr_root: signed.commitment.payload.mmr_root,
            validator_set_id: signed.commitment.validator_set_id,
            imported_at: 0,
        };

        self.best_block_number = signed.commitment.block_number;
        self.request_count += 1;

        // Buffer'ı temizle
        if self.imported.len() >= MAX_COMMITMENTS {
            self.imported.remove(0);
        }
        self.imported.push(imported.clone());

        Ok(imported)
    }

    /// Belirli blok doğrulanmış mı?
    pub fn is_block_finalized(&self, block_number: u64) -> bool {
        self.imported.iter().any(|c| c.block_number == block_number)
    }

    pub fn best_block(&self) -> u64 {
        self.best_block_number
    }

    pub fn imported_count(&self) -> usize {
        self.imported.len()
    }

    pub fn current_validator_set(&self) -> &BeefyAuthoritySet {
        &self.current_authority_set
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_authority_set(n: u32) -> BeefyAuthoritySet {
        let validators: Vec<[u8; 32]> = (0..n).map(|i| [i as u8; 32]).collect();
        BeefyAuthoritySet::new(1, validators)
    }

    fn make_signed_commitment(
        block_number: u64,
        authority_set: &BeefyAuthoritySet,
        sig_count: usize,
    ) -> BeefySignedCommitment {
        let commitment = BeefyCommitment::new(block_number, authority_set.id, [0xAAu8; 32]);
        let hash = commitment.commitment_hash();
        let mut signed = BeefySignedCommitment::new(commitment);
        for i in 0..sig_count {
            let sig = BeefySignature::mock_sign(i as u32, &hash, &[i as u8; 32]);
            signed.add_signature(sig);
        }
        signed
    }

    #[test]
    fn test_beefy_authority_set_quorum() {
        let set = make_authority_set(10);
        assert_eq!(set.quorum(), 7); // 10*2/3+1 = 7
    }

    #[test]
    fn test_beefy_commitment_hash() {
        let c = BeefyCommitment::new(100, 1, [0xBBu8; 32]);
        let h = c.commitment_hash();
        assert_ne!(h, [0u8; 32]);
        assert_eq!(h, c.commitment_hash()); // deterministic
    }

    #[test]
    fn test_beefy_light_client_initialize() {
        let set = make_authority_set(5);
        let lc = BeefyLightClient::initialize(set);
        assert_eq!(lc.best_block(), 0);
        assert_eq!(lc.imported_count(), 0);
    }

    #[test]
    fn test_beefy_update_authority_set_authorized() {
        let set = make_authority_set(5);
        let mut lc = BeefyLightClient::initialize(set.clone());
        let admin_key = [0xADu8; 32];
        lc.set_authorized_updater(admin_key);
        let new_set = make_authority_set(7);
        assert!(lc.update_authority_set(&admin_key, new_set).is_ok());
        assert!(lc.next_authority_set.is_some());
    }

    #[test]
    fn test_beefy_update_authority_set_unauthorized() {
        let set = make_authority_set(5);
        let mut lc = BeefyLightClient::initialize(set.clone());
        let admin_key = [0xADu8; 32];
        lc.set_authorized_updater(admin_key);
        // Farklı bir key ile deneme
        let attacker_key = [0xBAu8; 32];
        let new_set = make_authority_set(7);
        let err = lc.update_authority_set(&attacker_key, new_set).unwrap_err();
        assert!(matches!(err, GuardError::UnauthorizedAuthoritySetUpdate { .. }));
    }

    #[test]
    fn test_beefy_update_authority_set_no_updater_set() {
        let set = make_authority_set(5);
        let mut lc = BeefyLightClient::initialize(set.clone());
        // authorized_updater ayarlanmamış → herkes reddedilir
        let err = lc.update_authority_set(&[0xAAu8; 32], make_authority_set(7)).unwrap_err();
        assert!(matches!(err, GuardError::UnauthorizedAuthoritySetUpdate { .. }));
    }

    #[test]
    fn test_beefy_submit_commitment_success() {
        let set = make_authority_set(5);
        let quorum = set.quorum() as usize; // 4
        let mut lc = BeefyLightClient::initialize(set.clone());

        let signed = make_signed_commitment(100, &set, quorum);
        let imported = lc.submit_signed_commitment(signed).unwrap();

        assert_eq!(imported.block_number, 100);
        assert_eq!(lc.best_block(), 100);
        assert!(lc.is_block_finalized(100));
    }

    #[test]
    fn test_beefy_submit_commitment_insufficient_signatures() {
        let set = make_authority_set(5);
        let mut lc = BeefyLightClient::initialize(set.clone());

        // Quorum=4, sadece 2 imza
        let signed = make_signed_commitment(100, &set, 2);
        let result = lc.submit_signed_commitment(signed);

        assert!(matches!(result, Err(GuardError::BeefyCommitmentInvalid { .. })));
    }

    #[test]
    fn test_beefy_submit_commitment_block_not_advancing() {
        let set = make_authority_set(5);
        let quorum = set.quorum() as usize;
        let mut lc = BeefyLightClient::initialize(set.clone());

        // İlk taahhüt
        let s1 = make_signed_commitment(100, &set, quorum);
        lc.submit_signed_commitment(s1).unwrap();

        // Aynı blok numarası → hata
        let s2 = make_signed_commitment(100, &set, quorum);
        let result = lc.submit_signed_commitment(s2);
        assert!(matches!(result, Err(GuardError::BeefyCommitmentInvalid { .. })));
    }

    #[test]
    fn test_beefy_mmr_leaf_hash() {
        let leaf = MmrLeaf {
            block_number: 42,
            parent_hash: [0x01u8; 32],
            mmr_root: [0x02u8; 32],
            beefy_next_authority_set_id: 1,
            extra: vec![],
        };
        let h = leaf.hash();
        assert_ne!(h, [0u8; 32]);
        assert_eq!(h, leaf.hash());
    }

    #[test]
    fn test_beefy_is_block_finalized() {
        let set = make_authority_set(5);
        let quorum = set.quorum() as usize;
        let mut lc = BeefyLightClient::initialize(set.clone());

        assert!(!lc.is_block_finalized(50));

        let signed = make_signed_commitment(50, &set, quorum);
        lc.submit_signed_commitment(signed).unwrap();
        assert!(lc.is_block_finalized(50));
    }
}
