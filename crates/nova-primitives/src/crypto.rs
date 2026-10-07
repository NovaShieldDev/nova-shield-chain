//! Nova Shield Chain — Kriptografik İlkel Tipler
//!
//! Bu modül yalnızca TİP tanımları içerir.
//! Gerçek kriptografik işlemler `nova-crypto` crate'indedir.

use crate::constants::{BLS_PUBKEY_SIZE, BLS_SIGNATURE_SIZE, BLOCK_HASH_SIZE, MPC_PUBKEY_SIZE};
use borsh::{BorshDeserialize, BorshSerialize};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use zeroize::Zeroize;

// ─── HASH ──────────────────────────────────────────────────────────────────────

/// BLAKE3 çıktısı — 32 byte.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    BorshSerialize, BorshDeserialize,
    Encode, Decode,
    Zeroize,
)]
pub struct Hash([u8; BLOCK_HASH_SIZE]);

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; BLOCK_HASH_SIZE] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("hash uzunluğu 32 byte olmalı"))?;
        Ok(Self(arr))
    }
}

impl Hash {
    /// Sıfır hash.
    pub const ZERO: Self = Self([0u8; BLOCK_HASH_SIZE]);

    /// Ham byte'lardan oluştur.
    #[inline]
    pub fn from_bytes(bytes: [u8; BLOCK_HASH_SIZE]) -> Self {
        Self(bytes)
    }

    /// Slice'tan oluştur.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        let arr: [u8; BLOCK_HASH_SIZE] = slice.try_into().ok()?;
        Some(Self(arr))
    }

    /// Ham byte dizisine referans döner.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; BLOCK_HASH_SIZE] {
        &self.0
    }

    /// `0x` ön ekli hex string olarak döndürür.
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }

    /// İlk `n` hex karakterini `0x` ön ekiyle döndürür (kısaltılmış gösterim).
    pub fn short_hex(&self, n: usize) -> String {
        let s = hex::encode(self.0);
        format!("0x{}", &s[..n.min(s.len())])
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.short_hex(16))
    }
}

impl From<[u8; 32]> for Hash {
    fn from(b: [u8; 32]) -> Self { Self(b) }
}

impl From<Hash> for [u8; 32] {
    fn from(h: Hash) -> Self { h.0 }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

// ─── MPC PUBLIC KEY ───────────────────────────────────────────────────────────

/// MPC/secp256k1 compressed public key — 33 byte.
/// CGGMP21 DKG sonunda üretilir.
/// Adres türetimi: BLAKE3(pubkey)[12..32]
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Encode, Decode, Zeroize)]
pub struct PublicKey([u8; MPC_PUBKEY_SIZE]);

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; MPC_PUBKEY_SIZE] = b.try_into().map_err(|_| {
            serde::de::Error::custom("PublicKey 33 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl PublicKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; MPC_PUBKEY_SIZE]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 33 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; MPC_PUBKEY_SIZE] { &self.0 }
    /// secp256k1 compressed prefix byte'ını döner (0x02 veya 0x03).
    pub fn prefix(&self) -> u8 { self.0[0] }
    /// Compressed secp256k1 public key geçerliliğini doğrular.
    pub fn is_valid_compressed(&self) -> bool {
        self.0[0] == 0x02 || self.0[0] == 0x03
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}
impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = hex::encode(self.0);
        write!(f, "PublicKey(0x{}…{})", &h[..8], &h[h.len() - 4..])
    }
}

// ─── BLS PUBLIC KEY ────────────────────────────────────────────────────────────

/// BLS12-381 G1 compressed public key — 48 byte.
/// Validator oylamalarında kullanılır.
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Encode, Decode, Zeroize)]
pub struct BlsPublicKey([u8; BLS_PUBKEY_SIZE]);

impl Serialize for BlsPublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for BlsPublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; BLS_PUBKEY_SIZE] = b.try_into().map_err(|_| {
            serde::de::Error::custom("BlsPublicKey 48 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl BlsPublicKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; BLS_PUBKEY_SIZE]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 48 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; BLS_PUBKEY_SIZE] { &self.0 }
}
impl AsRef<[u8]> for BlsPublicKey {
    fn as_ref(&self) -> &[u8] { &self.0 }
}
impl fmt::Debug for BlsPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlsPublicKey(0x{}…)", &hex::encode(&self.0[..6]))
    }
}

// ─── BLS İMZASI ───────────────────────────────────────────────────────────────

/// BLS12-381 G2 compressed imzası — 96 byte.
#[derive(Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize, Encode, Decode, Zeroize)]
pub struct BlsSignature([u8; BLS_SIGNATURE_SIZE]);

impl Serialize for BlsSignature {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for BlsSignature {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; BLS_SIGNATURE_SIZE] = b.try_into().map_err(|_| {
            serde::de::Error::custom("BlsSignature 96 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl BlsSignature {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; BLS_SIGNATURE_SIZE]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 96 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; BLS_SIGNATURE_SIZE] { &self.0 }
}
impl AsRef<[u8]> for BlsSignature {
    fn as_ref(&self) -> &[u8] { &self.0 }
}
impl fmt::Debug for BlsSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlsSignature(0x{}…)", &hex::encode(&self.0[..8]))
    }
}

// ─── Ed25519 PUBLIC KEY (POLKADOT/SOLANA ANALOG) ─────────────────────────────

/// Ed25519 compressed public key — 32 byte.
///
/// Araştırma kaynağı:
/// - Polkadot: `research/chains/polkadot-sdk/substrate/primitives/core/src/ed25519.rs`
///   `Public([u8; 32])` yapısından alındı.
/// - Solana: `research/chains/solana/sdk/src/pubkey.rs`
///   `Pubkey([u8; 32])` yapısından alındı.
///
/// Kullanım: Guardian'ların Ed25519 bazlı imzaları, Solana köprüsü uyumluluğu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Encode, Decode, Zeroize)]
pub struct Ed25519PublicKey([u8; 32]);

impl Serialize for Ed25519PublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for Ed25519PublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; 32] = b.try_into().map_err(|_| {
            serde::de::Error::custom("Ed25519PublicKey 32 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl Ed25519PublicKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 32 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
    /// Hex string olarak döndürür.
    pub fn to_hex(&self) -> String { hex::encode(self.0) }

    /// Solana base58 formatı (Pubkey::to_string() analog).
    pub fn to_base58(&self) -> String {
        bs58::encode(self.0).into_string()
    }
}

impl fmt::Debug for Ed25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ed25519PK({}...)", &self.to_base58()[..8])
    }
}
impl fmt::Display for Ed25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}
impl AsRef<[u8]> for Ed25519PublicKey {
    fn as_ref(&self) -> &[u8] { &self.0 }
}
impl From<[u8; 32]> for Ed25519PublicKey {
    fn from(b: [u8; 32]) -> Self { Self(b) }
}

// ─── Sr25519 PUBLIC KEY (POLKADOT RISTRETTO ANALOG) ──────────────────────────

/// Sr25519 (Schnorrkel/Ristretto) public key — 32 byte.
///
/// Araştırma kaynağı:
/// - Polkadot: `research/chains/polkadot-sdk/substrate/primitives/core/src/sr25519.rs`
///   `Public([u8; 32])` — Ristretto255 üzerinde Schnorr imzası.
///
/// Polkadot'ta hesap public key'leri genellikle Sr25519 kullanır.
/// Nova'da: guardian key sharing'de kullanılabilir, FROST uyumlu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Encode, Decode, Zeroize)]
pub struct Sr25519PublicKey([u8; 32]);

impl Serialize for Sr25519PublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for Sr25519PublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; 32] = b.try_into().map_err(|_| {
            serde::de::Error::custom("Sr25519PublicKey 32 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl Sr25519PublicKey {
    /// Ham byte dizisinden oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    /// Slice'tan oluşturur; uzunluk 32 değilse `None` döner.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        slice.try_into().ok().map(Self)
    }
    /// Ham byte dizisine referans döner.
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
    /// Hex string olarak döndürür.
    pub fn to_hex(&self) -> String { hex::encode(self.0) }
    /// Polkadot uyumlu base58 formatı.
    pub fn to_base58(&self) -> String { bs58::encode(self.0).into_string() }
}

impl fmt::Debug for Sr25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sr25519PK({}...)", &self.to_base58()[..8])
    }
}
impl fmt::Display for Sr25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}
impl AsRef<[u8]> for Sr25519PublicKey {
    fn as_ref(&self) -> &[u8] { &self.0 }
}
impl From<[u8; 32]> for Sr25519PublicKey {
    fn from(b: [u8; 32]) -> Self { Self(b) }
}

// ─── DİLİTHİUM3 PUBLIC KEY (POST-QUANTUM) ────────────────────────────────────

/// Dilithium3 post-quantum public key — 1952 byte (NIST ML-DSA / CRYSTALS-Dilithium Level-3).
///
/// Araştırma kaynağı: §6.1.2 — Post-Quantum Güvenlik Katmanı
/// NIST ML-DSA standardı: Dilithium3 public key = 1952 byte.
/// (Hata notu: 1312 byte Dilithium2'ye aittir, Dilithium3 = 1952 byte)
///
/// nova-crypto crate'inde gerçek sign/verify implementasyonu yapılacak.
/// Bu sadece tip tanımıdır.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Encode, Decode)]
pub struct DilithiumPublicKey(pub Vec<u8>);

impl DilithiumPublicKey {
    /// Dilithium3 public key boyutu: 1952 byte (NIST ML-DSA Level-3).
    /// Kaynak: nova-crypto/src/signatures/dilithium.rs DILITHIUM3_PK_SIZE = 1952
    pub const SIZE: usize = 1952;

    /// Byte vektöründen oluşturur; boyut `SIZE` değilse `None` döner.
    pub fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() == Self::SIZE { Some(Self(bytes)) } else { None }
    }
    /// Ham byte dilimine referans döner.
    pub fn as_bytes(&self) -> &[u8] { &self.0 }

    /// Geçerli boyutta mı?
    pub fn is_valid(&self) -> bool { self.0.len() == Self::SIZE }
}

impl AsRef<[u8]> for DilithiumPublicKey {
    fn as_ref(&self) -> &[u8] { &self.0 }
}

// ─── İMZA TİPİ ────────────────────────────────────────────────────────────────

/// İmza tipi bayrak byte'ı.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum SignatureType {
    /// Sadece secp256k1 ECDSA (65 byte).
    Ecdsa = 0,
    /// Hibrit: ECDSA + Dilithium3 post-quantum. §6.1.2
    EcdsaDilithium = 1,
    /// BLS12-381 validator oylama imzası (96 byte).
    Bls = 2,
    /// Aggregate BLS (96 byte, n validator → 1 imza).
    AggregateBls = 3,
}

/// Borsh: #[repr(u8)] enum'lar için manuel impl gerekli (borsh 1.x).
impl BorshSerialize for SignatureType {
    fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        writer.write_all(&[*self as u8])
    }
}

impl BorshDeserialize for SignatureType {
    fn deserialize_reader<R: borsh::io::Read>(reader: &mut R) -> borsh::io::Result<Self> {
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;
        match buf[0] {
            0 => Ok(SignatureType::Ecdsa),
            1 => Ok(SignatureType::EcdsaDilithium),
            2 => Ok(SignatureType::Bls),
            3 => Ok(SignatureType::AggregateBls),
            v => Err(borsh::io::Error::new(
                borsh::io::ErrorKind::InvalidData,
                format!("geçersiz SignatureType: {v}"),
            )),
        }
    }
}

// ─── İMZA ─────────────────────────────────────────────────────────────────────

/// Genel imza yapısı (ECDSA / hibrit / BLS).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Encode, Decode)]
pub struct Signature {
    /// İmza ham baytları.
    pub bytes: Vec<u8>,
    /// İmza tipi.
    pub sig_type: SignatureType,
}

impl Zeroize for Signature {
    fn zeroize(&mut self) {
        self.bytes.zeroize();
    }
}

impl Drop for Signature {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl Signature {
    /// ECDSA imzası oluşturur (secp256k1, 65 byte).
    pub fn new_ecdsa(bytes: Vec<u8>) -> Self {
        Self { bytes, sig_type: SignatureType::Ecdsa }
    }
    /// Hibrit ECDSA+Dilithium3 imzası oluşturur. §6.1.2
    pub fn new_hybrid(bytes: Vec<u8>) -> Self {
        Self { bytes, sig_type: SignatureType::EcdsaDilithium }
    }
    /// BLS12-381 imzası oluşturur (96 byte).
    pub fn new_bls(bytes: Vec<u8>) -> Self {
        Self { bytes, sig_type: SignatureType::Bls }
    }
    /// Aggregate BLS imzası oluşturur (n validator → 1 imza).
    pub fn new_aggregate_bls(bytes: Vec<u8>) -> Self {
        Self { bytes, sig_type: SignatureType::AggregateBls }
    }
    /// İmza tipini döner.
    pub fn sig_type(&self) -> SignatureType { self.sig_type }
    /// Ham imza baytlarına referans döner.
    pub fn as_bytes(&self) -> &[u8] { &self.bytes }
    /// İmza bayt uzunluğunu döner.
    pub fn len(&self) -> usize { self.bytes.len() }
    /// İmza boş mu?
    pub fn is_empty(&self) -> bool { self.bytes.is_empty() }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({:?}, {} bytes)", self.sig_type, self.bytes.len())
    }
}
