//! account_* RPC metodları

use crate::errors::{RpcError, RpcResult};
use crate::state::ChainState;
use serde_json::Value;

pub struct AccountHandler {
    state: ChainState,
}

impl AccountHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_getAccount — hesap bilgisi
    pub fn get_account(&self, params: &[Value]) -> RpcResult<Value> {
        let address = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "adres parametresi gerekli".into(),
            })?;

        let acc = self.state.get_or_default_account(address);
        Ok(serde_json::json!({
            "address": acc.address,
            "nonce": format!("0x{:x}", acc.nonce),
            "balance": format!("0x{:x}", acc.balance),
            "guardianCount": acc.guardian_count,
            "exists": acc.balance > 0 || acc.nonce > 0,
        }))
    }

    /// nova_getBalance — hesap bakiyesi
    pub fn get_balance(&self, params: &[Value]) -> RpcResult<Value> {
        let address = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "adres parametresi gerekli".into(),
            })?;

        let acc = self.state.get_or_default_account(address);
        Ok(serde_json::json!(format!("0x{:x}", acc.balance)))
    }

    /// nova_getNonce — hesap nonce
    pub fn get_nonce(&self, params: &[Value]) -> RpcResult<Value> {
        let address = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "adres parametresi gerekli".into(),
            })?;

        let acc = self.state.get_or_default_account(address);
        Ok(serde_json::json!(format!("0x{:x}", acc.nonce)))
    }

    /// nova_estimateGas — tahmini gas ücreti
    pub fn estimate_gas(&self, params: &[Value]) -> RpcResult<Value> {
        let tx_obj = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "işlem nesnesi gerekli".into(),
        })?;

        // Mock: işlem tipine göre gas tahmini
        let gas = if tx_obj.get("data").is_some() {
            80_000u64 // sözleşme çağrısı
        } else {
            21_000u64 // basit transfer
        };

        Ok(serde_json::json!(format!("0x{:x}", gas)))
    }

    /// nova_getGuardians — hesabın guardian listesi
    pub fn get_guardians(&self, params: &[Value]) -> RpcResult<Value> {
        let address = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "adres parametresi gerekli".into(),
            })?;

        let acc = self.state.get_or_default_account(address);
        // Mock: guardian adresleri oluştur
        let guardians: Vec<Value> = (0..acc.guardian_count)
            .map(|i| serde_json::json!({
                "address": format!("0x{:0>40}", i + 1),
                "role": if i == 0 { "institutional" } else { "personal" },
                "active": true,
            }))
            .collect();

        Ok(serde_json::json!({
            "account": address,
            "guardians": guardians,
            "threshold": (acc.guardian_count + 1) / 2,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> AccountHandler {
        AccountHandler::new(ChainState::new())
    }

    const ALICE: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const UNKNOWN: &str = "0x0000000000000000000000000000000000000099";

    #[test]
    fn test_get_account_known() {
        let h = handler();
        let result = h.get_account(&[serde_json::json!(ALICE)]).unwrap();
        assert_eq!(result["address"], serde_json::json!(ALICE));
        assert_eq!(result["exists"], serde_json::json!(true));
    }

    #[test]
    fn test_get_account_unknown() {
        let h = handler();
        let result = h.get_account(&[serde_json::json!(UNKNOWN)]).unwrap();
        assert_eq!(result["exists"], serde_json::json!(false));
    }

    #[test]
    fn test_get_balance_known() {
        let h = handler();
        let result = h.get_balance(&[serde_json::json!(ALICE)]).unwrap();
        assert!(result.as_str().unwrap().starts_with("0x"));
        assert_ne!(result, serde_json::json!("0x0"));
    }

    #[test]
    fn test_get_balance_unknown() {
        let h = handler();
        let result = h.get_balance(&[serde_json::json!(UNKNOWN)]).unwrap();
        assert_eq!(result, serde_json::json!("0x0"));
    }

    #[test]
    fn test_get_nonce() {
        let h = handler();
        let result = h.get_nonce(&[serde_json::json!(ALICE)]).unwrap();
        assert_eq!(result, serde_json::json!("0x5"));
    }

    #[test]
    fn test_estimate_gas_transfer() {
        let h = handler();
        let result = h.estimate_gas(&[serde_json::json!({
            "from": ALICE,
            "to": UNKNOWN,
            "value": "0x1000",
        })]).unwrap();
        assert_eq!(result, serde_json::json!("0x5208")); // 21000
    }

    #[test]
    fn test_estimate_gas_contract_call() {
        let h = handler();
        let result = h.estimate_gas(&[serde_json::json!({
            "from": ALICE,
            "to": "0xcontract",
            "data": "0x12345678",
        })]).unwrap();
        assert_eq!(result, serde_json::json!("0x13880")); // 80000
    }

    #[test]
    fn test_get_account_no_params() {
        let h = handler();
        assert!(h.get_account(&[]).is_err());
    }

    #[test]
    fn test_get_guardians() {
        let h = handler();
        let result = h.get_guardians(&[serde_json::json!(ALICE)]).unwrap();
        let guardians = result["guardians"].as_array().unwrap();
        assert_eq!(guardians.len(), 3); // guardian_count = 3
    }
}
