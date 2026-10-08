//! nova-consensus hata tipleri
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing `ErrValidatorJailed`, `ErrMissingSelfDelegation`:
//!   Her modül ayrı error enum tanımlar, domain-specific hata mesajları.
//! - Avalanchego simplex `errUnknownMessageType`, `errNilSimplexParameters`:
//!   Sentinel error pattern — önceden tanımlanmış `var err = errors.New(...)`.
//! - Polkadot frame_support `DispatchError`: Enum variant'lar typed error taşır.

use thiserror::Error;

/// Konsensüs işlem sonucu.
pub type ConsensusResult<T> = Result<T, ConsensusError>;

/// Nova Shield Chain konsensüs hataları.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConsensusError {
    // ── Validator Hataları ──────────────────────────────────────────────────

    /// Validator bulunamadı.
    #[error("validator bulunamadı: {validator_id}")]
    ValidatorNotFound { validator_id: String },

    /// Validator zaten kayıtlı.
    #[error("validator zaten kayıtlı: {validator_id}")]
    ValidatorAlreadyRegistered { validator_id: String },

    /// Yetersiz stake miktarı.
    #[error("yetersiz stake: gereken {required}, mevcut {provided}")]
    InsufficientStake { required: u128, provided: u128 },

    /// Validator jaillenmiş durumda.
    ///
    /// Cosmos SDK x/slashing pattern: `ErrValidatorJailed`.
    #[error("validator jail'de, slot {jailed_until_slot}'a kadar")]
    ValidatorJailed { jailed_until_slot: u64 },

    /// Validator tombstone edilmiş (çift imza) — kalıcı.
    ///
    /// Cosmos SDK x/evidence pattern: tombstone = permanent removal.
    #[error("validator tombstone edildi (çift imza ihlali)")]
    ValidatorTombstoned,

    // ── Blok / BFT Hataları ─────────────────────────────────────────────────

    /// Geçersiz blok hash'i.
    #[error("geçersiz blok hash'i")]
    InvalidBlockHash,

    /// Blok yüksekliği uyuşmazlığı.
    #[error("yükseklik uyuşmazlığı: beklenen {expected}, alınan {got}")]
    HeightMismatch { expected: u64, got: u64 },

    /// Geçersiz parent hash.
    #[error("parent hash uyuşmazlığı")]
    InvalidParentHash,

    /// Blok bulunamadı.
    #[error("blok bulunamadı: yükseklik {height}")]
    BlockNotFound { height: u64 },

    /// Zaten finalize edilmiş yükseklik.
    #[error("yükseklik {height} zaten finalize edildi")]
    AlreadyFinalized { height: u64 },

    // ── QC / İmza Hataları ──────────────────────────────────────────────────

    /// Quorum için yeterli imza yok.
    ///
    /// Avalanchego simplex QC pattern: `errUnexpectedSigners`.
    #[error("yetersiz imza: quorum {required}, mevcut {got}")]
    QuorumNotReached { required: usize, got: usize },

    /// Geçersiz BLS agregasyon imzası.
    #[error("geçersiz BLS aggregate imzası")]
    InvalidAggregateSignature,

    /// Aynı validator'dan mükerrer imza.
    #[error("mükerrer imza: {validator_id}")]
    DuplicateSigner { validator_id: String },

    // ── Epoch Hataları ──────────────────────────────────────────────────────

    /// Yanlış epoch.
    #[error("yanlış epoch: beklenen {expected}, alınan {got}")]
    WrongEpoch { expected: u64, got: u64 },

    /// Epoch geçişi henüz mümkün değil.
    #[error("epoch geçişi henüz mümkün değil, bitiş slotu {ends_at}")]
    EpochNotExpired { ends_at: u64 },

    // ── Slashing Hataları ───────────────────────────────────────────────────

    /// Çift imza kanıtı geçersiz.
    ///
    /// Cosmos SDK x/evidence: `handleEquivocationEvidence` geçersiz delil kontrolü.
    #[error("çift imza kanıtı geçersiz")]
    InvalidDoubleSignEvidence,

    /// Çift imza delili çok eski.
    #[error("delil çok eski: yaş {age_slots} slot, maksimum {max_slots}")]
    EvidenceTooOld { age_slots: u64, max_slots: u64 },

    // ── Checkpoint Hataları ─────────────────────────────────────────────────

    /// Checkpoint bulunamadı.
    #[error("checkpoint bulunamadı: yükseklik {height}")]
    CheckpointNotFound { height: u64 },

    /// Geçersiz checkpoint.
    #[error("geçersiz checkpoint")]
    InvalidCheckpoint,

    // ── Genel ───────────────────────────────────────────────────────────────

    /// Yetkisiz unbonding talebi — sadece validator kendisi unbonding başlatabilir.
    ///
    /// NOVA-RED-0005: begin_unbonding() initiator kontrolü.
    #[error("yetkisiz unbonding: initiator {initiator} validator {validator} değil")]
    UnauthorizedUnbonding { initiator: String, validator: String },

    /// İç hata.
    #[error("iç hata: {0}")]
    Internal(String),
}

impl ConsensusError {
    /// Validator yapılandırma hatası mı?
    pub fn is_validator_error(&self) -> bool {
        matches!(
            self,
            Self::ValidatorNotFound { .. }
                | Self::ValidatorAlreadyRegistered { .. }
                | Self::InsufficientStake { .. }
                | Self::ValidatorJailed { .. }
                | Self::ValidatorTombstoned
        )
    }

    /// BFT / quorum hatası mı?
    pub fn is_bft_error(&self) -> bool {
        matches!(
            self,
            Self::QuorumNotReached { .. }
                | Self::InvalidAggregateSignature
                | Self::DuplicateSigner { .. }
                | Self::InvalidBlockHash
                | Self::HeightMismatch { .. }
                | Self::InvalidParentHash
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_jailed_error() {
        let err = ConsensusError::ValidatorJailed { jailed_until_slot: 1_000 };
        assert!(err.is_validator_error());
        assert!(!err.is_bft_error());
        assert!(err.to_string().contains("1000"));
    }

    #[test]
    fn test_validator_tombstoned_error() {
        let err = ConsensusError::ValidatorTombstoned;
        assert!(err.is_validator_error());
        assert!(err.to_string().contains("tombstone"));
    }

    #[test]
    fn test_quorum_not_reached_error() {
        let err = ConsensusError::QuorumNotReached { required: 134, got: 100 };
        assert!(err.is_bft_error());
        assert!(!err.is_validator_error());
        assert!(err.to_string().contains("134"));
        assert!(err.to_string().contains("100"));
    }

    #[test]
    fn test_height_mismatch_error() {
        let err = ConsensusError::HeightMismatch { expected: 10, got: 11 };
        assert!(err.is_bft_error());
        assert!(err.to_string().contains("10"));
        assert!(err.to_string().contains("11"));
    }

    #[test]
    fn test_insufficient_stake_error() {
        let err = ConsensusError::InsufficientStake {
            required: 100_000,
            provided: 50_000,
        };
        assert!(err.is_validator_error());
        assert!(err.to_string().contains("100000"));
    }

    #[test]
    fn test_wrong_epoch_error() {
        let err = ConsensusError::WrongEpoch { expected: 5, got: 6 };
        assert!(!err.is_validator_error());
        assert!(!err.is_bft_error());
        assert!(err.to_string().contains("5"));
        assert!(err.to_string().contains("6"));
    }

    #[test]
    fn test_duplicate_signer_error() {
        let err = ConsensusError::DuplicateSigner { validator_id: "abc".to_string() };
        assert!(err.is_bft_error());
    }

    #[test]
    fn test_evidence_too_old_error() {
        let err = ConsensusError::EvidenceTooOld { age_slots: 1_000_000, max_slots: 648_000 };
        assert!(err.to_string().contains("1000000"));
    }

    #[test]
    fn test_internal_error() {
        let err = ConsensusError::Internal("beklenmedik durum".to_string());
        assert!(err.to_string().contains("beklenmedik durum"));
    }

    #[test]
    fn test_error_clone_and_eq() {
        let err1 = ConsensusError::ValidatorTombstoned;
        let err2 = err1.clone();
        assert_eq!(err1, err2);
    }
}
