//! R1CS devreleri — arkworks BLS12-381 Groth16 için 8 adet ConstraintSynthesizer
//!
//! Her devre `setup_dummy()` (trusted setup) ve `new(...)` (kanıt üretimi) destekler.
//!
//! Devreler:
//! 1. MintCircuit           — Transparent → Shielded
//! 2. SpendCircuit          — Shielded not sahipliği + nullifier + Merkle
//! 3. TransferCircuit       — Shielded → Shielded
//! 4. BurnCircuit           — Shielded → Transparent
//! 5. StakeCircuit          — Shielded stake
//! 6. UnstakeCircuit        — Shielded unstake
//! 7. GovernanceVoteCircuit — Kimlik gizli oy
//! 8. NullifierDerivationCircuit — Bağımsız nullifier kanıtı

pub mod burn;
pub mod governance;
pub mod mint;
pub mod nullifier;
pub mod spend;
pub mod stake;
pub mod transfer;
pub mod unstake;

pub use burn::{BurnCircuit, compute_recipient_hash};
pub use governance::{GovernanceVoteCircuit, compute_vote_commitment, compute_vote_nullifier};
pub use mint::MintCircuit;
pub use nullifier::NullifierDerivationCircuit;
pub use spend::SpendCircuit;
pub use stake::{StakeCircuit, compute_stake_nullifier, compute_validator_hash};
pub use transfer::TransferCircuit;
pub use unstake::UnstakeCircuit;
