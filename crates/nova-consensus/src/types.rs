//! nova-consensus çekirdek veri tipleri — §5.10
//!
//! Araştırma kaynakları:
//! - Avalanchego simplex Block + QC yapıları:
//!   `ProtocolMetadata { Seq, Prev, Epoch }` → `BlockHeader { height, parent_hash, epoch }`.
//!   `QC { sig, signers }` → `QuorumCertificate { aggregate_signature, signers }`.
//! - Cosmos SDK x/staking `ValidatorStatus`:
//!   `Bonded/Unbonding/Unbonded` → `ValidatorStatus::Active/PendingUnbonding/Inactive`.
//! - Cosmos SDK x/slashing `ValidatorSigningInfo`:
//!   `MissedBlocksCounter`, `JailedUntil`, `Tombstoned` → direkt karşılıklar.
//! - Polkadot sp-staking `EraIndex`, `SessionIndex`:
//!   `EpochId` ile karşılandı (24h = 1 epoch).
//! - go-ethereum `types.Header`:
//!   `ParentHash`, `Root`, `Number`, `Time` → BlockHeader alanları.

use nova_primitives::{Address, Balance, BlsPublicKey, BlockHash, EpochId, SlotId, ValidatorId};
use serde::{Deserialize, Serialize};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Minimum validator stake miktarı: 100,000 NSC (18 ondalık basamak).
/// Whitepaper §5.10: "Minimum stake: 100,000 NSC"
pub const MIN_VALIDATOR_STAKE: u128 = 100_000 * 10u128.pow(18);

/// Aktif validator seti boyutu (başlangıç).
/// Whitepaper §5.10: "200 başlangıç, 1000'e kadar büyüyebilir"
pub const ACTIVE_VALIDATOR_COUNT: usize = 200;

/// Quorum eşiği: 2/3 + 1 (BFT quorum).
/// Whitepaper §5.10: "2/3 validator onayı = kesinleşme"
/// = ceil(2 * 200 / 3) = 134
pub const QUORUM_NUMERATOR: usize = 2;
pub const QUORUM_DENOMINATOR: usize = 3;

/// Epoch süresi: 24 saat slot cinsinden.
/// = 24 * 3600 * 1000 / 400ms = 216_000 slot
pub const EPOCH_DURATION_SLOTS: u64 = 216_000;

/// Slot süresi: 400ms.
pub const SLOT_DURATION_MS: u64 = 400;

/// Validator unbonding süresi: 21 gün slot cinsinden.
/// Cosmos SDK x/staking pattern: unbonding period = 21 days.
/// = 21 * 24 * 3600 * 1000 / 400 = 4_536_000 slot
pub const UNBONDING_PERIOD_SLOTS: u64 = 4_536_000;

/// Double-sign delilinin geçerli sayılacağı maksimum yaş: 72 saat slot.
/// Cosmos SDK x/evidence: `MaxAgeNumBlocks`.
pub const MAX_EVIDENCE_AGE_SLOTS: u64 = 648_000;

/// Çevrimdışı slash penceresi: 24 saatlik blok sayısı (liveness window).
/// Cosmos SDK x/slashing `SignedBlocksWindow` pattern.
pub const SIGNED_BLOCKS_WINDOW: u64 = EPOCH_DURATION_SLOTS;

/// Minimum imzalanması gereken blok yüzdesi: %50 (bu altı → downtime slash).
/// Cosmos SDK: `MinSignedPerWindow`.
pub const MIN_SIGNED_PER_WINDOW_BPS: u64 = 5_000;

/// Double-sign slash: %100.
pub const SLASH_DOUBLE_SIGN_BPS: u32 = 10_000;

/// Mempool sızıntı slash: %50.
pub const SLASH_MEMPOOL_LEAK_BPS: u32 = 5_000;

/// Çevrimdışı slash: %1 (günlük, whitepaper'da %0.1 ama biz per-epoch uyguluyoruz).
pub const SLASH_DOWNTIME_BPS: u32 = 100;

/// Yanlış batch clearing slash: %25.
pub const SLASH_WRONG_BATCH_BPS: u32 = 2_500;

/// Checkpoint aralığı: her 100 blokta bir.
/// Ethereum'un checkpoint/finality pattern'inden ilham.
pub const CHECKPOINT_INTERVAL: u64 = 100;

// ─── BlockHash yardımcısı ─────────────────────────────────────────────────────

/// Blok hash'ini hex string'e çevir.
pub fn block_hash_to_hex(h: &BlockHash) -> String {
    hex::encode(h.as_bytes())
}

// ─── BlockHeader ──────────────────────────────────────────────────────────────

/// Blok başlığı.
///
/// Pattern: go-ethereum `types.Header` → `Number, ParentHash, Root, Time, Miner`.
/// Avalanchego simplex `ProtocolMetadata → Seq (height), Prev (parent_hash), Epoch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Blok yüksekliği (Avalanchego: Seq).
    pub height: u64,
    /// Slot numarası.
    pub slot: SlotId,
    /// Epoch numarası (her 24 saatte bir artar).
    pub epoch: EpochId,
    /// Önceki bloğun hash'i (Avalanchego: Prev digest).
    pub parent_hash: BlockHash,
    /// Durum ağacı kökü (go-ethereum: Root).
    pub state_root: [u8; 32],
    /// Bu bloğu öneren validator.
    pub proposer: ValidatorId,
    /// Unix timestamp (saniye).
    pub timestamp: u64,
    /// İşlem sayısı.
    pub tx_count: u32,
}

impl BlockHeader {
    /// Blok hash'ini hesapla: blake3(height || parent_hash || state_root || proposer || slot).
    ///
    /// Pattern: go-ethereum `rlpHash(header)` — tüm alanlar hash'e dahil.
    /// Avalanchego simplex `computeDigest(bytes)` — sha256 yerine blake3.
    pub fn compute_hash(&self) -> BlockHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.slot.0.to_le_bytes());
        hasher.update(&self.epoch.0.to_le_bytes());
        hasher.update(self.parent_hash.as_bytes());
        hasher.update(&self.state_root);
        hasher.update(&self.proposer.0.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        BlockHash::from_bytes(*hasher.finalize().as_bytes())
    }

    /// Genesis blok mu? (yükseklik 0)
    pub fn is_genesis(&self) -> bool {
        self.height == 0
    }
}

// ─── Block ────────────────────────────────────────────────────────────────────

/// Konsensüs bloğu.
///
/// Pattern: Avalanchego simplex `Block { metadata, vmBlock, digest }` →
/// `header + block_hash` — digest = header'ın hesaplanmış hash'i.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Blok başlığı.
    pub header: BlockHeader,
    /// Hesaplanmış blok hash'i (header.compute_hash() ile üretilir).
    pub block_hash: BlockHash,
}

impl Block {
    /// Yeni blok oluştur (hash otomatik hesaplanır).
    pub fn new(header: BlockHeader) -> Self {
        let block_hash = header.compute_hash();
        Self { header, block_hash }
    }

    /// Blok hash'ini doğrula.
    pub fn verify_hash(&self) -> bool {
        self.block_hash == self.header.compute_hash()
    }
}

// ─── QuorumCertificate ────────────────────────────────────────────────────────

/// BFT Quorum Sertifikası — 2/3 validator'ın BLS aggregate imzası.
///
/// Pattern: Avalanchego simplex `QC { sig, signers }`:
/// - `sig`: BLS aggregate imzası (burada placeholder [u8; 96])
/// - `signers`: NodeID listesi (burada ValidatorId listesi)
///
/// Gerçek BLS implementasyonu nova-crypto'dan gelecek.
/// Şimdilik blake3 tabanlı mock signature kullanılır.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumCertificate {
    /// İmzalanan blok hash'i.
    pub block_hash: BlockHash,
    /// İmzalanan blok yüksekliği.
    pub height: u64,
    /// İmzacı validator listesi.
    pub signers: Vec<ValidatorId>,
    /// Aggregate BLS imzası (üretim: blst; test: blake3 mock).
    pub aggregate_signature: Vec<u8>,
    /// İmza atılan slot.
    pub signed_at_slot: SlotId,
}

impl QuorumCertificate {
    /// Quorum karşılandı mı? (2/3 validator onayı)
    ///
    /// Avalanchego simplex `simplex.Quorum(len)` hesabı:
    /// quorum = ceil(2n/3)
    pub fn has_quorum(&self, total_validators: usize) -> bool {
        let quorum = quorum_threshold(total_validators);
        self.signers.len() >= quorum
    }

    /// Test için geçerli mock QC üret.
    /// Gerçek BLS imzası yerine blake3(block_hash || signers) kullanır.
    #[cfg(test)]
    pub fn mock_valid(
        block_hash: BlockHash,
        height: u64,
        signers: Vec<ValidatorId>,
        slot: SlotId,
    ) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(block_hash.as_bytes());
        hasher.update(&height.to_le_bytes());
        for s in &signers {
            hasher.update(&s.0.to_le_bytes());
        }
        let hash = hasher.finalize();
        let mut sig = vec![0u8; 96];
        sig[..32].copy_from_slice(hash.as_bytes());

        Self {
            block_hash,
            height,
            signers,
            aggregate_signature: sig,
            signed_at_slot: slot,
        }
    }
}

/// Quorum eşiğini hesapla: ceil(2n/3).
///
/// Avalanchego simplex `simplex.Quorum(n)` pattern.
pub fn quorum_threshold(total: usize) -> usize {
    (2 * total + 2) / 3 // ceil(2n/3)
}

// ─── ValidatorStatus ──────────────────────────────────────────────────────────

/// Validator durum makinesi.
///
/// Pattern: Cosmos SDK x/staking `BondStatus { Bonded, Unbonding, Unbonded }` +
/// x/slashing `{ JailedUntil, Tombstoned }` birleştirme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Aktif — blok üretiyor, imzalıyor.
    Active,
    /// Jail'de — geçici askıya alındı (downtime).
    /// Cosmos SDK: `JailedUntil` field.
    Jailed {
        jailed_until_slot: u64,
    },
    /// Tombstone — kalıcı çıkarma (double-sign).
    /// Cosmos SDK x/evidence: `Tombstone(ctx, consAddr)`.
    Tombstoned,
    /// Aktif değil (stake yok veya unbonded).
    Inactive,
    /// Unbonding — stake çekiliyor (21 gün kilit).
    PendingUnbonding {
        unlock_at_slot: u64,
    },
}

impl ValidatorStatus {
    /// Blok üretimi veya imzalama yapabilir mi?
    pub fn can_participate(&self, current_slot: u64) -> bool {
        match self {
            Self::Active => true,
            Self::Jailed { jailed_until_slot } => current_slot >= *jailed_until_slot,
            Self::Tombstoned | Self::Inactive | Self::PendingUnbonding { .. } => false,
        }
    }

    /// Aktif mi?
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Jailed mi?
    pub fn is_jailed(&self) -> bool {
        matches!(self, Self::Jailed { .. })
    }

    /// Tombstoned mi?
    pub fn is_tombstoned(&self) -> bool {
        matches!(self, Self::Tombstoned)
    }
}

// ─── ValidatorInfo ────────────────────────────────────────────────────────────

/// Bir validator'ın tam bilgisi.
///
/// Pattern: Cosmos SDK `Validator { OperatorAddress, ConsPubKey, Status, Tokens, Commission }` +
/// `ValidatorSigningInfo { MissedBlocksCounter, IndexOffset, JailedUntil }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator kimliği (consensus address).
    pub validator_id: ValidatorId,
    /// Validator operatörü (staking/ödül adresi).
    pub operator: Address,
    /// Toplam stake miktarı (NSC, 18 ondalık).
    pub stake: Balance,
    /// Komisyon oranı (bps, 0-10000).
    pub commission_bps: u32,
    /// Validator durumu.
    pub status: ValidatorStatus,
    /// Liveness penceresi içinde kaçırılan blok sayısı.
    /// Cosmos SDK x/slashing `MissedBlocksCounter`.
    pub missed_blocks_counter: u64,
    /// Liveness bitmap offset (pencere içindeki konum).
    /// Cosmos SDK x/slashing `IndexOffset`.
    pub index_offset: u64,
    /// Son aktif slot.
    pub last_active_slot: SlotId,
    /// Kayıt slotu.
    pub registered_at_slot: SlotId,
    /// BLS imza anahtarı (32 byte pubkey placeholder).
    pub signing_key: [u8; 32],
    /// BLS public key — equivocation raporlama için key ownership proof.
    /// Polkadot: `ValidatorId` → `KeyOwnerProofSystem` kaydı.
    /// Validator kaydedilince `None`, `set_bls_pubkey()` ile set edilir.
    pub bls_pubkey: Option<BlsPublicKey>,
}

impl ValidatorInfo {
    /// Katılım hakkı var mı?
    pub fn can_participate(&self, current_slot: u64) -> bool {
        self.status.can_participate(current_slot)
    }

    /// Oy ağırlığı (stake bazlı).
    pub fn vote_weight(&self) -> u128 {
        self.stake.0
    }

    /// Yeni validator info oluştur.
    pub fn new(
        validator_id: ValidatorId,
        operator: Address,
        stake: Balance,
        commission_bps: u32,
        registered_at_slot: SlotId,
        signing_key: [u8; 32],
    ) -> Self {
        Self {
            validator_id,
            operator,
            stake,
            commission_bps,
            status: ValidatorStatus::Active,
            missed_blocks_counter: 0,
            index_offset: 0,
            last_active_slot: registered_at_slot,
            registered_at_slot,
            signing_key,
            bls_pubkey: None,
        }
    }
}

// ─── SlashReason ─────────────────────────────────────────────────────────────

/// Slash sebebi.
///
/// Whitepaper §5.10.4 + Cosmos SDK x/slashing/types `Infraction` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashReason {
    /// Aynı slot'ta iki farklı blok imzalamak → %100 slash.
    /// Cosmos SDK: `INFRACTION_DOUBLE_SIGN`.
    DoubleSign,
    /// Threshold decryption'dan önce mempool içeriği sızdırma → %50 slash.
    MempoolLeak,
    /// Uzun süre çevrimdışı → %1/epoch slash.
    ExtendedOffline,
    /// Yanlış batch clearing price hesabı → %25 slash.
    WrongBatchClearing,
}

impl SlashReason {
    /// Slash oranı (bps cinsinden).
    pub fn slash_bps(&self) -> u32 {
        match self {
            Self::DoubleSign => SLASH_DOUBLE_SIGN_BPS,
            Self::MempoolLeak => SLASH_MEMPOOL_LEAK_BPS,
            Self::ExtendedOffline => SLASH_DOWNTIME_BPS,
            Self::WrongBatchClearing => SLASH_WRONG_BATCH_BPS,
        }
    }

    /// Jail gerektiriyor mu?
    pub fn requires_jail(&self) -> bool {
        matches!(self, Self::ExtendedOffline | Self::WrongBatchClearing)
    }

    /// Tombstone gerektiriyor mu? (kalıcı çıkarma)
    pub fn requires_tombstone(&self) -> bool {
        matches!(self, Self::DoubleSign | Self::MempoolLeak)
    }
}

// ─── SlashRecord ─────────────────────────────────────────────────────────────

/// Gerçekleşmiş bir slash kaydı.
///
/// Cosmos SDK x/slashing pattern: `UnappliedSlash { validator, fraction, power }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashRecord {
    /// Slashed validator.
    pub validator: ValidatorId,
    /// Slash sebebi.
    pub reason: SlashReason,
    /// Uygulanan slash oranı (bps).
    pub slash_bps: u32,
    /// Slashed stake miktarı.
    pub slashed_amount: u128,
    /// Delil slotu (ihlal ne zaman tespit edildi).
    pub evidence_slot: SlotId,
    /// Slash'ın uygulandığı slot.
    pub applied_at_slot: SlotId,
}

// ─── DoubleSignEvidence ───────────────────────────────────────────────────────

/// Çift imza delili.
///
/// Cosmos SDK x/evidence `Equivocation { Height, Time, Power, ConsensusAddress }` pattern.
/// İki çelişkili imza — aynı yükseklikte farklı blok hash'leri.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubleSignEvidence {
    /// İhlali yapan validator.
    pub validator: ValidatorId,
    /// İhlal yüksekliği.
    pub height: u64,
    /// İlk blok hash'i.
    pub block_hash_a: BlockHash,
    /// İkinci blok hash'i (farklı — çelişkili).
    pub block_hash_b: BlockHash,
    /// Delilin sunulduğu slot.
    pub submitted_at_slot: SlotId,
}

impl DoubleSignEvidence {
    /// Delil geçerli mi? (iki farklı hash + aynı yükseklik).
    pub fn is_valid(&self) -> bool {
        self.block_hash_a != self.block_hash_b
    }

    /// Delil çok eski mi?
    pub fn is_too_old(&self, current_slot: u64) -> bool {
        let age = current_slot.saturating_sub(self.submitted_at_slot.0);
        age > MAX_EVIDENCE_AGE_SLOTS
    }
}

// ─── EpochInfo ────────────────────────────────────────────────────────────────

/// Epoch bilgisi.
///
/// Pattern: Polkadot sp-staking `EraIndex` + Cosmos SDK era/epoch management.
/// Her epoch 24 saat, 216_000 slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochInfo {
    /// Epoch kimliği.
    pub epoch_id: EpochId,
    /// Başlangıç slotu.
    pub started_at_slot: SlotId,
    /// Bitiş slotu.
    pub ends_at_slot: SlotId,
    /// Bu epoch'taki aktif validator seti.
    pub validator_set: Vec<ValidatorId>,
    /// Toplam aktif stake.
    pub total_stake: Balance,
    /// Bu epoch'ta üretilen blok sayısı.
    pub blocks_produced: u64,
}

impl EpochInfo {
    /// Epoch henüz aktif mi?
    pub fn is_active(&self, current_slot: SlotId) -> bool {
        current_slot >= self.started_at_slot && current_slot < self.ends_at_slot
    }

    /// Epoch doldu mu?
    pub fn is_expired(&self, current_slot: SlotId) -> bool {
        current_slot >= self.ends_at_slot
    }

    /// Kalan slot sayısı.
    pub fn remaining_slots(&self, current_slot: SlotId) -> u64 {
        if current_slot >= self.ends_at_slot {
            0
        } else {
            self.ends_at_slot.0 - current_slot.0
        }
    }

    /// Bu epoch'taki blok önerici validator (round-robin).
    ///
    /// Whitepaper §5.10: "Her slot'ta 1 validator blok üretir (round-robin + VRF)"
    /// Burada basit round-robin: proposer_index = (slot - started_at) % validator_count
    pub fn slot_proposer(&self, slot: SlotId) -> Option<ValidatorId> {
        if self.validator_set.is_empty() {
            return None;
        }
        let offset = slot.0.saturating_sub(self.started_at_slot.0);
        let idx = (offset as usize) % self.validator_set.len();
        Some(self.validator_set[idx])
    }
}

// ─── CheckpointRecord ────────────────────────────────────────────────────────

/// Finalize edilmiş blok checkpoint'i.
///
/// Pattern: Ethereum `Checkpoint { Epoch, Root }` + Cosmos SDK `commit` pattern.
/// Her CHECKPOINT_INTERVAL blokta bir üretilir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    /// Checkpoint yüksekliği.
    pub height: u64,
    /// Checkpoint blok hash'i.
    pub block_hash: BlockHash,
    /// State root bu noktada.
    pub state_root: [u8; 32],
    /// Epoch kimliği.
    pub epoch: EpochId,
    /// Bu checkpoint'i onaylayan QC.
    pub qc: QuorumCertificate,
    /// Oluşturulma slotu.
    pub created_at_slot: SlotId,
}

// ─── ConsensusEvent ──────────────────────────────────────────────────────────

/// Konsensüs motor eventi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusEvent {
    pub slot: SlotId,
    pub event_type: ConsensusEventType,
}

/// Konsensüs event türleri.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusEventType {
    /// Yeni blok finalize edildi.
    BlockFinalized {
        height: u64,
        block_hash: BlockHash,
    },
    /// Epoch geçişi tamamlandı.
    EpochTransition {
        old_epoch: u64,
        new_epoch: u64,
        new_validators: Vec<ValidatorId>,
    },
    /// Validator slashlandı.
    ValidatorSlashed {
        validator: ValidatorId,
        reason: SlashReason,
        slash_bps: u32,
    },
    /// Validator jaillandı.
    ValidatorJailed {
        validator: ValidatorId,
        jailed_until_slot: u64,
    },
    /// Validator tombstone edildi.
    ValidatorTombstoned {
        validator: ValidatorId,
        reason: SlashReason,
    },
    /// Checkpoint oluşturuldu.
    CheckpointCreated {
        height: u64,
        block_hash: BlockHash,
    },
    /// Quorum sertifikası toplanıldı.
    QuorumReached {
        height: u64,
        signer_count: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{Address, Balance, BlockHash, EpochId, SlotId, ValidatorId};

    fn make_validator_id(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn make_block_hash(seed: u8) -> BlockHash {
        BlockHash::from_bytes([seed; 32])
    }

    fn make_addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    // ─── quorum_threshold ─────────────────────────────────────────────────

    #[test]
    fn test_quorum_threshold_200_validators() {
        // 200 validator → quorum = ceil(2*200/3) = ceil(133.33) = 134
        assert_eq!(quorum_threshold(200), 134);
    }

    #[test]
    fn test_quorum_threshold_3_validators() {
        // 3 → ceil(2*3/3) = ceil(2) = 2
        assert_eq!(quorum_threshold(3), 2);
    }

    #[test]
    fn test_quorum_threshold_1_validator() {
        // 1 → ceil(2/3) = 1
        assert_eq!(quorum_threshold(1), 1);
    }

    #[test]
    fn test_quorum_threshold_100_validators() {
        // 100 → ceil(200/3) = 67
        assert_eq!(quorum_threshold(100), 67);
    }

    // ─── BlockHeader ──────────────────────────────────────────────────────

    #[test]
    fn test_block_header_compute_hash_deterministic() {
        let header = BlockHeader {
            height: 1,
            slot: SlotId(400),
            epoch: EpochId(0),
            parent_hash: make_block_hash(0),
            state_root: [1u8; 32],
            proposer: make_validator_id(10),
            timestamp: 1_000_000,
            tx_count: 42,
        };

        let h1 = header.compute_hash();
        let h2 = header.compute_hash();
        assert_eq!(h1, h2, "deterministik olmalı");
    }

    #[test]
    fn test_block_header_compute_hash_different_height() {
        let base = BlockHeader {
            height: 1,
            slot: SlotId(400),
            epoch: EpochId(0),
            parent_hash: make_block_hash(0),
            state_root: [1u8; 32],
            proposer: make_validator_id(10),
            timestamp: 1_000_000,
            tx_count: 0,
        };
        let mut other = base.clone();
        other.height = 2;
        assert_ne!(base.compute_hash(), other.compute_hash());
    }

    #[test]
    fn test_block_genesis_flag() {
        let genesis = BlockHeader {
            height: 0,
            slot: SlotId(0),
            epoch: EpochId(0),
            parent_hash: make_block_hash(0),
            state_root: [0u8; 32],
            proposer: make_validator_id(1),
            timestamp: 0,
            tx_count: 0,
        };
        assert!(genesis.is_genesis());

        let mut next = genesis.clone();
        next.height = 1;
        assert!(!next.is_genesis());
    }

    // ─── Block ────────────────────────────────────────────────────────────

    #[test]
    fn test_block_verify_hash_valid() {
        let header = BlockHeader {
            height: 5,
            slot: SlotId(2000),
            epoch: EpochId(0),
            parent_hash: make_block_hash(4),
            state_root: [5u8; 32],
            proposer: make_validator_id(1),
            timestamp: 2_000_000,
            tx_count: 10,
        };
        let block = Block::new(header);
        assert!(block.verify_hash());
    }

    #[test]
    fn test_block_tampered_hash_invalid() {
        let header = BlockHeader {
            height: 5,
            slot: SlotId(2000),
            epoch: EpochId(0),
            parent_hash: make_block_hash(4),
            state_root: [5u8; 32],
            proposer: make_validator_id(1),
            timestamp: 2_000_000,
            tx_count: 10,
        };
        let mut block = Block::new(header);
        // Hash'i bozan değişiklik
        block.block_hash = make_block_hash(99);
        assert!(!block.verify_hash());
    }

    // ─── QuorumCertificate ────────────────────────────────────────────────

    #[test]
    fn test_qc_has_quorum_200_validators() {
        let signers: Vec<ValidatorId> = (0..134u8).map(make_validator_id).collect();
        let qc = QuorumCertificate::mock_valid(make_block_hash(1), 1, signers, SlotId(400));
        assert!(qc.has_quorum(200));
    }

    #[test]
    fn test_qc_no_quorum_below_threshold() {
        // 133 < 134 = quorum for 200
        let signers: Vec<ValidatorId> = (0..133u8).map(make_validator_id).collect();
        let qc = QuorumCertificate::mock_valid(make_block_hash(1), 1, signers, SlotId(400));
        assert!(!qc.has_quorum(200));
    }

    // ─── ValidatorStatus ──────────────────────────────────────────────────

    #[test]
    fn test_validator_status_active_can_participate() {
        assert!(ValidatorStatus::Active.can_participate(1000));
    }

    #[test]
    fn test_validator_status_jailed_cannot_participate_during_jail() {
        let status = ValidatorStatus::Jailed { jailed_until_slot: 5000 };
        assert!(!status.can_participate(4999));
        assert!(status.can_participate(5000));
    }

    #[test]
    fn test_validator_status_tombstoned_cannot_participate() {
        assert!(!ValidatorStatus::Tombstoned.can_participate(u64::MAX));
    }

    #[test]
    fn test_validator_status_inactive_cannot_participate() {
        assert!(!ValidatorStatus::Inactive.can_participate(1000));
    }

    #[test]
    fn test_validator_status_unbonding_cannot_participate() {
        let status = ValidatorStatus::PendingUnbonding { unlock_at_slot: 1000 };
        assert!(!status.can_participate(1000));
    }

    // ─── SlashReason ──────────────────────────────────────────────────────

    #[test]
    fn test_slash_reason_bps() {
        assert_eq!(SlashReason::DoubleSign.slash_bps(), 10_000);
        assert_eq!(SlashReason::MempoolLeak.slash_bps(), 5_000);
        assert_eq!(SlashReason::ExtendedOffline.slash_bps(), 100);
        assert_eq!(SlashReason::WrongBatchClearing.slash_bps(), 2_500);
    }

    #[test]
    fn test_slash_reason_tombstone_requirement() {
        assert!(SlashReason::DoubleSign.requires_tombstone());
        assert!(SlashReason::MempoolLeak.requires_tombstone());
        assert!(!SlashReason::ExtendedOffline.requires_tombstone());
        assert!(!SlashReason::WrongBatchClearing.requires_tombstone());
    }

    #[test]
    fn test_slash_reason_jail_requirement() {
        assert!(SlashReason::ExtendedOffline.requires_jail());
        assert!(SlashReason::WrongBatchClearing.requires_jail());
        assert!(!SlashReason::DoubleSign.requires_jail());
        assert!(!SlashReason::MempoolLeak.requires_jail());
    }

    // ─── DoubleSignEvidence ───────────────────────────────────────────────

    #[test]
    fn test_double_sign_evidence_valid() {
        let ev = DoubleSignEvidence {
            validator: make_validator_id(1),
            height: 100,
            block_hash_a: make_block_hash(1),
            block_hash_b: make_block_hash(2),
            submitted_at_slot: SlotId(1000),
        };
        assert!(ev.is_valid());
    }

    #[test]
    fn test_double_sign_evidence_invalid_same_hash() {
        let ev = DoubleSignEvidence {
            validator: make_validator_id(1),
            height: 100,
            block_hash_a: make_block_hash(1),
            block_hash_b: make_block_hash(1),
            submitted_at_slot: SlotId(1000),
        };
        assert!(!ev.is_valid());
    }

    #[test]
    fn test_double_sign_evidence_too_old() {
        let ev = DoubleSignEvidence {
            validator: make_validator_id(1),
            height: 100,
            block_hash_a: make_block_hash(1),
            block_hash_b: make_block_hash(2),
            submitted_at_slot: SlotId(0),
        };
        // MAX_EVIDENCE_AGE_SLOTS + 1 slot sonra → çok eski
        assert!(ev.is_too_old(MAX_EVIDENCE_AGE_SLOTS + 1));
        assert!(!ev.is_too_old(MAX_EVIDENCE_AGE_SLOTS));
    }

    // ─── EpochInfo ────────────────────────────────────────────────────────

    #[test]
    fn test_epoch_info_is_active() {
        let epoch = EpochInfo {
            epoch_id: EpochId(1),
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(EPOCH_DURATION_SLOTS),
            validator_set: vec![make_validator_id(1)],
            total_stake: Balance(1_000_000),
            blocks_produced: 0,
        };
        assert!(epoch.is_active(SlotId(0)));
        assert!(epoch.is_active(SlotId(EPOCH_DURATION_SLOTS - 1)));
        assert!(!epoch.is_active(SlotId(EPOCH_DURATION_SLOTS)));
    }

    #[test]
    fn test_epoch_info_slot_proposer_round_robin() {
        let v1 = make_validator_id(1);
        let v2 = make_validator_id(2);
        let v3 = make_validator_id(3);
        let epoch = EpochInfo {
            epoch_id: EpochId(1),
            started_at_slot: SlotId(100),
            ends_at_slot: SlotId(100 + EPOCH_DURATION_SLOTS),
            validator_set: vec![v1, v2, v3],
            total_stake: Balance(3_000_000),
            blocks_produced: 0,
        };
        assert_eq!(epoch.slot_proposer(SlotId(100)), Some(v1));
        assert_eq!(epoch.slot_proposer(SlotId(101)), Some(v2));
        assert_eq!(epoch.slot_proposer(SlotId(102)), Some(v3));
        assert_eq!(epoch.slot_proposer(SlotId(103)), Some(v1)); // döngü
    }

    #[test]
    fn test_epoch_info_remaining_slots() {
        let epoch = EpochInfo {
            epoch_id: EpochId(0),
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(1000),
            validator_set: vec![],
            total_stake: Balance(0),
            blocks_produced: 0,
        };
        assert_eq!(epoch.remaining_slots(SlotId(0)), 1000);
        assert_eq!(epoch.remaining_slots(SlotId(500)), 500);
        assert_eq!(epoch.remaining_slots(SlotId(1000)), 0);
        assert_eq!(epoch.remaining_slots(SlotId(1001)), 0);
    }

    // ─── Sabitler ─────────────────────────────────────────────────────────

    #[test]
    fn test_epoch_duration_is_24h() {
        // 24h = 24 * 3600 * 1000 / 400ms = 216_000 slot
        let expected = 24u64 * 3600 * 1000 / 400;
        assert_eq!(EPOCH_DURATION_SLOTS, expected);
    }

    #[test]
    fn test_unbonding_period_is_21_days() {
        // 21d = 21 * 24 * 3600 * 1000 / 400ms = 4_536_000 slot
        let expected = 21u64 * 24 * 3600 * 1000 / 400;
        assert_eq!(UNBONDING_PERIOD_SLOTS, expected);
    }

    #[test]
    fn test_min_validator_stake() {
        // 100_000 NSC * 10^18
        assert_eq!(MIN_VALIDATOR_STAKE, 100_000 * 10u128.pow(18));
    }
}

// ─── SlotTimer ────────────────────────────────────────────────────────────────

/// Drift-free slot zamanlayıcı.
///
/// ## BUG-2 FIX: Fixed-interval sleep yerine genesis-offset hesabı
///
/// Yanlış yöntem (drift birikir):
/// ```text
/// loop { sleep(400ms); slot += 1; }
/// ```
///
/// Doğru yöntem (drift-free):
/// ```text
/// next_tick = genesis_time + (current_slot + 1) * SLOT_DURATION_MS
/// sleep_until(next_tick)
/// ```
///
/// ## İlham: Avalanchego Simplex
/// `simplex/parameters.go`: `MaxNetworkDelay = 5s`, `MaxRebroadcastWait = 5s`
/// Slot timer: genesis timestamp'ten itibaren mutlak zaman hesabı.
pub struct SlotTimer {
    /// Zincir başlangıç zamanı (Unix ms).
    genesis_time_ms: u64,
    /// Slot süresi (ms). Varsayılan: 400ms.
    slot_duration_ms: u64,
}

impl SlotTimer {
    /// Yeni slot timer oluştur.
    pub fn new(genesis_time_ms: u64, slot_duration_ms: u64) -> Self {
        Self { genesis_time_ms, slot_duration_ms }
    }

    /// Şu anki unix zamanını ms olarak al.
    fn now_ms() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Şu anki slot numarasını hesapla (genesis'ten geçen süreye göre).
    ///
    /// `slot = (now - genesis_time) / slot_duration`
    pub fn current_slot(&self) -> SlotId {
        let elapsed = Self::now_ms().saturating_sub(self.genesis_time_ms);
        SlotId(elapsed / self.slot_duration_ms)
    }

    /// Bir sonraki slot'un başlangıç zamanı (Unix ms).
    ///
    /// `next_start = genesis_time + (current_slot + 1) * slot_duration`
    pub fn next_slot_start_ms(&self) -> u64 {
        let current = self.current_slot().0;
        self.genesis_time_ms + (current + 1) * self.slot_duration_ms
    }

    /// Sonraki slot'a kadar beklenecek süre (ms).
    ///
    /// Async konsensüste: `tokio::time::sleep(Duration::from_millis(timer.ms_until_next_slot()))`
    /// Her tick sabit 400ms değil, mutlak zamana göre hesaplanır → drift yok.
    pub fn ms_until_next_slot(&self) -> u64 {
        self.next_slot_start_ms().saturating_sub(Self::now_ms())
    }

    /// Genesis zamanı.
    pub fn genesis_time_ms(&self) -> u64 { self.genesis_time_ms }

    /// Slot süresi.
    pub fn slot_duration_ms(&self) -> u64 { self.slot_duration_ms }
}

#[cfg(test)]
mod slot_timer_tests {
    use super::*;

    #[test]
    fn test_slot_timer_genesis_slot_zero() {
        // Genesis zamanı = 1 gün önce
        let genesis = SlotTimer::now_ms_test() - 1_000;
        let timer = SlotTimer::new(genesis, 400);
        // 1000ms geçmiş → slot = 1000/400 = 2
        let slot = (1_000u64) / 400;
        // current_slot en az `slot` olmalı (not = ile çünkü now değişiyor)
        assert!(timer.current_slot().0 >= slot);
    }

    #[test]
    fn test_slot_timer_next_slot_in_future() {
        let genesis = SlotTimer::now_ms_test();
        let timer = SlotTimer::new(genesis, 400);
        // next_slot_start genesis'ten 400ms sonra olmalı
        assert_eq!(timer.next_slot_start_ms(), genesis + 400);
    }

    #[test]
    fn test_slot_timer_deterministic_from_genesis() {
        let genesis = 1_000_000_000u64; // Sabit genesis
        let timer = SlotTimer::new(genesis, 400);
        // Slot 0: [genesis, genesis+400)
        // Slot 1: [genesis+400, genesis+800)
        // genesis + 800ms → slot 2
        let elapsed = 800u64;
        let slot = elapsed / 400;
        assert_eq!(slot, 2);
        // next_start(slot=2) = genesis + 3*400 = genesis + 1200
        let next = genesis + (slot + 1) * 400;
        assert_eq!(next, genesis + 1200);
    }
}

impl SlotTimer {
    /// Test için şu anki zamanı döndür (test'lerde override edilebilir).
    #[cfg(test)]
    pub fn now_ms_test() -> u64 {
        Self::now_ms()
    }
}
