//! chain_* RPC metodları

use crate::errors::{RpcError, RpcResult};
use crate::state::ChainState;
use serde_json::Value;

pub struct ChainHandler {
    state: ChainState,
    chain_id: u64,
}

impl ChainHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state, chain_id: nova_primitives::constants::MAINNET_CHAIN_ID }
    }

    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// nova_blockNumber — güncel blok numarası
    pub fn block_number(&self) -> RpcResult<Value> {
        let n = self.state.best_block_number();
        Ok(serde_json::json!(format!("0x{:x}", n)))
    }

    /// nova_getBlock — blok bilgisi (number veya hash ile)
    pub fn get_block(&self, params: &[Value]) -> RpcResult<Value> {
        let block_id = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "blok ID gerekli".into(),
            })?;

        let block_num: u64 = if block_id.starts_with("0x") {
            u64::from_str_radix(block_id.trim_start_matches("0x"), 16)
                .map_err(|_| RpcError::InvalidParams {
                    reason: "geçersiz blok numarası".into(),
                })?
        } else if block_id == "latest" {
            self.state.best_block_number()
        } else if block_id == "earliest" {
            0
        } else {
            block_id.parse::<u64>().map_err(|_| RpcError::InvalidParams {
                reason: "geçersiz blok tanımlayıcısı".into(),
            })?
        };

        let block = self.state.get_block_at(block_num)
            .ok_or_else(|| RpcError::BlockNotFound {
                id: block_id.into(),
            })?;

        Ok(serde_json::json!({
            "number": format!("0x{:x}", block.number),
            "hash": block.hash,
            "parentHash": block.parent_hash,
            "timestamp": format!("0x{:x}", block.timestamp),
            "transactions": block.transactions,
            "validator": block.validator,
            "transactionCount": block.tx_count,
        }))
    }

    /// nova_chainId — zincir ID (EIP-695 uyumlu hex formatı)
    pub fn chain_id(&self) -> RpcResult<Value> {
        Ok(serde_json::json!(format!("0x{:x}", self.chain_id)))
    }

    /// nova_syncing — sync durumu
    pub fn syncing(&self) -> RpcResult<Value> {
        // Mock: her zaman synced
        Ok(serde_json::json!(false))
    }

    /// nova_getSlot — slot bilgisi
    pub fn get_slot(&self) -> RpcResult<Value> {
        let best = self.state.best_block_number();
        Ok(serde_json::json!({
            "slot": best,
            "slotDurationMs": 400,
            "epochLength": 200,
            "currentEpoch": best / 200,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> ChainHandler {
        let state = ChainState::new();
        ChainHandler::new(state)
    }

    #[test]
    fn test_block_number_genesis() {
        let h = handler();
        let result = h.block_number().unwrap();
        assert_eq!(result, serde_json::json!("0x0"));
    }

    #[test]
    fn test_block_number_after_produce() {
        let state = ChainState::new();
        state.produce_block();
        state.produce_block();
        let h = ChainHandler::new(state);
        let result = h.block_number().unwrap();
        assert_eq!(result, serde_json::json!("0x2"));
    }

    #[test]
    fn test_get_block_genesis() {
        let h = handler();
        let result = h.get_block(&[serde_json::json!("0x0")]).unwrap();
        assert_eq!(result["number"], serde_json::json!("0x0"));
    }

    #[test]
    fn test_get_block_latest() {
        let h = handler();
        let result = h.get_block(&[serde_json::json!("latest")]).unwrap();
        assert_eq!(result["number"], serde_json::json!("0x0"));
    }

    #[test]
    fn test_get_block_not_found() {
        let h = handler();
        let result = h.get_block(&[serde_json::json!("0x999")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_block_no_params() {
        let h = handler();
        let result = h.get_block(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_chain_id_mainnet_default() {
        let h = handler();
        // Varsayılan chain_id = MAINNET_CHAIN_ID = 1984 = 0x7c0
        let result = h.chain_id().unwrap();
        assert_eq!(result, serde_json::json!(format!("0x{:x}", nova_primitives::constants::MAINNET_CHAIN_ID)));
    }

    #[test]
    fn test_chain_id_custom() {
        let h = handler().with_chain_id(19840); // testnet
        let result = h.chain_id().unwrap();
        assert_eq!(result, serde_json::json!("0x4d80")); // 19840 = 0x4d80
    }

    #[test]
    fn test_syncing() {
        let h = handler();
        let result = h.syncing().unwrap();
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn test_get_slot() {
        let h = handler();
        let result = h.get_slot().unwrap();
        assert_eq!(result["slotDurationMs"], serde_json::json!(400));
    }
}
