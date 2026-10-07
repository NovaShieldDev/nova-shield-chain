//! # nova-crypto — Nova Shield Chain Kriptografik Kütüphanesi
//!
//! Whitepaper §6.1'deki tüm kriptografik primitiflerin Rust implementasyonu.
//!
//! ## Modüller
//! - [`hash`] — BLAKE3 wrapper, Merkle tree
//! - [`commitment`] — Pedersen commitment şeması
//! - [`vrf`] — VRF (Verifiable Random Function)
//! - [`signatures`] — ECDSA, Dilithium3, hibrit, BLS
//! - [`threshold`] — ElGamal threshold şifreleme, DKG
//! - [`zk`] — ZK kanıt sistemleri (Groth16, PLONK)
//! - [`symmetric`] — AES-GCM, ChaCha20Poly1305
//! - [`kdf`] — HKDF anahtar türetme

#![warn(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod commitment;
pub mod errors;
pub mod hash;
pub mod kdf;
pub mod signatures;
pub mod symmetric;
pub mod threshold;
pub mod viewing_keys;
pub mod vrf;
pub mod zk;

pub use errors::{CryptoError, CryptoResult};
pub use hash::{blake3_hash, blake3_hash_many, merkle_root, Blake3Hasher, MerkleTree};
pub use signatures::{
    bls::{BlsKeypair, BlsVerifier},
    ecdsa::{EcdsaKeypair, EcdsaVerifier},
    hybrid::{HybridKeypair, HybridVerifier},
};
pub use viewing_keys::{
    AddressDiversifier, DiversifiedAddress,
    FullViewingKey, IncomingViewingKey, NullifierKey, OutgoingViewingKey, SpendAuthKey, SpendKey,
};
