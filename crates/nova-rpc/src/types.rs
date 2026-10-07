//! JSON-RPC 2.0 temel türleri — istek, yanıt, abonelik

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::RpcError;

// ─── RpcRequest ───────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 isteği
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Vec<Value>,
    pub id: RequestId,
}

impl RpcRequest {
    pub fn new(method: impl Into<String>, params: Vec<Value>, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: RequestId::Number(id),
        }
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        if self.jsonrpc != "2.0" {
            return Err(RpcError::InvalidRequest);
        }
        if self.method.is_empty() {
            return Err(RpcError::InvalidRequest);
        }
        Ok(())
    }
}

/// İstek ID — number veya string
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    Text(String),
    Null,
}

impl RequestId {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            RequestId::Number(n) => Some(*n),
            _ => None,
        }
    }
}

// ─── RpcResponse ──────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 yanıtı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcErrorObject>,
}

impl RpcResponse {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn error(id: RequestId, err: &RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcErrorObject {
                code: err.code(),
                message: err.to_string(),
                data: None,
            }),
        }
    }

    pub fn is_success(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// JSON-RPC hata nesnesi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ─── WsNotification ───────────────────────────────────────────────────────────

/// WebSocket bildirim mesajı (abonelik)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: WsNotificationParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsNotificationParams {
    pub subscription: String,
    pub result: Value,
}

impl WsNotification {
    pub fn new_heads(subscription_id: &str, block_number: u64, block_hash: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: "nova_subscription".into(),
            params: WsNotificationParams {
                subscription: subscription_id.into(),
                result: serde_json::json!({
                    "number": format!("0x{:x}", block_number),
                    "hash": block_hash,
                    "parentHash": "0x0000000000000000",
                    "timestamp": block_number * 400,
                }),
            },
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_validate_ok() {
        let req = RpcRequest::new("nova_blockNumber", vec![], 1);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_request_validate_wrong_version() {
        let mut req = RpcRequest::new("m", vec![], 1);
        req.jsonrpc = "1.0".into();
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_request_validate_empty_method() {
        let mut req = RpcRequest::new("", vec![], 1);
        req.method = "".into();
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_response_success() {
        let resp = RpcResponse::success(RequestId::Number(1), serde_json::json!(42));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap(), serde_json::json!(42));
    }

    #[test]
    fn test_response_error() {
        let err = RpcError::MethodNotFound { method: "unknown".into() };
        let resp = RpcResponse::error(RequestId::Number(1), &err);
        assert!(!resp.is_success());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_response_json_serialize() {
        let resp = RpcResponse::success(RequestId::Number(5), serde_json::json!("hello"));
        let json = resp.to_json();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_ws_notification_new_heads() {
        let notif = WsNotification::new_heads("sub-123", 1000, "0xabc...");
        assert_eq!(notif.method, "nova_subscription");
        assert_eq!(notif.params.subscription, "sub-123");
    }

    #[test]
    fn test_request_id_types() {
        let num = RequestId::Number(42);
        assert_eq!(num.as_u64(), Some(42));

        let txt = RequestId::Text("abc".into());
        assert_eq!(txt.as_u64(), None);
    }
}
