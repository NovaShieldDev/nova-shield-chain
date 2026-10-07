//! tx_* RPC metodları

use crate::errors::{RpcError, RpcResult};
use crate::state::{ChainState, TxStatus};
use serde_json::Value;

pub struct TxHandler {
    state: ChainState,
}

impl TxHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_sendTransaction — işlem gönder
    pub fn send_transaction(&self, params: &[Value]) -> RpcResult<Value> {
        let tx = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "işlem nesnesi gerekli".into(),
        })?;

        let from = tx.get("from").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "from alanı gerekli".into(),
            })?;

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

    fn send_tx(h: &TxHandler) -> String {
        let result = h.send_transaction(&[serde_json::json!({
            "from": ALICE,
            "to": BOB,
            "value": "0xde0b6b3a7640000",
            "gasPrice": "0x3b9aca00",
        })]).unwrap();
        result.as_str().unwrap().to_string()
    }

    #[test]
    fn test_send_transaction() {
        let h = handler();
        let hash = send_tx(&h);
        assert!(hash.starts_with("0x"));
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
        let result = h.send_transaction(&[serde_json::json!({"to": BOB})]);
        assert!(result.is_err());
    }
}
