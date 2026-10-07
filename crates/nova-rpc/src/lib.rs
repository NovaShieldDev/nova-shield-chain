//! nova-rpc — Nova Shield Chain JSON-RPC Sunucu (§9)
//!
//! JSON-RPC 2.0 endpoint'leri:
//! - chain: nova_blockNumber, nova_getBlock, nova_chainId, nova_syncing, nova_getSlot
//! - account: nova_getAccount, nova_getBalance, nova_getNonce, nova_estimateGas
//! - tx: nova_sendTransaction, nova_getTransaction, nova_getTransactionReceipt
//! - guardian: nova_searchGuardians, nova_createInheritancePlan, nova_sendHeartbeat
//! - batch: nova_submitBatchOrder, nova_getBatchStatus, nova_cancelOrder
//! - zk: nova_submitZkTx, nova_generateProof, nova_verifyProof

pub mod endpoints;
pub mod errors;
pub mod rate_limit;
pub mod router;
pub mod server;
pub mod state;
pub mod types;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use errors::{RpcError, RpcResult};
pub use router::RpcRouter;
pub use server::HttpRpcServer;
pub use state::{ChainState, RuntimeBackend};
pub use types::{RequestId, RpcRequest, RpcResponse, RpcErrorObject, WsNotification};

pub const RPC_VERSION: &str = "0.1.0";

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> RpcRouter {
        RpcRouter::new(ChainState::new())
    }

    fn call(router: &RpcRouter, method: &str, params: serde_json::Value) -> RpcResponse {
        let req = RpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: params.as_array().cloned().unwrap_or_default(),
            id: RequestId::Number(1),
        };
        router.handle(req)
    }

    // ── Full integration flows ────────────────────────────────────────────────

    #[test]
    fn test_rpc_chain_flow() {
        let r = router();

        // Block number
        let resp = call(&r, "nova_blockNumber", serde_json::json!([]));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap(), serde_json::json!("0x0"));

        // Chain ID
        let resp = call(&r, "nova_chainId", serde_json::json!([]));
        assert_eq!(resp.result.unwrap(), serde_json::json!("0x7c0"));

        // Get genesis block
        let resp = call(&r, "nova_getBlock", serde_json::json!(["earliest"]));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap()["number"], serde_json::json!("0x0"));
    }

    #[test]
    fn test_rpc_account_flow() {
        let r = router();
        let alice = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

        let resp = call(&r, "nova_getAccount", serde_json::json!([alice]));
        assert!(resp.result.as_ref().unwrap()["exists"].as_bool().unwrap());

        let resp = call(&r, "nova_getBalance", serde_json::json!([alice]));
        let balance = resp.result.unwrap();
        assert!(balance.as_str().unwrap() != "0x0");

        let resp = call(&r, "nova_getNonce", serde_json::json!([alice]));
        assert_eq!(resp.result.unwrap(), serde_json::json!("0x5"));
    }

    #[test]
    fn test_rpc_tx_flow() {
        let r = router();

        // TX gönder
        let resp = call(&r, "nova_sendTransaction", serde_json::json!([{
            "from": "0xaaaa",
            "to": "0xbbbb",
            "value": "0x1000",
            "gasPrice": "0x3b9aca00",
        }]));
        assert!(resp.is_success());
        let hash = resp.result.unwrap().as_str().unwrap().to_string();
        assert!(hash.starts_with("0x"));

        // TX sorgula
        let resp = call(&r, "nova_getTransaction", serde_json::json!([hash]));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap()["status"], serde_json::json!("pending"));

        // Pending listesi
        let resp = call(&r, "nova_pendingTransactions", serde_json::json!([]));
        assert!(resp.result.unwrap().as_array().unwrap().len() >= 1);
    }

    #[test]
    fn test_rpc_guardian_flow() {
        let r = router();

        // Guardian ara
        let resp = call(&r, "nova_searchGuardians", serde_json::json!([{
            "kycOnly": true,
        }]));
        assert!(resp.is_success());
        let count = resp.result.unwrap()["count"].as_u64().unwrap();
        assert_eq!(count, 2);

        // Miras planı oluştur
        let resp = call(&r, "nova_createInheritancePlan", serde_json::json!([{
            "owner": "0xaaaa",
            "inactivityPeriodSecs": 7776000,
        }]));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap()["status"], serde_json::json!("created"));

        // Heartbeat gönder
        let resp = call(&r, "nova_sendHeartbeat", serde_json::json!([{
            "account": "0xaaaa",
            "planId": 1,
            "signature": "0xsig",
        }]));
        assert!(resp.result.unwrap()["accepted"].as_bool().unwrap());
    }

    #[test]
    fn test_rpc_batch_flow() {
        let r = router();

        // Batch emir gönder
        let resp = call(&r, "nova_submitBatchOrder", serde_json::json!([{
            "sender": "0xaaaa",
            "sellToken": "NOVA",
            "buyToken": "USDC",
            "sellAmount": "0xde0b6b3a7640000",
        }]));
        assert!(resp.is_success());

        // Batch durumu sorgula
        let resp = call(&r, "nova_getBatchStatus", serde_json::json!([0]));
        assert!(resp.is_success());
    }

    #[test]
    fn test_rpc_zk_flow() {
        let r = router();

        // ZK kanıt üret (mint devresi — gerçek Groth16)
        let resp = call(&r, "nova_generateProof", serde_json::json!([{
            "circuit": "mint",
            "value": 1_000_000u64,
            "randomness": "0x0101010101010101010101010101010101010101010101010101010101010101",
            "transmissionKey": "0x0303030303030303030303030303030303030303030303030303030303030303",
        }]));
        assert!(resp.is_success());
        let resp_val = resp.result.unwrap();
        let proof = resp_val["proof"].as_str().unwrap().to_string();
        let pub_inputs = resp_val["publicInputs"].clone();
        assert!(proof.starts_with("0x"));
        assert_eq!(resp_val["realGroth16"], serde_json::json!(true));

        // ZK kanıt doğrula (aynı mint kanıtı)
        let resp = call(&r, "nova_verifyProof", serde_json::json!([{
            "proof": proof,
            "publicInputs": pub_inputs,
            "circuit": "mint",
        }]));
        assert_eq!(resp.result.unwrap()["valid"], serde_json::json!(true));

        // ZK TX gönder
        let resp = call(&r, "nova_submitZkTx", serde_json::json!([{
            "type": "full_shield",
            "proof": "0xabcdef1234567890",
        }]));
        assert!(resp.result.unwrap()["proofVerified"].as_bool().unwrap());
    }

    #[test]
    fn test_rpc_error_handling() {
        let r = router();

        // Method not found
        let resp = call(&r, "eth_blockNumber", serde_json::json!([]));
        assert!(!resp.is_success());
        assert_eq!(resp.error.unwrap().code, errors::METHOD_NOT_FOUND);

        // Account not found için get_transaction
        let resp = call(&r, "nova_getTransaction", serde_json::json!(["0xdeadbeef"]));
        assert!(!resp.is_success());
    }

    #[test]
    fn test_rpc_raw_json() {
        let r = router();
        let json = r#"{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":42}"#;
        let resp_str = r.handle_raw(json);
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        assert_eq!(resp["result"], serde_json::json!("0x7c0"));
        assert_eq!(resp["id"], serde_json::json!(42));
    }

    #[test]
    fn test_rpc_version() {
        assert_eq!(RPC_VERSION, "0.1.0");
    }
}
