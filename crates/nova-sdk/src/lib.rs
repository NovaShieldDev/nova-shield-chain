//! nova-sdk — Nova Shield Chain Geliştirici SDK (§9.2)
//!
//! Builder pattern, JSON-RPC istemci, deadman switch, guardian marketplace

pub mod account;
pub mod batch;
pub mod deadman;
pub mod errors;
pub mod guardian_client;
pub mod rpc;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use account::{AccountManager, GuardianRole, GuardianSpec, NovaAccount, NovaAccountBuilder};
pub use batch::{BatchClient, BatchOrder, OrderBuilder, OrderKind, OrderSide};
pub use deadman::{
    DeadmanPlan, DeadmanPlanBuilder, HeartbeatMsg, HeartbeatSender, ReleaseStep, TriggerConfig,
};
pub use errors::{SdkError, SdkResult};
pub use guardian_client::{
    GuardianCategory, GuardianContract, GuardianFilter, GuardianListing, GuardianMarketplace,
};
pub use rpc::{
    MockRpcClient, NovaRpcClient, NovaRpcMethod, RpcErrorBody, RpcRequest, RpcResponse,
    WsSubscription, DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT_MS,
};

// ─── SDK Versiyonu ────────────────────────────────────────────────────────────

pub const SDK_VERSION: &str = "0.1.0";
pub const NOVA_CHAIN_ID: u64 = 42_001;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn user_addr() -> [u8; 20] {
        [0xAAu8; 20]
    }

    fn guardian_addr(n: u8) -> [u8; 20] {
        [n; 20]
    }

    fn seed(n: u8) -> [u8; 32] {
        [n; 32]
    }

    // ── Account ──────────────────────────────────────────────────────────────

    #[test]
    fn test_sdk_account_builder_full() {
        let account = NovaAccountBuilder::new()
            .with_seed(seed(0x11))
            .with_guardian(guardian_addr(1), "Guardian-A")
            .with_institutional_guardian(guardian_addr(2), "Guardian-B")
            .with_threshold(2)
            .build()
            .unwrap();

        assert!(account.address != [0u8; 20]);
        assert_eq!(account.guardians.len(), 2);
        assert_eq!(account.threshold, 2);
    }

    #[test]
    fn test_sdk_account_manager_lifecycle() {
        let mut mgr = AccountManager::new();
        let account = NovaAccountBuilder::new()
            .with_seed(seed(0x22))
            .with_guardian(guardian_addr(1), "G1")
            .build()
            .unwrap();

        let addr = account.address;
        mgr.register(account);
        mgr.credit(&addr, 1_000_000);

        let acc = mgr.get(&addr).unwrap();
        assert_eq!(acc.balance, 1_000_000);
    }

    #[test]
    fn test_sdk_account_debit_insufficient() {
        let mut mgr = AccountManager::new();
        let account = NovaAccountBuilder::new()
            .with_seed(seed(0x33))
            .with_guardian(guardian_addr(1), "G1")
            .build()
            .unwrap();
        let addr = account.address;
        mgr.register(account);

        let result = mgr.debit(&addr, 999_999);
        assert!(result.is_err());
    }

    // ── Batch ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sdk_batch_order_full_flow() {
        let mut client = BatchClient::new(1000);

        let order = OrderBuilder::new()
            .sender(user_addr())
            .sell("NOVA", 5_000_000)
            .buy("USDC", 4_500_000)
            .limit_price(4_800_000)
            .valid_for(200)
            .build(1000)
            .unwrap();

        let id = client.submit_order(order).unwrap();
        assert_ne!(id, [0u8; 32]);
        assert_eq!(client.submitted_count(), 1);
    }

    #[test]
    fn test_sdk_batch_queue_and_flush() {
        let mut client = BatchClient::new(500);
        for i in 0u64..3 {
            let order = OrderBuilder::new()
                .sender(user_addr())
                .sell("NOVA", 1_000 + i)
                .buy("USDC", 900)
                .build(500)
                .unwrap();
            client.queue_order(order);
        }
        let ids = client.flush_pending().unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(client.pending_count(), 0);
    }

    // ── Deadman ───────────────────────────────────────────────────────────────

    #[test]
    fn test_sdk_deadman_full_flow() {
        let heir1 = [0x10u8; 20];
        let heir2 = [0x20u8; 20];

        let plan = DeadmanPlanBuilder::new(user_addr())
            .add_heir(heir1, 7_000, 100)
            .add_heir(heir2, 3_000, 200)
            .build(5_000)
            .unwrap();

        assert!(plan.active);
        assert_eq!(plan.heir_count(), 2);
        plan.validate_fractions().unwrap();
    }

    #[test]
    fn test_sdk_heartbeat_full_flow() {
        let key = [0x55u8; 32];
        let mut sender = HeartbeatSender::new(user_addr(), key);

        let msg = sender.send_heartbeat(1, 1000);
        assert!(!msg.is_duress);
        assert_eq!(msg.slot, 1000);

        let verifier = HeartbeatSender::new(user_addr(), key);
        assert!(verifier.verify_heartbeat(&msg));
    }

    #[test]
    fn test_sdk_duress_heartbeat() {
        let mut sender = HeartbeatSender::new(user_addr(), [0x99u8; 32]);
        let msg = sender.send_duress_heartbeat(2, 2000);
        assert!(msg.is_duress);

        let verifier = HeartbeatSender::new(user_addr(), [0x99u8; 32]);
        assert!(verifier.verify_heartbeat(&msg));
    }

    // ── Guardian Marketplace ─────────────────────────────────────────────────

    #[test]
    fn test_sdk_guardian_marketplace_flow() {
        let mut mp = GuardianMarketplace::new();

        mp.register_guardian(GuardianListing {
            address: guardian_addr(1),
            name: "Pro-Guardian".into(),
            category: GuardianCategory::Institutional,
            annual_fee: 500,
            kyc_verified: true,
            reputation_score: 90,
            successful_recoveries: 10,
            response_time_slots: 50,
            active: true,
        });

        let filter = GuardianFilter {
            category: Some(GuardianCategory::Institutional),
            min_reputation: Some(80),
            ..Default::default()
        };

        let top = mp.top_guardians(3, &filter);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name, "Pro-Guardian");
    }

    #[test]
    fn test_sdk_guardian_contract_create() {
        let mut mp = GuardianMarketplace::new();
        mp.register_guardian(GuardianListing {
            address: guardian_addr(5),
            name: "Safe-Guard".into(),
            category: GuardianCategory::Hardware,
            annual_fee: 1_000,
            kyc_verified: true,
            reputation_score: 75,
            successful_recoveries: 3,
            response_time_slots: 100,
            active: true,
        });

        let contract_id = mp.create_contract(user_addr(), guardian_addr(5), 1000).unwrap();
        let contract = mp.get_contract(contract_id).unwrap();
        assert_eq!(contract.guardian, guardian_addr(5));
        assert!(contract.active);
    }

    // ── RPC ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_sdk_rpc_mock_client_flow() {
        let mut client = MockRpcClient::new("http://localhost:9944");

        let resp = client.call(NovaRpcMethod::CHAIN_GET_BLOCK_NUMBER, vec![]).unwrap();
        assert!(resp.is_success());

        let balance_resp = client
            .call(NovaRpcMethod::ACCOUNT_BALANCE, vec![serde_json::json!("0xAA")])
            .unwrap();
        assert!(balance_resp.is_success());
    }

    #[test]
    fn test_sdk_rpc_request_ids_unique() {
        let mut client = MockRpcClient::new("http://localhost:9944");
        let r1 = client.build_request("m1", vec![]);
        let r2 = client.build_request("m2", vec![]);
        assert_ne!(r1.id, r2.id);
    }

    #[test]
    fn test_sdk_version() {
        assert_eq!(SDK_VERSION, "0.1.0");
        assert_eq!(NOVA_CHAIN_ID, 42_001);
    }
}
