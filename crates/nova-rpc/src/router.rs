//! JSON-RPC method router — gelen isteği doğru handler'a yönlendir

use crate::endpoints::{
    AccountHandler, BatchHandler, ChainHandler, DexHandler, GuardianHandler, TxHandler, ZkHandler,
};
use crate::errors::RpcError;
use crate::state::ChainState;
use crate::types::{RequestId, RpcRequest, RpcResponse};

/// JSON-RPC router — metod adına göre handler seçer
pub struct RpcRouter {
    chain: ChainHandler,
    account: AccountHandler,
    tx: TxHandler,
    guardian: GuardianHandler,
    batch: BatchHandler,
    zk: ZkHandler,
    dex: DexHandler,
}

impl RpcRouter {
    pub fn new(state: ChainState) -> Self {
        Self {
            chain: ChainHandler::new(state.clone()),
            account: AccountHandler::new(state.clone()),
            tx: TxHandler::new(state.clone()),
            guardian: GuardianHandler::new(state.clone()),
            batch: BatchHandler::new(state.clone()),
            zk: ZkHandler::new(state),
            dex: DexHandler::new(),
        }
    }

    /// İsteği işle ve yanıt döndür
    pub fn handle(&self, req: RpcRequest) -> RpcResponse {
        if let Err(e) = req.validate() {
            return RpcResponse::error(req.id, &e);
        }

        let id = req.id.clone();
        let params = &req.params;

        let result = match req.method.as_str() {
            // ── Chain ─────────────────────────────────────────────────────────
            "nova_blockNumber" => self.chain.block_number(),
            "nova_getBlock" => self.chain.get_block(params),
            "nova_chainId" => self.chain.chain_id(),
            "nova_syncing" => self.chain.syncing(),
            "nova_getSlot" => self.chain.get_slot(),

            // ── Account ───────────────────────────────────────────────────────
            "nova_getAccount" => self.account.get_account(params),
            "nova_getBalance" => self.account.get_balance(params),
            "nova_getNonce" => self.account.get_nonce(params),
            "nova_estimateGas" => self.account.estimate_gas(params),
            "nova_getGuardians" => self.account.get_guardians(params),

            // ── Transaction ───────────────────────────────────────────────────
            "nova_sendTransaction" => self.tx.send_transaction(params),
            "nova_getTransaction" => self.tx.get_transaction(params),
            "nova_getTransactionReceipt" => self.tx.get_transaction_receipt(params),
            "nova_pendingTransactions" => self.tx.pending_transactions(),

            // ── Guardian ──────────────────────────────────────────────────────
            "nova_searchGuardians" => self.guardian.search_guardians(params),
            "nova_getGuardian" => self.guardian.get_guardian(params),
            "nova_createInheritancePlan" => self.guardian.create_inheritance_plan(params),
            "nova_sendHeartbeat" => self.guardian.send_heartbeat(params),

            // ── Batch ─────────────────────────────────────────────────────────
            "nova_submitBatchOrder" => self.batch.submit_batch_order(params),
            "nova_getBatchStatus" => self.batch.get_batch_status(params),
            "nova_cancelOrder" => self.batch.cancel_order(params),

            // ── ZK ────────────────────────────────────────────────────────────
            "nova_submitZkTx" => self.zk.submit_zk_tx(params),
            "nova_generateProof" => self.zk.generate_proof(params),
            "nova_verifyProof" => self.zk.verify_proof(params),

            // ── DEX proxy ─────────────────────────────────────────────────────
            "nova_dex_health"          => self.dex.health(params),
            "nova_dex_createPool"      => self.dex.create_pool(params),
            "nova_dex_getPools"        => self.dex.get_pools(params),
            "nova_dex_getPool"         => self.dex.get_pool(params),
            "nova_dex_swap"            => self.dex.swap(params),
            "nova_dex_addLiquidity"    => self.dex.add_liquidity(params),
            "nova_dex_removeLiquidity" => self.dex.remove_liquidity(params),
            "nova_dex_extendLock"      => self.dex.extend_lock(params),
            "nova_dex_getPosition"     => self.dex.get_position(params),
            "nova_dex_processPayouts"  => self.dex.process_payouts(params),

            _ => Err(RpcError::MethodNotFound { method: req.method.clone() }),
        };

        match result {
            Ok(value) => RpcResponse::success(id, value),
            Err(e) => RpcResponse::error(id, &e),
        }
    }

    /// Ham JSON string işle
    pub fn handle_raw(&self, json: &str) -> String {
        let req = match serde_json::from_str::<RpcRequest>(json) {
            Ok(r) => r,
            Err(_) => {
                let resp = RpcResponse::error(
                    RequestId::Null,
                    &RpcError::ParseError,
                );
                return resp.to_json();
            }
        };
        self.handle(req).to_json()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> RpcRouter {
        RpcRouter::new(ChainState::new())
    }

    fn req(method: &str, params: serde_json::Value) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: params.as_array().cloned().unwrap_or_default(),
            id: RequestId::Number(1),
        }
    }

    #[test]
    fn test_router_block_number() {
        let r = router();
        let resp = r.handle(req("nova_blockNumber", serde_json::json!([])));
        assert!(resp.is_success());
    }

    #[test]
    fn test_router_chain_id() {
        let r = router();
        let resp = r.handle(req("nova_chainId", serde_json::json!([])));
        assert!(resp.is_success());
        assert_eq!(resp.result.unwrap(), serde_json::json!("0x7c0"));
    }

    #[test]
    fn test_router_method_not_found() {
        let r = router();
        let resp = r.handle(req("nova_unknown_method", serde_json::json!([])));
        assert!(!resp.is_success());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_router_invalid_request_version() {
        let r = router();
        let mut request = req("nova_blockNumber", serde_json::json!([]));
        request.jsonrpc = "1.0".into();
        let resp = r.handle(request);
        assert!(!resp.is_success());
    }

    #[test]
    fn test_router_raw_valid() {
        let r = router();
        let raw = r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}"#;
        let resp_json = r.handle_raw(raw);
        assert!(resp_json.contains("result"));
    }

    #[test]
    fn test_router_raw_parse_error() {
        let r = router();
        let resp_json = r.handle_raw("not valid json {{");
        assert!(resp_json.contains("-32700"));
    }

    #[test]
    fn test_router_get_balance() {
        let r = router();
        let resp = r.handle(req(
            "nova_getBalance",
            serde_json::json!(["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]),
        ));
        assert!(resp.is_success());
    }

    /// NOVA-RED-RPC-004: İmzalı işlem kabul edilmeli
    #[test]
    fn test_router_send_transaction() {
        let r = router();
        // 65 byte geçerli format imza
        let sig = "0x".to_string() + &"aa".repeat(65);
        let resp = r.handle(req(
            "nova_sendTransaction",
            serde_json::json!([{
                "from": "0xaaaa",
                "to": "0xbbbb",
                "value": "0x1000",
                "signature": sig,
            }]),
        ));
        assert!(resp.is_success());
    }

    /// NOVA-RED-RPC-004: İmzasız işlem reddedilmeli
    #[test]
    fn test_router_send_transaction_no_sig_rejected() {
        let r = router();
        let resp = r.handle(req(
            "nova_sendTransaction",
            serde_json::json!([{
                "from": "0xaaaa",
                "to": "0xbbbb",
                "value": "0x1000",
            }]),
        ));
        assert!(!resp.is_success(), "imzasız işlem reddedilmeli");
    }

    #[test]
    fn test_router_search_guardians() {
        let r = router();
        let resp = r.handle(req("nova_searchGuardians", serde_json::json!([])));
        assert!(resp.is_success());
    }

    /// NOVA-RED-RPC-005: Mock kanıt artık reddedilmeli
    #[test]
    fn test_router_submit_zk_tx_mock_rejected() {
        let r = router();
        let resp = r.handle(req(
            "nova_submitZkTx",
            serde_json::json!([{
                "type": "standard",
                "proof": "0xabcdef1234567890", // 8 byte — mock kanıt
            }]),
        ));
        // NOVA-RED-RPC-005: Mock kanıt geçersiz boyut → hata
        assert!(!resp.is_success(), "mock kanıt reddedilmeli");
    }

    #[test]
    fn test_router_submit_batch_order() {
        let r = router();
        // NOVA-RED-AUCTION-003: signature alanı zorunlu
        let mock_sig = format!("0x{}", "aa".repeat(65));
        let resp = r.handle(req(
            "nova_submitBatchOrder",
            serde_json::json!([{
                "sender": "0xaaaa",
                "signature": mock_sig,
                "sellToken": "NOVA",
                "buyToken": "USDC",
                "sellAmount": "0x1000",
            }]),
        ));
        assert!(resp.is_success());
    }

    #[test]
    fn test_router_all_nova_methods_available() {
        let r = router();
        let methods = [
            "nova_blockNumber", "nova_chainId", "nova_syncing", "nova_getSlot",
            "nova_pendingTransactions",
        ];
        for method in methods {
            let resp = r.handle(req(method, serde_json::json!([])));
            assert!(resp.is_success(), "failed: {}", method);
        }
    }
}
