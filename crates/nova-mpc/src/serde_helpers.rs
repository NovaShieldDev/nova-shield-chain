//! Serde yardımcıları — serde 1.0.x'te desteklenmeyen büyük sabit diziler.
//!
//! serde 1.0.228'de [u8; N] için N > 32 Serialize/Deserialize implement edilmemiş.
//! Bu modül, EC noktaları ([u8; 33]) ve diğer büyük sabit diziler için
//! custom serde yardımcıları sağlar.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

// ─── [u8; 33] ─────────────────────────────────────────────────────────────────

/// Sıkıştırılmış secp256k1 noktası (33 byte) için serde yardımcısı.
pub mod bytes_33 {
    use super::*;

    pub fn serialize<S: Serializer>(val: &[u8; 33], s: S) -> Result<S::Ok, S::Error> {
        val.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 33], D::Error> {
        let v: Vec<u8> = Vec::deserialize(d)?;
        v.try_into().map_err(|v: Vec<u8>| {
            de::Error::invalid_length(v.len(), &"tam olarak 33 byte gerekli")
        })
    }
}

// ─── Vec<[u8; 33]> ────────────────────────────────────────────────────────────

/// VSS commitment gibi [u8; 33] vektörleri için serde yardımcısı.
pub mod vec_bytes_33 {
    use super::*;

    pub fn serialize<S: Serializer>(val: &Vec<[u8; 33]>, s: S) -> Result<S::Ok, S::Error> {
        let v: Vec<Vec<u8>> = val.iter().map(|x| x.to_vec()).collect();
        v.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[u8; 33]>, D::Error> {
        let vv: Vec<Vec<u8>> = Vec::deserialize(d)?;
        vv.into_iter()
            .map(|v| {
                v.try_into().map_err(|v: Vec<u8>| {
                    de::Error::invalid_length(v.len(), &"her eleman tam olarak 33 byte olmalı")
                })
            })
            .collect()
    }
}

// ─── [u8; 20] ─────────────────────────────────────────────────────────────────

/// Ethereum adresi (20 byte) için serde yardımcısı.
pub mod bytes_20 {
    use super::*;

    pub fn serialize<S: Serializer>(val: &[u8; 20], s: S) -> Result<S::Ok, S::Error> {
        val.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 20], D::Error> {
        let v: Vec<u8> = Vec::deserialize(d)?;
        v.try_into().map_err(|v: Vec<u8>| {
            de::Error::invalid_length(v.len(), &"tam olarak 20 byte gerekli")
        })
    }
}
