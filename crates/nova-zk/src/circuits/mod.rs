//! ZK devreleri: Mint, Transfer, Burn, Spend + 8 R1CS devreleri

/// ZK-friendly MiMC hash (BLS12-381 Fr) — native + R1CS
pub mod mimc;

/// Arkworks R1CS devre implementasyonları (8 adet)
pub mod r1cs;

/// Mevcut mock/native devre implementasyonları
pub mod burn;
pub mod mint;
pub mod spend;
pub mod transfer;

pub use burn::{BurnProof, BurnStatement, BurnWitness};
pub use mint::{MintProof, MintStatement, MintWitness};
pub use spend::{SpendProof, SpendStatement, SpendWitness};
pub use transfer::{TransferProof, TransferStatement, TransferWitness};

// R1CS devre re-exports
pub use r1cs::{
    BurnCircuit, GovernanceVoteCircuit, MintCircuit, NullifierDerivationCircuit,
    SpendCircuit, StakeCircuit, TransferCircuit, UnstakeCircuit,
    compute_recipient_hash, compute_stake_nullifier, compute_validator_hash,
    compute_vote_commitment, compute_vote_nullifier,
};

// MiMC yardımcı fonksiyon re-exports
pub use mimc::{
    mimc_merkle_root, mimc_merkle_root_padded, mimc_note_commitment,
    mimc_nullifier, bytes_to_fr, fr_to_bytes, u128_to_fr,
    MERKLE_DEPTH, MIMC_ROUNDS,
};
