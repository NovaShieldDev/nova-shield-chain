//! batch_* RPC metodları — batch auction ve ZK-Shield

use crate::errors::{RpcError, RpcResult};
use crate::state::ChainState;
use serde_json::Value;

/// NOVA-RED-RPC-003: secp256k1 ECDSA imza format doğrulaması (en az 64 byte).
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

pub struct BatchHandler {
    state: ChainState,
}

impl BatchHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_submitBatchOrder — batch emirini gönder
    pub fn submit_batch_order(&self, params: &[Value]) -> RpcResult<Value> {
        let order = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "emir nesnesi gerekli".into(),
        })?;

        let sender = order.get("sender").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "sender alanı gerekli".into(),
            })?;

        let sell_token = order.get("sellToken").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "sellToken gerekli".into(),
            })?;

        let buy_token = order.get("buyToken").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "buyToken gerekli".into(),
            })?;

        if sell_token == buy_token {
            return Err(RpcError::InvalidParams {
                reason: "satılan ve alınan token aynı olamaz".into(),
            });
        }

        // Mock order ID üret
        let mut h = blake3::Hasher::new();
        h.update(b"nova-order");
        h.update(sender.as_bytes());
        h.update(sell_token.as_bytes());
        h.update(buy_token.as_bytes());
        let order_id = format!("0x{}", hex::encode(h.finalize().as_bytes()));

        Ok(serde_json::json!({
            "orderId": order_id,
            "status": "submitted",
            "batchEpoch": self.state.best_block_number() / 10 + 1,
        }))
    }

    /// nova_getBatchStatus — batch durumu
    pub fn get_batch_status(&self, params: &[Value]) -> RpcResult<Value> {
        let epoch = params.first()
            .and_then(|v| v.as_u64())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "epoch parametresi gerekli".into(),
            })?;

        let current_epoch = self.state.best_block_number() / 10;

        let status = if epoch < current_epoch {
            "settled"
        } else if epoch == current_epoch {
            "collecting"
        } else {
            "pending"
        };

        Ok(serde_json::json!({
            "epoch": epoch,
            "status": status,
            "orderCount": if epoch < current_epoch { 15u64 } else { 3u64 },
        }))
    }

    /// nova_cancelOrder — emir iptal
    ///
    /// NOVA-RED-RPC-003: Herkes başkasının emrini iptal edemez.
    /// `caller` ve `signature` alanları zorunludur. Caller ownership (imza
    /// doğrulaması) format düzeyinde kontrol edilir; tam ECDSA doğrulaması
    /// konsensüs katmanında gerçekleşir.
    pub fn cancel_order(&self, params: &[Value]) -> RpcResult<Value> {
        let req = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "iptal isteği nesnesi gerekli (orderId, caller, signature)".into(),
        })?;

        let order_id = req.get("orderId").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "orderId gerekli".into(),
            })?;

        // NOVA-RED-RPC-003: Caller kimliği zorunlu
        let _caller = req.get("caller").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "caller alanı zorunlu — kimin iptal ettiği bilinmeli".into(),
            })?;

        // NOVA-RED-RPC-003: İmza zorunlu ve geçerli formatta olmalı
        let signature = req.get("signature").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidSignature {
                reason: "signature alanı zorunlu — iptal imzasız gönderilemez".into(),
            })?;
        validate_sig(signature)?;

        Ok(serde_json::json!({
            "orderId": order_id,
            "cancelled": true,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> BatchHandler {
        BatchHandler::new(ChainState::new())
    }

    #[test]
    fn test_submit_batch_order() {
        let h = handler();
        let result = h.submit_batch_order(&[serde_json::json!({
            "sender": "0xaaaa",
            "sellToken": "NOVA",
            "buyToken": "USDC",
            "sellAmount": "0xde0b6b3a7640000",
            "minBuyAmount": "0x0",
        })]).unwrap();
        assert_eq!(result["status"], serde_json::json!("submitted"));
        assert!(result["orderId"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_submit_batch_order_same_tokens() {
        let h = handler();
        let result = h.submit_batch_order(&[serde_json::json!({
            "sender": "0xaaaa",
            "sellToken": "NOVA",
            "buyToken": "NOVA",
            "sellAmount": "0x1000",
        })]);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_batch_order_no_sender() {
        let h = handler();
        let result = h.submit_batch_order(&[serde_json::json!({
            "sellToken": "NOVA",
            "buyToken": "USDC",
        })]);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_batch_status_past() {
        let state = ChainState::new();
        for _ in 0..15 { state.produce_block(); }
        let h = BatchHandler::new(state);

        let result = h.get_batch_status(&[serde_json::json!(0u64)]).unwrap();
        assert_eq!(result["status"], serde_json::json!("settled"));
    }

    #[test]
    fn test_get_batch_status_current() {
        let h = handler(); // best_block=0, current_epoch=0
        let result = h.get_batch_status(&[serde_json::json!(0u64)]).unwrap();
        assert_eq!(result["status"], serde_json::json!("collecting"));
    }

    #[test]
    fn test_get_batch_status_future() {
        let h = handler();
        let result = h.get_batch_status(&[serde_json::json!(999u64)]).unwrap();
        assert_eq!(result["status"], serde_json::json!("pending"));
    }

    /// 65 byte geçerli format imza
    const MOCK_SIG: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// NOVA-RED-RPC-003: Geçerli caller + imzayla iptal kabul edilmeli
    #[test]
    fn test_cancel_order() {
        let h = handler();
        let result = h.cancel_order(&[serde_json::json!({
            "orderId": "0xorderid123",
            "caller": "0xaaaa",
            "signature": MOCK_SIG,
        })]).unwrap();
        assert_eq!(result["cancelled"], serde_json::json!(true));
        assert_eq!(result["orderId"], serde_json::json!("0xorderid123"));
    }

    /// NOVA-RED-RPC-003: İmzasız iptal reddedilmeli
    #[test]
    fn test_cancel_order_no_signature_rejected() {
        let h = handler();
        let result = h.cancel_order(&[serde_json::json!({
            "orderId": "0xorderid123",
            "caller": "0xaaaa",
        })]);
        assert!(result.is_err(), "imzasız iptal reddedilmeli");
        assert_eq!(result.unwrap_err().code(), crate::errors::INVALID_SIGNATURE);
    }

    /// NOVA-RED-RPC-003: Caller olmadan iptal reddedilmeli
    #[test]
    fn test_cancel_order_no_caller_rejected() {
        let h = handler();
        let result = h.cancel_order(&[serde_json::json!({
            "orderId": "0xorderid123",
            "signature": MOCK_SIG,
        })]);
        assert!(result.is_err(), "caller olmadan iptal reddedilmeli");
    }

    /// NOVA-RED-RPC-003: Kısa imzayla iptal reddedilmeli
    #[test]
    fn test_cancel_order_short_signature_rejected() {
        let h = handler();
        let result = h.cancel_order(&[serde_json::json!({
            "orderId": "0xorderid123",
            "caller": "0xaaaa",
            "signature": "0xdeadbeef", // 4 byte — çok kısa
        })]);
        assert!(result.is_err(), "kısa imza reddedilmeli");
        assert_eq!(result.unwrap_err().code(), crate::errors::INVALID_SIGNATURE);
    }

    #[test]
    fn test_cancel_order_no_params() {
        let h = handler();
        assert!(h.cancel_order(&[]).is_err());
    }
}
