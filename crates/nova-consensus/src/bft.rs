//! Simplex BFT motoru — blok önerme, oylama, QC toplama (§5.10)
//!
//! Araştırma kaynakları:
//! - Avalanchego simplex `Engine`:
//!   `epoch *simplex.Epoch`, `blockTracker`, `SignatureAggregator`.
//!   `NewEngine(ctx, config)` → init pattern.
//!   Bizde: `BftEngine::new(validator_set)`.
//! - Avalanchego simplex `SignatureAggregator::Aggregate(signatures)`:
//!   `quorumSize = simplex.Quorum(len(nodeID2PK))`
//!   Filtered, deduplicated, BLS aggregate.
//!   Bizde: `BftEngine::aggregate_votes` → mock aggregate.
//! - Avalanchego simplex `blockTracker::verifyAndTrackBlock`:
//!   Block verify → SetPreference → track.
//!   Bizde: `BftEngine::verify_block` → parent hash + height kontrolü.
//! - Avalanchego simplex `QC::Verify(msg)`:
//!   `quorum = Quorum(len)` signers → AggPK aggregate → verify.
//!   Bizde: `BftEngine::verify_qc` → signer count + mock signature verify.
//! - Polkadot GRANDPA `finalize_block(block_hash, round_number)`:
//!   Round tabanlı finality. Bizde: slot-based single-slot finality.

use crate::errors::{ConsensusError, ConsensusResult};
use crate::signing::{vote_message, AggregateVote, BftSigner, SignedVote};
use crate::types::{Block, BlockHeader, QuorumCertificate, ValidatorStatus, quorum_threshold};
use dashmap::DashMap;
use nova_crypto::BlsVerifier;
use nova_primitives::{BlsPublicKey, BlsSignature, BlockHash, EpochId, SlotId, ValidatorId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

// ─── Vote ─────────────────────────────────────────────────────────────────────

/// Bir validator'ın blok için verdiği oy.
///
/// Pattern: Avalanchego simplex `Signature { Signer, Value }`.
#[derive(Debug, Clone)]
pub struct Vote {
    /// Oy veren validator.
    pub validator: ValidatorId,
    /// Oy verilen blok hash'i.
    pub block_hash: BlockHash,
    /// Oy verilen yükseklik.
    pub height: u64,
    /// Oy imzası (mock: blake3(block_hash || validator || height)).
    pub signature: [u8; 64],
    /// Oyun verildiği slot.
    pub slot: SlotId,
}

impl Vote {
    /// Mock vote oluştur (test için).
    #[cfg(test)]
    pub fn mock(validator: ValidatorId, block_hash: BlockHash, height: u64, slot: SlotId) -> Self {
        let mut sig = [0u8; 64];
        let mut hasher = blake3::Hasher::new();
        hasher.update(block_hash.as_bytes());
        hasher.update(&validator.0.to_le_bytes());
        hasher.update(&height.to_le_bytes());
        let hash = hasher.finalize();
        sig[..32].copy_from_slice(hash.as_bytes());
        Self { validator, block_hash, height, signature: sig, slot }
    }
}

// ─── VoteCollector ────────────────────────────────────────────────────────────

/// Belirli bir yükseklik için oy toplama.
///
/// Pattern: Avalanchego simplex `SignatureAggregator` — votes → QC.
struct VoteCollector {
    /// Oy verilen yükseklik.
    height: u64,
    /// Toplanan mock oylar (validator → vote) — test/genel path.
    votes: HashMap<ValidatorId, Vote>,
    /// Gerçek BLS SignedVote'lar (validator → SignedVote) — üretim path.
    signed_votes: HashMap<ValidatorId, SignedVote>,
    /// Toplam aktif validator sayısı (quorum hesabı için).
    total_validators: usize,
}

impl VoteCollector {
    fn new(height: u64, total_validators: usize) -> Self {
        Self {
            height,
            votes: HashMap::new(),
            signed_votes: HashMap::new(),
            total_validators,
        }
    }

    /// Oy ekle. Return: quorum sağlandı mı?
    fn add_vote(&mut self, vote: Vote) -> Result<bool, ConsensusError> {
        if vote.height != self.height {
            return Err(ConsensusError::HeightMismatch {
                expected: self.height,
                got: vote.height,
            });
        }

        if self.votes.contains_key(&vote.validator) {
            return Err(ConsensusError::DuplicateSigner {
                validator_id: vote.validator.0.to_string(),
            });
        }

        self.votes.insert(vote.validator, vote);
        let quorum = quorum_threshold(self.total_validators);
        Ok(self.votes.len() >= quorum)
    }

    /// Quorum sağlandı mı?
    fn has_quorum(&self) -> bool {
        let quorum = quorum_threshold(self.total_validators);
        self.votes.len() >= quorum
    }

    /// Mock oy sayısı.
    fn vote_count(&self) -> usize {
        self.votes.len()
    }

    /// BLS SignedVote ekle. Return: quorum sağlandı mı?
    ///
    /// BLS imza doğrulaması `BftEngine::process_signed_vote`'da yapılır,
    /// burada sadece kayıt ve quorum kontrolü.
    fn add_signed_vote(&mut self, vote: SignedVote) -> Result<bool, ConsensusError> {
        if vote.height != self.height {
            return Err(ConsensusError::HeightMismatch {
                expected: self.height,
                got: vote.height,
            });
        }
        if self.signed_votes.contains_key(&vote.validator_id) {
            return Err(ConsensusError::DuplicateSigner {
                validator_id: vote.validator_id.0.to_string(),
            });
        }
        self.signed_votes.insert(vote.validator_id, vote);
        let quorum = quorum_threshold(self.total_validators);
        Ok(self.signed_votes.len() >= quorum)
    }

    /// BLS oy sayısı.
    fn signed_vote_count(&self) -> usize {
        self.signed_votes.len()
    }

    /// Gerçek BLS QC üret (signed_votes quorumu varsa).
    ///
    /// `AggregateVote::build()` → sıraya göre imzaları serialize et.
    /// Format: `[sig1[96] ++ sig2[96] ++ ...]` (signer sırasıyla eşleşir).
    ///
    /// Pattern: Ethereum `BeaconBlock::aggregate_attestations`.
    fn try_produce_qc_bls(&self, block_hash: BlockHash, slot: SlotId) -> Option<QuorumCertificate> {
        let quorum = quorum_threshold(self.total_validators);
        if self.signed_votes.len() < quorum {
            return None;
        }
        let votes_owned: Vec<SignedVote> = self.signed_votes.values().cloned().collect();
        let agg = AggregateVote::build(&votes_owned).ok()?;

        // Signer sırasıyla 96-byte BLS imzaları birleştir
        let mut agg_bytes = Vec::with_capacity(agg.signatures.len() * 96);
        for sig in &agg.signatures {
            agg_bytes.extend_from_slice(sig.as_bytes());
        }

        Some(QuorumCertificate {
            block_hash,
            height: self.height,
            signers: agg.signers,
            aggregate_signature: agg_bytes,
            signed_at_slot: slot,
        })
    }

    /// QC üret (quorum varsa).
    ///
    /// Önce BLS signed_votes dener (üretim path), yoksa mock'a düşer (test path).
    /// Avalanchego `SignatureAggregator::Aggregate` pattern.
    fn try_produce_qc(&self, block_hash: BlockHash, slot: SlotId) -> Option<QuorumCertificate> {
        // Üretim: BLS signed oylar quorum sağladıysa → gerçek QC
        if self.signed_votes.len() >= quorum_threshold(self.total_validators) {
            return self.try_produce_qc_bls(block_hash, slot);
        }

        // Mock fallback (mevcut testler için)
        if !self.has_quorum() {
            return None;
        }

        let signers: Vec<ValidatorId> = self.votes.keys().cloned().collect();
        let mut hasher = blake3::Hasher::new();
        hasher.update(block_hash.as_bytes());
        hasher.update(&self.height.to_le_bytes());
        for (validator, vote) in &self.votes {
            hasher.update(&validator.0.to_le_bytes());
            hasher.update(&vote.signature);
        }
        let hash = hasher.finalize();
        let mut aggregate = vec![0u8; 96];
        aggregate[..32].copy_from_slice(hash.as_bytes());

        Some(QuorumCertificate {
            block_hash,
            height: self.height,
            signers,
            aggregate_signature: aggregate,
            signed_at_slot: slot,
        })
    }
}

// ─── BftEngine ────────────────────────────────────────────────────────────────

/// Simplex BFT konsensüs motoru.
///
/// Sorumluluklar:
/// 1. Blok doğrulama (parent hash + height zinciri)
/// 2. Oy toplama → QC üretme
/// 3. Blok finalizasyon kararı
///
/// Pattern: Avalanchego simplex `Engine { epoch, blockTracker, signatureAggregator }`.
pub struct BftEngine {
    /// Validator seti (ID → BLS public key, None = pubkey bilinmiyor).
    validator_set: Arc<DashMap<ValidatorId, Option<BlsPublicKey>>>,
    /// Toplam validator sayısı (quorum hesabı için).
    validator_count: usize,
    /// Aktif oy kolektörleri (yükseklik → VoteCollector).
    vote_collectors: Arc<RwLock<HashMap<u64, VoteCollector>>>,
    /// Doğrulanmış bloklar (hash → Block).
    /// Avalanchego blockTracker `simplexDigestsToBlock`.
    verified_blocks: Arc<DashMap<BlockHash, Block>>,
    /// Son finalize yükseklik.
    last_finalized_height: Arc<RwLock<u64>>,
    /// Bu node'un BLS signer'ı — validator ise `Some`, gözlemci ise `None`.
    ///
    /// Pattern: Polkadot `sc-keystore::LocalKeystore` — node başlangıcında ayarlanır.
    signer: Option<Arc<BftSigner>>,
}

impl BftEngine {
    /// Yeni BFT motoru oluştur (validator ID'leri, pubkey bilinmiyor).
    /// Test ve geçici yapılandırma için. QC BLS doğrulaması atlanır.
    pub fn new(active_validators: Vec<ValidatorId>) -> Self {
        let validator_count = active_validators.len();
        let validator_set = Arc::new(DashMap::new());
        for vid in active_validators {
            validator_set.insert(vid, None);
        }

        Self {
            validator_set,
            validator_count,
            vote_collectors: Arc::new(RwLock::new(HashMap::new())),
            verified_blocks: Arc::new(DashMap::new()),
            last_finalized_height: Arc::new(RwLock::new(0)),
            signer: None,
        }
    }

    /// Validator BLS public key'leri ile motor oluştur (üretim path).
    ///
    /// verify_qc() bu pubkey'leri kullanarak aggregate BLS imzasını doğrular.
    /// Pattern: Ethereum beacon chain `BeaconState.validators[].pubkey`.
    pub fn new_with_pubkeys(validators: Vec<(ValidatorId, BlsPublicKey)>) -> Self {
        let validator_count = validators.len();
        let validator_set = Arc::new(DashMap::new());
        for (vid, pk) in validators {
            validator_set.insert(vid, Some(pk));
        }
        Self {
            validator_set,
            validator_count,
            vote_collectors: Arc::new(RwLock::new(HashMap::new())),
            verified_blocks: Arc::new(DashMap::new()),
            last_finalized_height: Arc::new(RwLock::new(0)),
            signer: None,
        }
    }

    /// Validator'ın BLS public key'ini kaydet (geç yapılandırma).
    pub fn register_pubkey(&self, vid: ValidatorId, pubkey: BlsPublicKey) {
        self.validator_set.insert(vid, Some(pubkey));
    }

    /// BLS signer ile motor oluştur (validator node için).
    ///
    /// Pattern: Polkadot `sc-consensus-grandpa::GrandpaParams { keystore }`.
    pub fn new_with_signer(active_validators: Vec<ValidatorId>, signer: BftSigner) -> Self {
        let mut engine = Self::new(active_validators);
        engine.signer = Some(Arc::new(signer));
        engine
    }

    /// Signer'ı sonradan ayarla (aşamalı yapılandırma için).
    pub fn set_signer(&mut self, signer: BftSigner) {
        self.signer = Some(Arc::new(signer));
    }

    /// Bu motor'un BLS signer'ı var mı?
    pub fn has_signer(&self) -> bool {
        self.signer.is_some()
    }

    /// Validator setini güncelle (epoch geçişinde).
    pub fn update_validator_set(&mut self, new_validators: Vec<ValidatorId>) {
        self.validator_set.clear();
        self.validator_count = new_validators.len();
        for vid in new_validators {
            self.validator_set.insert(vid, None);
        }
    }

    // ─── Blok Doğrulama ───────────────────────────────────────────────────

    /// Bloğu doğrula ve takip et.
    ///
    /// Pattern: Avalanchego simplex `Block::Verify(ctx)`:
    /// - Genesis bloğu doğrulanmaz
    /// - Parent hash mevcut ve eşleşiyor mu?
    /// - Yükseklik artımlı mı?
    pub fn verify_block(&self, block: &Block) -> ConsensusResult<()> {
        // Genesis bloğu her zaman geçerli
        if block.header.is_genesis() {
            self.verified_blocks.insert(block.block_hash, block.clone());
            return Ok(());
        }

        // Hash bütünlüğü
        if !block.verify_hash() {
            return Err(ConsensusError::InvalidBlockHash);
        }

        // Finalize sonrası blok mu?
        let last_finalized = *self.last_finalized_height.read();
        if block.header.height <= last_finalized && !block.header.is_genesis() {
            return Err(ConsensusError::AlreadyFinalized {
                height: block.header.height,
            });
        }

        // Parent hash doğrulama
        // Avalanchego: `verifyParentMatchesPrevBlock` → prev block'un digest'i ile karşılaştır.
        // Bizde: parent_hash == verified_blocks'daki ilgili bloğun block_hash'i
        let parent_exists = self
            .verified_blocks
            .iter()
            .any(|e| e.block_hash == block.header.parent_hash);

        if !parent_exists {
            return Err(ConsensusError::InvalidParentHash);
        }

        self.verified_blocks.insert(block.block_hash, block.clone());

        debug!(
            target: "nova_consensus::bft",
            height = block.header.height,
            hash = %hex::encode(block.block_hash.as_bytes()),
            "blok doğrulandı ve takibe alındı"
        );

        Ok(())
    }

    // ─── Oy İşleme ────────────────────────────────────────────────────────

    /// Oy işle.
    ///
    /// Pattern: Avalanchego simplex `comm.go` inbound message işleme.
    /// Return: QC üretildiyse Some(QC), henüz quorum yoksa None.
    pub fn process_vote(
        &self,
        vote: Vote,
        current_slot: SlotId,
    ) -> ConsensusResult<Option<QuorumCertificate>> {
        // Validator yetkili mi?
        if !self.validator_set.contains_key(&vote.validator) {
            return Err(ConsensusError::ValidatorNotFound {
                validator_id: vote.validator.0.to_string(),
            });
        }

        let height = vote.height;
        let block_hash = vote.block_hash;

        let mut collectors = self.vote_collectors.write();
        let collector = collectors
            .entry(height)
            .or_insert_with(|| VoteCollector::new(height, self.validator_count));

        let quorum_reached = collector.add_vote(vote)?;
        let vote_count = collector.vote_count();

        debug!(
            target: "nova_consensus::bft",
            height = height,
            votes = vote_count,
            quorum = quorum_threshold(self.validator_count),
            "oy işlendi"
        );

        if quorum_reached {
            let qc = collector.try_produce_qc(block_hash, current_slot);
            return Ok(qc);
        }

        Ok(None)
    }

    // ─── QC Doğrulama ─────────────────────────────────────────────────────

    /// QC'yi doğrula.
    ///
    /// Pattern: Avalanchego simplex `QC::Verify(msg)`:
    /// 1. Signer sayısı quorum >= ceil(2n/3)
    /// 2. Duplicate signer yok
    /// 3. Tüm signerlar validator setinde
    /// 4. Aggregate signature geçerli (mock: signer sayısı kontrolü yeterli)
    pub fn verify_qc(&self, qc: &QuorumCertificate) -> ConsensusResult<()> {
        let required = quorum_threshold(self.validator_count);

        if qc.signers.len() < required {
            return Err(ConsensusError::QuorumNotReached {
                required,
                got: qc.signers.len(),
            });
        }

        // 1. Duplicate signer kontrolü (Avalanchego: `uniqueSigners` set)
        let mut seen = std::collections::HashSet::new();
        for signer in &qc.signers {
            if !seen.insert(*signer) {
                return Err(ConsensusError::DuplicateSigner {
                    validator_id: signer.0.to_string(),
                });
            }
            // Validator setinde mi?
            if !self.validator_set.contains_key(signer) {
                return Err(ConsensusError::ValidatorNotFound {
                    validator_id: signer.0.to_string(),
                });
            }
        }

        // 2. Gerçek BLS aggregate imza doğrulama (NOVA-RED-CONSENSUS-002 düzeltmesi)
        //
        // HATA: Önceki kod `if len == expected` kullanıyordu → uzunluk yanlışsa
        // tüm BLS doğrulama ATLANIYORDU. Boş veya yanlış boyutlu imza QC'yi geçiyordu.
        //
        // DÜZELTME: Uzunluk yanlışsa doğrudan hata döndür; BLS doğrulama zorunludur.
        let expected_bls_len = qc.signers.len() * 96;
        if qc.aggregate_signature.len() != expected_bls_len {
            return Err(ConsensusError::Internal(format!(
                "QC imza boyutu yanlış: beklenen {} byte ({} imzalayan × 96), gelen {} byte",
                expected_bls_len,
                qc.signers.len(),
                qc.aggregate_signature.len()
            )));
        }

        // Tüm signers için pubkey mevcut mu?
        let pubkeys: Vec<BlsPublicKey> = qc.signers.iter()
            .filter_map(|vid| {
                self.validator_set.get(vid)
                    .and_then(|entry| entry.clone())
            })
            .collect();

        if pubkeys.len() != qc.signers.len() {
            return Err(ConsensusError::Internal(
                "bazı imzalayanların BLS public key'i bulunamadı".to_string()
            ));
        }

        // Her signer kendi validator_id'sini içeren mesajı imzaladı
        let messages: Vec<[u8; 32]> = qc.signers.iter()
            .map(|&vid| vote_message(qc.block_hash, vid, qc.height, qc.signed_at_slot))
            .collect();

        // Her signer için 96-byte imzayı çıkar
        let sigs: Vec<BlsSignature> = qc.signers.iter().enumerate()
            .map(|(i, _)| {
                let start = i * 96;
                let bytes: [u8; 96] = qc.aggregate_signature[start..start + 96]
                    .try_into()
                    .expect("boyut daha önce doğrulandı");
                BlsSignature::from_bytes(bytes)
            })
            .collect();

        let msg_refs: Vec<&[u8]> = messages.iter().map(|m| m.as_slice()).collect();
        let sig_refs: Vec<&BlsSignature> = sigs.iter().collect();
        let pk_refs: Vec<&BlsPublicKey> = pubkeys.iter().collect();

        BlsVerifier::verify_aggregate(&msg_refs, &sig_refs, &pk_refs)
            .map_err(|e| ConsensusError::Internal(
                format!("BLS aggregate QC doğrulaması başarısız: {}", e)
            ))?;

        debug!(
            target: "nova_consensus::bft",
            signers = qc.signers.len(),
            "BLS aggregate QC doğrulandı"
        );

        Ok(())
    }

    // ─── Finalizasyon ─────────────────────────────────────────────────────

    /// Bloğu finalize et (QC ile).
    ///
    /// Pattern: Avalanchego simplex `blockTracker::indexBlock(ctx, digest)`:
    /// Accept bloğu, reject rekabet eden blokları.
    /// Polkadot GRANDPA `finalize_block(block_hash)`.
    pub fn finalize_block(
        &self,
        block_hash: BlockHash,
        qc: &QuorumCertificate,
        current_slot: SlotId,
    ) -> ConsensusResult<Block> {
        // QC geçerli mi?
        self.verify_qc(qc)?;

        // QC'deki hash ile eşleşiyor mu?
        if qc.block_hash != block_hash {
            return Err(ConsensusError::InvalidBlockHash);
        }

        // Blok doğrulanmış mı?
        let block = self
            .verified_blocks
            .get(&block_hash)
            .ok_or(ConsensusError::BlockNotFound {
                height: qc.height,
            })?
            .clone();

        // Zaten finalize edilmiş mi?
        if block.header.height <= *self.last_finalized_height.read()
            && !block.header.is_genesis()
        {
            return Err(ConsensusError::AlreadyFinalized {
                height: block.header.height,
            });
        }

        // Son finalize yüksekliği güncelle
        *self.last_finalized_height.write() = block.header.height;

        // Oy kolektörünü temizle
        self.vote_collectors.write().remove(&block.header.height);

        info!(
            target: "nova_consensus::bft",
            height = block.header.height,
            hash = %hex::encode(block_hash.as_bytes()),
            signers = qc.signers.len(),
            "blok finalize edildi"
        );

        Ok(block)
    }

    // ─── BLS İmzalama / Üretim ────────────────────────────────────────────

    /// Bu validator'ın blok hash'ini BLS ile imzala ve oyu işle.
    ///
    /// Validator node'un kendi oyunu üretmesi için kullanılır.
    /// Pattern: Ethereum `AttestationService::produce_and_sign_attestation`.
    ///
    /// Return: quorum sağlandıysa `Some(QC)`, henüz yeterliyse `None`.
    pub fn sign_and_process_vote(
        &self,
        block_hash: BlockHash,
        height: u64,
        current_slot: SlotId,
    ) -> ConsensusResult<Option<QuorumCertificate>> {
        let signer = self.signer.as_ref().ok_or_else(|| {
            ConsensusError::Internal("BFT signer yapılandırılmamış".into())
        })?;

        let signed_vote = signer
            .sign_vote(block_hash, height, current_slot)
            .map_err(|e| ConsensusError::Internal(e.to_string()))?;

        self.process_signed_vote(signed_vote, current_slot)
    }

    /// Ağdan alınan BLS `SignedVote`'u doğrula ve işle.
    ///
    /// 1. BLS imza doğrulaması (blst verify)
    /// 2. Validator set kontrolü
    /// 3. VoteCollector'a ekle → quorum kontrolü
    /// 4. Quorum → BLS QC üret
    ///
    /// Pattern: Ethereum `AttestationPool::insert_verified_attestation`.
    pub fn process_signed_vote(
        &self,
        signed_vote: SignedVote,
        current_slot: SlotId,
    ) -> ConsensusResult<Option<QuorumCertificate>> {
        // BLS imza doğrulaması — saldırı vektörü: sahte imzaları filtrele
        signed_vote
            .verify()
            .map_err(|e| ConsensusError::Internal(e.to_string()))?;

        // Validator yetkili mi?
        if !self.validator_set.contains_key(&signed_vote.validator_id) {
            return Err(ConsensusError::ValidatorNotFound {
                validator_id: signed_vote.validator_id.0.to_string(),
            });
        }

        // BLS pubkey'i validator_set'e kaydet — verify_qc'nin ilerleyen doğrulamasında kullanılır.
        // (SignedVote imzası zaten doğrulandı; pubkey güvenilirdir.)
        self.validator_set.insert(signed_vote.validator_id, Some(signed_vote.bls_pubkey));

        let height = signed_vote.height;
        let block_hash = signed_vote.block_hash;

        let mut collectors = self.vote_collectors.write();
        let collector = collectors
            .entry(height)
            .or_insert_with(|| VoteCollector::new(height, self.validator_count));

        let quorum_reached = collector.add_signed_vote(signed_vote)?;
        let count = collector.signed_vote_count();

        debug!(
            target: "nova_consensus::bft",
            height = height,
            signed_votes = count,
            quorum = quorum_threshold(self.validator_count),
            "BLS oy işlendi"
        );

        if quorum_reached {
            let qc = collector.try_produce_qc_bls(block_hash, current_slot);
            return Ok(qc);
        }

        Ok(None)
    }

    // ─── Sorgular ─────────────────────────────────────────────────────────

    /// Son finalize yükseklik.
    pub fn last_finalized_height(&self) -> u64 {
        *self.last_finalized_height.read()
    }

    /// Belirli bir yükseklik için mevcut oy sayısı.
    pub fn vote_count_for_height(&self, height: u64) -> usize {
        self.vote_collectors
            .read()
            .get(&height)
            .map_or(0, |c| c.vote_count())
    }

    /// Validator bu blok setinde aktif mi?
    pub fn is_validator(&self, vid: &ValidatorId) -> bool {
        self.validator_set.contains_key(vid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};

    fn vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn bh(seed: u8) -> BlockHash {
        BlockHash::from_bytes([seed; 32])
    }

    fn make_engine(n: u8) -> BftEngine {
        let validators: Vec<ValidatorId> = (1..=n).map(vid).collect();
        BftEngine::new(validators)
    }

    /// Deterministik BLS IKM: validator i için
    fn bls_ikm_for(i: u8) -> [u8; 32] {
        [(i as u8).wrapping_mul(37); 32]
    }

    /// BLS pubkey'leri kayıtlı engine — verify_qc için gerekli
    fn make_engine_with_bls(n: u8) -> BftEngine {
        use nova_crypto::BlsKeypair;
        let validators: Vec<(ValidatorId, BlsPublicKey)> = (1..=n)
            .map(|i| {
                let ikm = bls_ikm_for(i);
                let kp = BlsKeypair::from_ikm(&ikm).unwrap();
                (vid(i), kp.bls_public_key())
            })
            .collect();
        BftEngine::new_with_pubkeys(validators)
    }

    /// Gerçek BLS imzalı QuorumCertificate (verify_qc testleri için)
    fn make_bls_qc(block_hash: BlockHash, height: u64, signer_ids: &[u8], slot: SlotId) -> QuorumCertificate {
        use nova_crypto::BlsKeypair;
        let signers: Vec<ValidatorId> = signer_ids.iter().map(|&i| vid(i)).collect();
        let mut agg_bytes = Vec::new();
        for &i in signer_ids {
            let ikm = bls_ikm_for(i);
            let kp = BlsKeypair::from_ikm(&ikm).unwrap();
            let msg = vote_message(block_hash, vid(i), height, slot);
            let sig = kp.sign_raw(&msg);
            agg_bytes.extend_from_slice(sig.as_bytes());
        }
        QuorumCertificate {
            block_hash,
            height,
            signers,
            aggregate_signature: agg_bytes,
            signed_at_slot: slot,
        }
    }

    fn make_genesis_block() -> Block {
        let header = BlockHeader {
            height: 0,
            slot: SlotId(0),
            epoch: EpochId(0),
            parent_hash: bh(0),
            state_root: [0u8; 32],
            proposer: vid(1),
            timestamp: 0,
            tx_count: 0,
        };
        Block::new(header)
    }

    fn make_block(height: u64, parent_hash: BlockHash, slot: SlotId) -> Block {
        let header = BlockHeader {
            height,
            slot,
            epoch: EpochId(0),
            parent_hash,
            state_root: [height as u8; 32],
            proposer: vid(1),
            timestamp: slot.0 * 400,
            tx_count: 5,
        };
        Block::new(header)
    }

    // ─── verify_block ─────────────────────────────────────────────────────

    #[test]
    fn test_verify_genesis_block() {
        let engine = make_engine(5);
        let genesis = make_genesis_block();
        assert!(engine.verify_block(&genesis).is_ok());
    }

    #[test]
    fn test_verify_block_valid_chain() {
        let engine = make_engine(5);
        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();

        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        assert!(engine.verify_block(&block1).is_ok());
    }

    #[test]
    fn test_verify_block_invalid_parent() {
        let engine = make_engine(5);
        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();

        // Parent hash yanlış
        let bad_block = make_block(1, bh(99), SlotId(400));
        let err = engine.verify_block(&bad_block).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidParentHash));
    }

    #[test]
    fn test_verify_block_tampered_hash_fails() {
        let engine = make_engine(5);
        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();

        let mut block = make_block(1, genesis.block_hash, SlotId(400));
        block.block_hash = bh(99); // Sahte hash

        let err = engine.verify_block(&block).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidBlockHash));
    }

    // ─── process_vote ──────────────────────────────────────────────────────

    #[test]
    fn test_process_votes_no_quorum_yet() {
        // 5 validator, quorum = ceil(10/3) = 4
        let engine = make_engine(5);
        let block_hash = bh(1);
        let height = 1u64;

        for i in 1u8..=3 {
            let vote = Vote::mock(vid(i), block_hash, height, SlotId(400));
            let qc = engine.process_vote(vote, SlotId(400)).unwrap();
            assert!(qc.is_none(), "3 oy yeterli değil (quorum=4)");
        }
    }

    #[test]
    fn test_process_votes_produces_qc_at_quorum() {
        // 5 validator, quorum = 4
        let engine = make_engine(5);
        let block_hash = bh(1);
        let height = 1u64;

        let mut qc = None;
        for i in 1u8..=4 {
            let vote = Vote::mock(vid(i), block_hash, height, SlotId(400));
            qc = engine.process_vote(vote, SlotId(400)).unwrap();
        }
        assert!(qc.is_some(), "4 oy quorum sağlamalı");
        let qc = qc.unwrap();
        assert_eq!(qc.height, height);
        assert_eq!(qc.signers.len(), 4);
    }

    #[test]
    fn test_process_vote_duplicate_fails() {
        let engine = make_engine(5);
        let vote1 = Vote::mock(vid(1), bh(1), 1, SlotId(400));
        let vote2 = Vote::mock(vid(1), bh(1), 1, SlotId(400));

        engine.process_vote(vote1, SlotId(400)).unwrap();
        let err = engine.process_vote(vote2, SlotId(400)).unwrap_err();
        assert!(matches!(err, ConsensusError::DuplicateSigner { .. }));
    }

    #[test]
    fn test_process_vote_unauthorized_validator_fails() {
        let engine = make_engine(5);
        // vid(99) set'te yok
        let vote = Vote::mock(vid(99), bh(1), 1, SlotId(400));
        let err = engine.process_vote(vote, SlotId(400)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorNotFound { .. }));
    }

    // ─── verify_qc ────────────────────────────────────────────────────────

    #[test]
    fn test_verify_qc_valid() {
        // 5 validator, quorum = 4 — gerçek BLS imzalı QC
        let engine = make_engine_with_bls(5);
        let qc = make_bls_qc(bh(1), 1, &[1, 2, 3, 4], SlotId(400));
        assert!(engine.verify_qc(&qc).is_ok());
    }

    #[test]
    fn test_verify_qc_insufficient_signers() {
        let engine = make_engine(5);
        let signers: Vec<ValidatorId> = (1..=3).map(vid).collect(); // quorum=4
        let qc = QuorumCertificate::mock_valid(bh(1), 1, signers, SlotId(400));
        let err = engine.verify_qc(&qc).unwrap_err();
        assert!(matches!(err, ConsensusError::QuorumNotReached { required: 4, got: 3 }));
    }

    #[test]
    fn test_verify_qc_unknown_signer() {
        let engine = make_engine(5);
        // vid(99) set'te yok
        let signers = vec![vid(1), vid(2), vid(3), vid(99)];
        let qc = QuorumCertificate::mock_valid(bh(1), 1, signers, SlotId(400));
        let err = engine.verify_qc(&qc).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorNotFound { .. }));
    }

    // ─── finalize_block ───────────────────────────────────────────────────

    #[test]
    fn test_finalize_block_success() {
        let engine = make_engine_with_bls(5);
        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();

        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        engine.verify_block(&block1).unwrap();

        let qc = make_bls_qc(block1.block_hash, 1, &[1, 2, 3, 4], SlotId(400));
        let finalized = engine.finalize_block(block1.block_hash, &qc, SlotId(400)).unwrap();
        assert_eq!(finalized.header.height, 1);
        assert_eq!(engine.last_finalized_height(), 1);
    }

    #[test]
    fn test_finalize_unverified_block_fails() {
        let engine = make_engine_with_bls(5);
        // Blok hiç verify edilmedi — QC geçerli ama blok set'te yok → BlockNotFound
        let fake_hash = bh(42);
        let qc = make_bls_qc(fake_hash, 1, &[1, 2, 3, 4], SlotId(400));
        let err = engine.finalize_block(fake_hash, &qc, SlotId(400)).unwrap_err();
        assert!(matches!(err, ConsensusError::BlockNotFound { .. }));
    }

    #[test]
    fn test_finalize_already_finalized_fails() {
        let engine = make_engine_with_bls(5);
        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();

        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        engine.verify_block(&block1).unwrap();

        let qc = make_bls_qc(block1.block_hash, 1, &[1, 2, 3, 4], SlotId(400));
        engine.finalize_block(block1.block_hash, &qc, SlotId(400)).unwrap();

        // Tekrar finalize → hata
        let qc2 = make_bls_qc(block1.block_hash, 1, &[1, 2, 3, 4], SlotId(401));
        let err = engine.finalize_block(block1.block_hash, &qc2, SlotId(401)).unwrap_err();
        assert!(matches!(err, ConsensusError::AlreadyFinalized { .. }));
    }

    // ─── update_validator_set ─────────────────────────────────────────────

    #[test]
    fn test_update_validator_set() {
        let mut engine = make_engine(5);
        assert!(engine.is_validator(&vid(1)));
        assert!(!engine.is_validator(&vid(10)));

        engine.update_validator_set(vec![vid(10), vid(11), vid(12)]);
        assert!(!engine.is_validator(&vid(1)));
        assert!(engine.is_validator(&vid(10)));
    }

    // ─── BLS imzalama testleri ────────────────────────────────────────────

    use crate::signing::BftSigner;

    fn make_bft_signer(n: u64) -> BftSigner {
        // Deterministik IKM: test için sabit seed
        let ikm = [(n as u8).wrapping_mul(37); 32];
        BftSigner::from_ikm(ValidatorId(n), &ikm).unwrap()
    }

    fn make_engine_with_signer(n: u8) -> BftEngine {
        let validators: Vec<ValidatorId> = (1..=n).map(vid).collect();
        BftEngine::new_with_signer(validators, make_bft_signer(1))
    }

    #[test]
    fn test_bft_engine_signer_state() {
        let engine = make_engine(5);
        assert!(!engine.has_signer(), "signer olmadan oluşturuldu");

        let engine_with = make_engine_with_signer(5);
        assert!(engine_with.has_signer(), "signer ile oluşturuldu");
    }

    #[test]
    fn test_set_signer_after_construction() {
        let mut engine = make_engine(3);
        assert!(!engine.has_signer());
        engine.set_signer(make_bft_signer(1));
        assert!(engine.has_signer());
    }

    #[test]
    fn test_sign_and_process_vote_no_signer_fails() {
        let engine = make_engine(5);
        let err = engine
            .sign_and_process_vote(bh(1), 1, SlotId(400))
            .unwrap_err();
        assert!(
            matches!(err, ConsensusError::Internal(_)),
            "signer yoksa Internal hatası beklenir"
        );
    }

    #[test]
    fn test_sign_and_process_vote_single_validator_quorum() {
        // 1 validator → quorum = 1 → kendi oyu ile QC üretir
        let validators = vec![vid(1)];
        let engine = BftEngine::new_with_signer(validators, make_bft_signer(1));

        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();
        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        engine.verify_block(&block1).unwrap();

        let result = engine.sign_and_process_vote(block1.block_hash, 1, SlotId(400));
        assert!(result.is_ok());
        let qc = result.unwrap().expect("tek validator quorum sağlamalı");

        assert_eq!(qc.height, 1);
        assert_eq!(qc.signers.len(), 1);
        // 1 BLS imzası = 96 byte
        assert_eq!(qc.aggregate_signature.len(), 96);
    }

    #[test]
    fn test_process_signed_vote_quorum_produces_bls_qc() {
        // 5 validator, quorum = ceil(10/3) = 4
        let validators: Vec<ValidatorId> = (1..=5).map(vid).collect();
        let engine = BftEngine::new(validators);

        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();
        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        engine.verify_block(&block1).unwrap();

        let mut last_qc = None;
        for i in 1u64..=4 {
            let signed = make_bft_signer(i)
                .sign_vote(block1.block_hash, 1, SlotId(400))
                .unwrap();
            last_qc = engine.process_signed_vote(signed, SlotId(400)).unwrap();
        }

        let qc = last_qc.expect("4/5 validator quorum sağlamalı");
        assert_eq!(qc.signers.len(), 4);
        // 4 BLS imzası × 96 byte = 384 byte
        assert_eq!(qc.aggregate_signature.len(), 4 * 96);
    }

    #[test]
    fn test_process_signed_vote_invalid_bls_rejected() {
        use nova_primitives::BlsSignature;
        let validators = vec![vid(1), vid(2), vid(3)];
        let engine = BftEngine::new(validators);

        let mut signed = make_bft_signer(1).sign_vote(bh(1), 1, SlotId(400)).unwrap();
        // BLS imzayı boz → doğrulama başarısız olmalı
        signed.bls_signature = BlsSignature::from_bytes([0u8; 96]);

        let err = engine.process_signed_vote(signed, SlotId(400)).unwrap_err();
        assert!(
            matches!(err, ConsensusError::Internal(_)),
            "geçersiz BLS imzası reddedilmeli"
        );
    }

    #[test]
    fn test_process_signed_vote_unauthorized_validator_rejected() {
        let validators = vec![vid(1), vid(2)];
        let engine = BftEngine::new(validators);

        // vid(99) validator setinde yok
        let signed = make_bft_signer(99).sign_vote(bh(1), 1, SlotId(400)).unwrap();
        let err = engine.process_signed_vote(signed, SlotId(400)).unwrap_err();
        assert!(matches!(err, ConsensusError::ValidatorNotFound { .. }));
    }

    #[test]
    fn test_process_signed_vote_duplicate_rejected() {
        let validators = vec![vid(1), vid(2), vid(3)];
        let engine = BftEngine::new(validators);

        let signer = make_bft_signer(1);
        let signed1 = signer.sign_vote(bh(1), 1, SlotId(400)).unwrap();
        let signed2 = signer.sign_vote(bh(1), 1, SlotId(400)).unwrap();

        engine.process_signed_vote(signed1, SlotId(400)).unwrap();
        let err = engine.process_signed_vote(signed2, SlotId(400)).unwrap_err();
        assert!(matches!(err, ConsensusError::DuplicateSigner { .. }));
    }

    #[test]
    fn test_bls_qc_finalizes_block_end_to_end() {
        // 3 validator, quorum = ceil(6/3) = 2
        let validators: Vec<ValidatorId> = (1..=3).map(vid).collect();
        let engine = BftEngine::new(validators);

        let genesis = make_genesis_block();
        engine.verify_block(&genesis).unwrap();
        let block1 = make_block(1, genesis.block_hash, SlotId(400));
        engine.verify_block(&block1).unwrap();

        // 2 signed vote → quorum
        let mut qc = None;
        for i in 1u64..=2 {
            let signed = make_bft_signer(i)
                .sign_vote(block1.block_hash, 1, SlotId(400))
                .unwrap();
            qc = engine.process_signed_vote(signed, SlotId(400)).unwrap();
        }

        let qc = qc.expect("quorum sağlanmalı");
        let finalized = engine.finalize_block(block1.block_hash, &qc, SlotId(400)).unwrap();
        assert_eq!(finalized.header.height, 1);
        assert_eq!(engine.last_finalized_height(), 1);
    }

    #[test]
    fn test_bls_no_quorum_no_qc() {
        // 5 validator, quorum = 4; sadece 3 oy → QC üretilmez
        let validators: Vec<ValidatorId> = (1..=5).map(vid).collect();
        let engine = BftEngine::new(validators);

        for i in 1u64..=3 {
            let signed = make_bft_signer(i).sign_vote(bh(1), 1, SlotId(400)).unwrap();
            let qc = engine.process_signed_vote(signed, SlotId(400)).unwrap();
            assert!(qc.is_none(), "3 oy quorum sağlamamalı (gerekli: 4)");
        }
    }
}
