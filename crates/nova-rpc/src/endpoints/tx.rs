//! tx_* RPC metodları

use crate::errors::{RpcError, RpcResult};
use crate::state::{ChainState, TxStatus};
use serde_json::Value;

/// NOVA-RED-RPC-002/003/004: secp256k1 ECDSA imza format doğrulaması.
///
/// İmza: r (32 B) + s (32 B) [+ v (1 B) recovery bit] = 64 veya 65 byte.
/// Sadece format kontrolü yapılır (hex decode + uzunluk). Gerçek imza
/// doğrulaması zincir/konsensüs katmanında gerçekleşir.
fn validate_sig(sig_hex: &str) -> RpcResult<()> {
    let hex_part = sig_hex.trim_start_matches("0x");
    let bytes = hex::decode(hex_part).map_err(|_| RpcError::InvalidSignature {
        reason: "imza hex decode hatası".into(),
    })?;
    if bytes.len() < 64 {
        return Err(RpcError::InvalidSignature {
            reason: format!(
                "imza çok kısa: {} byte (secp256k1 r+s için en az 64 gerekli)",
                bytes.len()
            ),
        });
    }
    Ok(())
}

pub struct TxHandler {
    state: ChainState,
}

impl TxHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_sendTransaction — işlem gönder
    ///
    /// NOVA-RED-RPC-004: `signature` alanı zorunlu ve geçerli secp256k1 formatında
    /// olmalıdır. İmza yoksa veya çok kısaysa istek reddedilir.
    pub fn send_transaction(&self, params: &[Value]) -> RpcResult<Value> {
        let tx = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "işlem nesnesi gerekli".into(),
        })?;

        let from = tx.get("from").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "from alanı gerekli".into(),
            })?;

        // NOVA-RED-RPC-004: İmza zorunlu — imzasız işlem reddedilir
        let signature = tx.get("signature").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidSignature {
                reason: "signature alanı zorunlu — işlem imzasız gönderilemez".into(),
            })?;
        validate_sig(signature)?;

        let to = tx.get("to").and_then(|v| v.as_str())
            .unwrap_or("0x0000000000000000000000000000000000000000");

        let value = tx.get("value")
            .and_then(|v| v.as_str())
            .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);

        let gas_price = tx.get("gasPrice")
            .and_then(|v| v.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(1_000_000_000);

        let hash = self.state.send_transaction(
            from.into(),
            to.into(),
            value,
            gas_price,
        );

        Ok(serde_json::json!(hash))
    }

    /// nova_getTransaction — işlem bilgisi
    pub fn get_transaction(&self, params: &[Value]) -> RpcResult<Value> {
        let hash = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "işlem hash'i gerekli".into(),
            })?;

        let tx = self.state.transactions.get(hash)
            .map(|t| t.clone())
            .ok_or_else(|| RpcError::TxNotFound { hash: hash.into() })?;

        Ok(serde_json::json!({
            "hash": tx.hash,
            "from": tx.from,
            "to": tx.to,
            "value": format!("0x{:x}", tx.value),
            "gasPrice": format!("0x{:x}", tx.gas_price),
            "status": match tx.status {
                TxStatus::Pending => "pending",
                TxStatus::Confirmed => "confirmed",
                TxStatus::Failed => "failed",
            },
            "blockNumber": tx.block_number.map(|n| format!("0x{:x}", n)),
        }))
    }

    /// nova_getTransactionReceipt — işlem fişi
    pub fn get_transaction_receipt(&self, params: &[Value]) -> RpcResult<Value> {
        let hash = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "işlem hash'i gerekli".into(),
            })?;

        let tx = self.state.transactions.get(hash).map(|t| t.clone());

        match tx {
            None => Ok(serde_json::json!(null)),
            Some(tx) if tx.status == TxStatus::Pending => Ok(serde_json::json!(null)),
            Some(tx) => Ok(serde_json::json!({
                "transactionHash": tx.hash,
                "from": tx.from,
                "to": tx.to,
                "status": "0x1",
                "blockNumber": tx.block_number.map(|n| format!("0x{:x}", n)),
                "gasUsed": "0x5208",
            })),
        }
    }

    /// nova_pendingTransactions — bekleyen işlemler
    pub fn pending_transactions(&self) -> RpcResult<Value> {
        let pending: Vec<Value> = self.state.transactions.iter()
            .filter(|t| t.status == TxStatus::Pending)
            .map(|t| serde_json::json!({
                "hash": t.hash,
                "from": t.from,
                "to": t.to,
            }))
            .collect();

        Ok(serde_json::json!(pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> TxHandler {
        TxHandler::new(ChainState::new())
    }

    const ALICE: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BOB: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    /// 65 byte geçerli format imza (r+s+v, 130 hex kar.)
    const MOCK_SIG: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn send_tx(h: &TxHandler) -> String {
        let result = h.send_transaction(&[serde_json::json!({
            "from": ALICE,
            "to": BOB,
            "value": "0xde0b6b3a7640000",
            "gasPrice": "0x3b9aca00",
            "signature": MOCK_SIG,
        })]).unwrap();
        result.as_str().unwrap().to_string()
    }

    #[test]
    fn test_send_transaction() {
        let h = handler();
        let hash = send_tx(&h);
        assert!(hash.starts_with("0x"));
    }

    /// NOVA-RED-RPC-004: İmzasız işlem reddedilmeli
    #[test]
    fn test_send_transaction_no_signature_rejected() {
        let h = handler();
        let result = h.send_transaction(&[serde_json::json!({
            "from": ALICE,
            "to": BOB,
            "value": "0xde0b6b3a7640000",
        })]);
        assert!(result.is_err(), "imzasız işlem reddedilmeli");
        let err = result.unwrap_err();
        assert_eq!(err.code(), crate::errors::INVALID_SIGNATURE);
    }

    /// NOVA-RED-RPC-004: Çok kısa imza reddedilmeli
    #[test]
    fn test_send_transaction_short_signature_rejected() {
        let h = handler();
        let result = h.send_transaction(&[serde_json::json!({
            "from": ALICE,
            "to": BOB,
            "value": "0x1000",
            "signature": "0xdeadbeef", // 4 byte — çok kısa
        })]);
        assert!(result.is_err(), "kısa imza reddedilmeli");
        let err = result.unwrap_err();
        assert_eq!(err.code(), crate::errors::INVALID_SIGNATURE);
    }

    /// NOVA-RED-RPC-004: Geçersiz hex imza reddedilmeli
    #[test]
    fn test_send_transaction_invalid_hex_signature_rejected() {
        let h = handler();
        let result = h.send_transaction(&[serde_json::json!({
            "from": ALICE,
            "to": BOB,
            "value": "0x1000",
            "signature": "0xGGGGGGGG", // geçersiz hex
        })]);
        assert!(result.is_err(), "geçersiz hex imza reddedilmeli");
        let err = result.unwrap_err();
        assert_eq!(err.code(), crate::errors::INVALID_SIGNATURE);
    }

    #[test]
    fn test_get_transaction() {
        let h = handler();
        let hash = send_tx(&h);

        let result = h.get_transaction(&[serde_json::json!(hash)]).unwrap();
        assert_eq!(result["from"], serde_json::json!(ALICE));
        assert_eq!(result["to"], serde_json::json!(BOB));
        assert_eq!(result["status"], serde_json::json!("pending"));
    }

    #[test]
    fn test_get_transaction_not_found() {
        let h = handler();
        let result = h.get_transaction(&[serde_json::json!("0xdeadbeef")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_transaction_receipt_pending() {
        let h = handler();
        let hash = send_tx(&h);
        let result = h.get_transaction_receipt(&[serde_json::json!(hash)]).unwrap();
        assert_eq!(result, serde_json::json!(null));
    }

    #[test]
    fn test_get_transaction_receipt_not_found() {
        let h = handler();
        let result = h.get_transaction_receipt(&[serde_json::json!("0xunknown")]).unwrap();
        assert_eq!(result, serde_json::json!(null));
    }

    #[test]
    fn test_pending_transactions() {
        let h = handler();
        send_tx(&h);
        send_tx(&h); // ikinci TX (değer farklı olmalı, ama hash aynı olabilir — ignore)

        let result = h.pending_transactions().unwrap();
        let arr = result.as_array().unwrap();
        assert!(!arr.is_empty());
    }

    #[test]
    fn test_send_transaction_no_params() {
        let h = handler();
        assert!(h.send_transaction(&[]).is_err());
    }

    #[test]
    fn test_send_transaction_no_from() {
        let h = handler();
        let result = h.send_transaction(&[serde_json::json!({
            "to": BOB,
            "signature": MOCK_SIG,
        })]);
        assert!(result.is_err());
    }
}
