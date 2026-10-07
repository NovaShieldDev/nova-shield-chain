//! Bitcoin UTXO işlem oluşturucu
//!
//! Bitcoin UTXO modeli:
//! - Her giriş: txid + vout + scriptSig/witness
//! - Her çıkış: value (satoshi) + scriptPubKey
//! - Locktime: CSV/CLTV
//! - Segwit (witness) format

use crate::errors::{GuardError, GuardResult};
use crate::types::{AssetAddress, blake3_hash_many};
use serde::{Deserialize, Serialize};

// ─── BitcoinUtxo ──────────────────────────────────────────────────────────────

/// Bitcoin harcanmamış çıkışı (UTXO)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinUtxo {
    /// İşlem ID (32 byte, little-endian)
    pub txid: [u8; 32],
    /// Çıkış indeksi
    pub vout: u32,
    /// Miktar (satoshi)
    pub value: u64,
    /// scriptPubKey (kilit scripti)
    pub script_pub_key: Vec<u8>,
    /// Confirmations
    pub confirmations: u32,
}

impl BitcoinUtxo {
    pub fn new(txid: [u8; 32], vout: u32, value: u64, script_pub_key: Vec<u8>) -> Self {
        Self { txid, vout, value, script_pub_key, confirmations: 6 }
    }
}

// ─── BitcoinTxInput ───────────────────────────────────────────────────────────

/// Bitcoin işlem girdisi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinTxInput {
    pub txid: [u8; 32],
    pub vout: u32,
    /// scriptSig (legacy) — Segwit için boş
    pub script_sig: Vec<u8>,
    /// Witness veri (Segwit)
    pub witness: Vec<Vec<u8>>,
    /// Sequence (RBF için 0xFFFFFFFD)
    pub sequence: u32,
}

impl BitcoinTxInput {
    pub fn from_utxo(utxo: &BitcoinUtxo) -> Self {
        Self {
            txid: utxo.txid,
            vout: utxo.vout,
            script_sig: vec![],
            witness: vec![],
            sequence: 0xFFFFFFFD, // RBF enabled
        }
    }
}

// ─── BitcoinTxOutput ──────────────────────────────────────────────────────────

/// Bitcoin işlem çıktısı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinTxOutput {
    /// Miktar (satoshi)
    pub value: u64,
    /// scriptPubKey
    pub script_pub_key: Vec<u8>,
}

impl BitcoinTxOutput {
    pub fn new(value: u64, script_pub_key: Vec<u8>) -> Self {
        Self { value, script_pub_key }
    }

    pub fn op_return(data: &[u8]) -> Self {
        // OP_RETURN (0x6A) + veri
        let mut script = vec![0x6A, data.len() as u8];
        script.extend_from_slice(&data[..data.len().min(80)]);
        Self::new(0, script)
    }
}

// ─── BitcoinTx ────────────────────────────────────────────────────────────────

/// Bitcoin işlemi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinTx {
    /// Versiyon (1 veya 2 — segwit için 2)
    pub version: u32,
    pub inputs: Vec<BitcoinTxInput>,
    pub outputs: Vec<BitcoinTxOutput>,
    /// Locktime
    pub locktime: u32,
    /// MPC imzaları (sıralı guardian imzaları)
    pub signatures: Vec<Vec<u8>>,
}

impl BitcoinTx {
    pub fn new(inputs: Vec<BitcoinTxInput>, outputs: Vec<BitcoinTxOutput>) -> Self {
        Self { version: 2, inputs, outputs, locktime: 0, signatures: vec![] }
    }

    /// TXID: BLAKE3(version || inputs || outputs || locktime)
    pub fn compute_txid(&self) -> [u8; 32] {
        let ver = self.version.to_le_bytes();
        let input_count = self.inputs.len() as u32;
        let output_count = self.outputs.len() as u32;
        let total_value: u64 = self.outputs.iter().map(|o| o.value).sum();

        blake3_hash_many(&[
            b"bitcoin-txid-v1",
            &ver,
            &input_count.to_le_bytes(),
            &output_count.to_le_bytes(),
            &total_value.to_le_bytes(),
            &self.locktime.to_le_bytes(),
        ])
    }

    /// Sighash (imzalanacak mesaj) — BIP143 Segwit mock
    pub fn sighash(&self) -> [u8; 32] {
        let txid = self.compute_txid();
        blake3_hash_many(&[b"bitcoin-sighash-bip143", &txid])
    }

    /// MPC imzasını ekle
    pub fn add_signature(&mut self, sig: Vec<u8>) {
        self.signatures.push(sig);
    }

    /// Serileştir (mock: JSON)
    pub fn serialize(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Toplam girdi değeri
    pub fn total_output_value(&self) -> u64 {
        self.outputs.iter().map(|o| o.value).sum()
    }
}

// ─── BitcoinTxBuilder ─────────────────────────────────────────────────────────

/// Bitcoin işlem inşaatçısı
/// §5.6.3: "1. Nova Shield'de BTC gönder seçilir → 4. Bitcoin ağına broadcast edilir"
pub struct BitcoinTxBuilder {
    utxos: Vec<BitcoinUtxo>,
    fee_rate_sat_vb: u64, // satoshi/vbyte
}

impl BitcoinTxBuilder {
    pub fn new(fee_rate_sat_vb: u64) -> Self {
        Self { utxos: vec![], fee_rate_sat_vb }
    }

    pub fn add_utxo(&mut self, utxo: BitcoinUtxo) {
        self.utxos.push(utxo);
    }

    /// İşlem oluştur — coin selection (greedy)
    pub fn build(
        &self,
        to: &AssetAddress,
        amount_satoshi: u64,
        change_script: Vec<u8>,
    ) -> GuardResult<BitcoinTx> {
        // Girdi seç
        let mut selected = vec![];
        let mut total_in = 0u64;

        // Tahmini boyut: 10 + 148*inputs + 34*outputs byte
        let estimated_fee = self.fee_rate_sat_vb * (10 + 148 + 34 * 2);

        let required = amount_satoshi.saturating_add(estimated_fee);

        for utxo in &self.utxos {
            if total_in >= required {
                break;
            }
            selected.push(utxo.clone());
            total_in += utxo.value;
        }

        if total_in < required {
            return Err(GuardError::InsufficientBalance {
                available: total_in,
                required,
            });
        }

        let inputs: Vec<BitcoinTxInput> = selected.iter().map(BitcoinTxInput::from_utxo).collect();

        // P2WSH scriptPubKey (alıcı adresi)
        let to_script = if to.raw.len() >= 32 {
            crate::bitcoin::address::build_p2wsh_script(&to.raw)
        } else {
            let mut padded = vec![0u8; 32];
            padded[..to.raw.len()].copy_from_slice(&to.raw);
            crate::bitcoin::address::build_p2wsh_script(&padded)
        };

        let mut outputs = vec![
            BitcoinTxOutput::new(amount_satoshi, to_script),
        ];

        // Para üstü
        let change = total_in.saturating_sub(amount_satoshi + estimated_fee);
        if change > 546 { // dust threshold
            outputs.push(BitcoinTxOutput::new(change, change_script));
        }

        Ok(BitcoinTx::new(inputs, outputs))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssetAddress;

    fn dummy_utxo(value: u64) -> BitcoinUtxo {
        BitcoinUtxo::new([1u8; 32], 0, value, vec![0x00, 0x20, 0xAA])
    }

    fn btc_addr() -> AssetAddress {
        AssetAddress::bitcoin("bc1qtest")
    }

    #[test]
    fn test_bitcoin_tx_compute_txid() {
        let tx = BitcoinTx::new(
            vec![BitcoinTxInput::from_utxo(&dummy_utxo(100_000))],
            vec![BitcoinTxOutput::new(90_000, vec![0x00, 0x14])],
        );
        let txid = tx.compute_txid();
        assert_ne!(txid, [0u8; 32]);
    }

    #[test]
    fn test_bitcoin_tx_txid_deterministic() {
        let tx = BitcoinTx::new(
            vec![BitcoinTxInput::from_utxo(&dummy_utxo(100_000))],
            vec![BitcoinTxOutput::new(90_000, vec![0x00, 0x14])],
        );
        assert_eq!(tx.compute_txid(), tx.compute_txid());
    }

    #[test]
    fn test_bitcoin_tx_sighash() {
        let tx = BitcoinTx::new(vec![], vec![]);
        let sighash = tx.sighash();
        assert_ne!(sighash, [0u8; 32]);
    }

    #[test]
    fn test_bitcoin_tx_builder_success() {
        let mut builder = BitcoinTxBuilder::new(5);
        builder.add_utxo(dummy_utxo(500_000));

        let to = btc_addr();
        let change_script = vec![0x00u8; 22];
        let tx = builder.build(&to, 100_000, change_script).unwrap();

        assert!(!tx.inputs.is_empty());
        assert!(!tx.outputs.is_empty());
    }

    #[test]
    fn test_bitcoin_tx_builder_insufficient() {
        let mut builder = BitcoinTxBuilder::new(5);
        builder.add_utxo(dummy_utxo(1_000)); // Çok az

        let to = btc_addr();
        let change_script = vec![0x00];
        let result = builder.build(&to, 100_000, change_script);

        assert!(matches!(result, Err(GuardError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_bitcoin_tx_output_value() {
        let tx = BitcoinTx::new(
            vec![],
            vec![
                BitcoinTxOutput::new(50_000, vec![]),
                BitcoinTxOutput::new(30_000, vec![]),
            ],
        );
        assert_eq!(tx.total_output_value(), 80_000);
    }

    #[test]
    fn test_bitcoin_tx_op_return() {
        let out = BitcoinTxOutput::op_return(b"nova-guard-test");
        assert_eq!(out.value, 0);
        assert_eq!(out.script_pub_key[0], 0x6A); // OP_RETURN
    }

    #[test]
    fn test_bitcoin_tx_add_signature() {
        let mut tx = BitcoinTx::new(vec![], vec![]);
        tx.add_signature(vec![0x30u8; 71]);
        tx.add_signature(vec![0x30u8; 71]);
        assert_eq!(tx.signatures.len(), 2);
    }

    #[test]
    fn test_bitcoin_tx_serialize() {
        let tx = BitcoinTx::new(vec![], vec![]);
        let bytes = tx.serialize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_utxo_from_builder() {
        let mut builder = BitcoinTxBuilder::new(1);
        // Birden fazla UTXO
        builder.add_utxo(dummy_utxo(50_000));
        builder.add_utxo(dummy_utxo(100_000));

        let to = btc_addr();
        let tx = builder.build(&to, 120_000, vec![0x00u8; 22]).unwrap();
        // İki girdi seçilmeli
        assert_eq!(tx.inputs.len(), 2);
    }
}
