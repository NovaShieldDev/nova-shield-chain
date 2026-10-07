//! JSON-RPC istemci türleri
//!
//! Ethereum JSON-RPC standardı (eth_* metodları) + Nova Shield eklentileri

use crate::errors::{SdkError, SdkResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── RpcRequest ───────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 isteği
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<Value>,
    pub id: u64,
}

impl RpcRequest {
    pub fn new(method: impl Into<String>, params: Vec<Value>, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id,
        }
    }

    pub fn serialize(&self) -> SdkResult<String> {
        serde_json::to_string(self).map_err(|e| SdkError::SerializationError {
            reason: e.to_string(),
        })
    }
}

// ─── RpcError ─────────────────────────────────────────────────────────────────

/// JSON-RPC hata nesnesi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorBody {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

// ─── RpcResponse ──────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 yanıtı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<RpcErrorBody>,
}

impl RpcResponse {
    pub fn success(id: u64, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn error(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcErrorBody { code, message: message.into(), data: None }),
        }
    }

    pub fn is_success(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }

    pub fn unwrap_result(&self) -> SdkResult<&Value> {
        self.result.as_ref().ok_or_else(|| {
            let msg = self.error.as_ref()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "bilinmeyen hata".into());
            SdkError::RpcError { reason: msg }
        })
    }
}

// ─── WsSubscription ───────────────────────────────────────────────────────────

/// WebSocket abonelik mesajı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSubscription {
    pub jsonrpc: String,
    pub method: String,
    pub params: WsParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsParams {
    pub subscription: String,
    pub result: Value,
}

impl WsSubscription {
    pub fn new_block(block_number: u64, block_hash: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: "nova_subscription".into(),
            params: WsParams {
                subscription: "newHeads".into(),
                result: serde_json::json!({
                    "number": block_number,
                    "hash": block_hash,
                }),
            },
        }
    }
}

// ─── Nova Shield RPC metodları ────────────────────────────────────────────────

/// Nova Shield RPC metod isimleri
pub struct NovaRpcMethod;

impl NovaRpcMethod {
    pub const CHAIN_GET_BLOCK: &'static str = "nova_getBlock";
    pub const CHAIN_GET_BLOCK_NUMBER: &'static str = "nova_blockNumber";
    pub const ACCOUNT_GET: &'static str = "nova_getAccount";
    pub const ACCOUNT_BALANCE: &'static str = "nova_getBalance";
    pub const TX_SEND: &'static str = "nova_sendTransaction";
    pub const TX_GET: &'static str = "nova_getTransaction";
    pub const GUARDIAN_SEARCH: &'static str = "nova_searchGuardians";
    pub const INHERITANCE_CREATE: &'static str = "nova_createInheritancePlan";
    pub const HEARTBEAT_SEND: &'static str = "nova_sendHeartbeat";
    pub const BATCH_SUBMIT: &'static str = "nova_submitBatchOrder";
    pub const ZK_SUBMIT: &'static str = "nova_submitZkTx";
}

// ─── MockRpcClient ────────────────────────────────────────────────────────────

/// Mock JSON-RPC istemcisi (test için)
pub struct MockRpcClient {
    endpoint: String,
    next_id: u64,
    block_number: u64,
}

impl MockRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self { endpoint: endpoint.into(), next_id: 1, block_number: 1 }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// RPC isteği oluştur
    pub fn build_request(&mut self, method: &str, params: Vec<Value>) -> RpcRequest {
        let id = self.next_id;
        self.next_id += 1;
        RpcRequest::new(method, params, id)
    }

    /// Mock RPC çağrısı — gerçek ağ yok, deterministik yanıt
    pub fn call(&mut self, method: &str, params: Vec<Value>) -> SdkResult<RpcResponse> {
        let req = self.build_request(method, params);
        let result = match method {
            "nova_blockNumber" => {
                self.block_number += 1;
                serde_json::json!(self.block_number)
            }
            "nova_getBalance" => {
                serde_json::json!("0x38D7EA4C68000") // 1000 NSC mock
            }
            "nova_getAccount" => {
                serde_json::json!({
                    "address": "0x1234...",
                    "nonce": 5,
                    "balance": "1000000000000000000"
                })
            }
            "nova_sendTransaction" => {
                serde_json::json!({
                    "txHash": "0xabc123..."
                })
            }
            _ => serde_json::json!(null),
        };
        Ok(RpcResponse::success(req.id, result))
    }

    pub fn current_block(&self) -> u64 {
        self.block_number
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_serialize() {
        let req = RpcRequest::new("nova_blockNumber", vec![], 1);
        let json = req.serialize().unwrap();
        assert!(json.contains("nova_blockNumber"));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_rpc_response_success() {
        let resp = RpcResponse::success(1, serde_json::json!(42));
        assert!(resp.is_success());
        assert_eq!(*resp.unwrap_result().unwrap(), serde_json::json!(42));
    }

    #[test]
    fn test_rpc_response_error() {
        let resp = RpcResponse::error(1, -32600, "Invalid Request");
        assert!(!resp.is_success());
        assert!(matches!(resp.unwrap_result(), Err(SdkError::RpcError { .. })));
    }

    #[test]
    fn test_ws_subscription_new_block() {
        let ws = WsSubscription::new_block(100, "0xabc...");
        assert_eq!(ws.method, "nova_subscription");
        assert_eq!(ws.params.subscription, "newHeads");
    }

    #[test]
    fn test_mock_rpc_block_number() {
        let mut client = MockRpcClient::new("http://localhost:9944");
        let resp = client.call("nova_blockNumber", vec![]).unwrap();
        assert!(resp.is_success());
        assert!(resp.unwrap_result().unwrap().as_u64().unwrap() > 0);
    }

    #[test]
    fn test_mock_rpc_balance() {
        let mut client = MockRpcClient::new("http://localhost:9944");
        let resp = client.call("nova_getBalance", vec![serde_json::json!("0x123")]).unwrap();
        assert!(resp.is_success());
    }

    #[test]
    fn test_mock_rpc_send_tx() {
        let mut client = MockRpcClient::new("http://localhost:9944");
        let resp = client.call("nova_sendTransaction", vec![serde_json::json!({})]).unwrap();
        assert!(resp.is_success());
    }

    #[test]
    fn test_mock_rpc_request_ids() {
        let mut client = MockRpcClient::new("http://localhost:9944");
        let r1 = client.build_request("m1", vec![]);
        let r2 = client.build_request("m2", vec![]);
        assert_ne!(r1.id, r2.id);
    }

    #[test]
    fn test_nova_rpc_method_names() {
        assert_eq!(NovaRpcMethod::CHAIN_GET_BLOCK, "nova_getBlock");
        assert_eq!(NovaRpcMethod::TX_SEND, "nova_sendTransaction");
        assert_eq!(NovaRpcMethod::HEARTBEAT_SEND, "nova_sendHeartbeat");
    }

    #[test]
    fn test_rpc_response_error_code() {
        let resp = RpcResponse::error(42, -32602, "Invalid params");
        assert_eq!(resp.error.as_ref().unwrap().code, -32602);
        assert_eq!(resp.id, 42);
    }
}
