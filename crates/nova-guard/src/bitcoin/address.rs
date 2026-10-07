//! Bitcoin MPC tabanlı adres türetimi
//!
//! §5.6.3 — Nova Shield Guard MPC mekanizması
//! Gerçek bir Bitcoin kütüphanesi olmadan BLAKE3 mock türetmesi:
//! - `derive_mpc_address(guardians, threshold)` → P2WSH benzeri adres
//! - Script: `OP_M <pubkey1> .. <pubkeyN> OP_N OP_CHECKMULTISIG`

use crate::types::{AssetAddress, ChainId, MpcThreshold, blake3_hash_many};

/// Bitcoin MPC adresi türet
/// §5.6.3: threshold t/n → MPC kontrollü Bitcoin adresi
/// Mock: BLAKE3(guardians || threshold) → 32-byte script hash → bech32-like display
pub fn derive_mpc_btc_address(threshold: &MpcThreshold) -> AssetAddress {
    // Script: t-of-n multisig mock — BLAKE3(threshold || guardian_bytes)
    let mut inputs: Vec<&[u8]> = vec![b"nova-btc-mpc-v1"];
    let t_bytes = threshold.threshold.to_le_bytes();
    let n_bytes = threshold.total.to_le_bytes();
    inputs.push(&t_bytes);
    inputs.push(&n_bytes);

    let guardian_bytes: Vec<Vec<u8>> = threshold.guardians.iter().map(|g| g.to_vec()).collect();
    let flat_guardians: Vec<u8> = guardian_bytes.into_iter().flatten().collect();

    let script_hash = blake3_hash_many(&[b"nova-btc-mpc-v1", &t_bytes, &n_bytes, &flat_guardians]);

    // Segwit v0 P2WSH address: "bc1q" + hex(script_hash[:16])
    let display = format!("bc1q{}", hex::encode(&script_hash[..16]));

    AssetAddress {
        chain: ChainId::Bitcoin,
        raw: script_hash.to_vec(),
        display,
    }
}

/// Bitcoin scriptPubKey oluştur (mock P2WSH)
/// Gerçekte: OP_0 <32-byte-witness-script-hash>
pub fn build_p2wsh_script(script_hash: &[u8]) -> Vec<u8> {
    let mut script = Vec::with_capacity(34);
    script.push(0x00); // OP_0 (witness version)
    script.push(0x20); // 32 byte push
    script.extend_from_slice(&script_hash[..32.min(script_hash.len())]);
    script
}

/// Multisig redeem script mock
/// Gerçekte: OP_M <pubkey> ... OP_N OP_CHECKMULTISIG
pub fn build_multisig_redeem_script(threshold: &MpcThreshold) -> Vec<u8> {
    let mut script = vec![];

    // OP_M: threshold sayısı (OP_1=0x51 ... OP_16=0x60)
    script.push(0x50 + threshold.threshold as u8);

    // Her guardian pubkey'i (mock: guardian adresi genişletilmiş 33 byte)
    for guardian in &threshold.guardians {
        script.push(0x21); // 33 byte push
        script.push(0x02); // compressed pubkey prefix
        script.extend_from_slice(guardian);
        // Kalan 12 byte dolgu
        script.extend_from_slice(&[0u8; 12]);
    }

    // OP_N: total guardian sayısı
    script.push(0x50 + threshold.total as u8);

    // OP_CHECKMULTISIG
    script.push(0xAE);

    script
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_threshold(t: u32, n: u32) -> MpcThreshold {
        let g: Vec<[u8; 20]> = (0..n).map(|i| [i as u8; 20]).collect();
        MpcThreshold::new(t, n, g).unwrap()
    }

    #[test]
    fn test_derive_mpc_btc_address_format() {
        let t = make_threshold(3, 5);
        let addr = derive_mpc_btc_address(&t);
        assert_eq!(addr.chain, ChainId::Bitcoin);
        assert!(addr.display.starts_with("bc1q"));
    }

    #[test]
    fn test_derive_mpc_btc_address_deterministic() {
        let t = make_threshold(3, 5);
        let a1 = derive_mpc_btc_address(&t);
        let a2 = derive_mpc_btc_address(&t);
        assert_eq!(a1.display, a2.display);
        assert_eq!(a1.raw, a2.raw);
    }

    #[test]
    fn test_different_thresholds_different_addresses() {
        let t1 = make_threshold(2, 3);
        let t2 = make_threshold(3, 5);
        let a1 = derive_mpc_btc_address(&t1);
        let a2 = derive_mpc_btc_address(&t2);
        assert_ne!(a1.display, a2.display);
    }

    #[test]
    fn test_p2wsh_script_format() {
        let hash = [0xAAu8; 32];
        let script = build_p2wsh_script(&hash);
        assert_eq!(script[0], 0x00); // OP_0
        assert_eq!(script[1], 0x20); // 32 byte push
        assert_eq!(script.len(), 34);
    }

    #[test]
    fn test_multisig_redeem_script() {
        let t = make_threshold(2, 3);
        let script = build_multisig_redeem_script(&t);
        // OP_2=0x52
        assert_eq!(script[0], 0x52);
        // Son byte OP_CHECKMULTISIG=0xAE
        assert_eq!(*script.last().unwrap(), 0xAE);
    }
}
