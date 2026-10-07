//! guardian_* RPC metodları

use crate::errors::{RpcError, RpcResult};
use crate::state::ChainState;
use serde_json::Value;

pub struct GuardianHandler {
    state: ChainState,
}

impl GuardianHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_searchGuardians — guardian marketplace arama
    pub fn search_guardians(&self, params: &[Value]) -> RpcResult<Value> {
        let filter = params.first().cloned().unwrap_or(serde_json::json!({}));

        let max_fee = filter.get("maxAnnualFee")
            .and_then(|v| v.as_u64())
            .unwrap_or(u64::MAX);

        let min_rep = filter.get("minReputation")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;

        let kyc_only = filter.get("kycOnly")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Mock guardian listesi
        let guardians = self.mock_guardians();
        let filtered: Vec<&Value> = guardians.iter()
            .filter(|g| {
                let fee = g["annualFee"].as_u64().unwrap_or(u64::MAX);
                let rep = g["reputationScore"].as_u64().unwrap_or(0) as u8;
                let kyc = g["kycVerified"].as_bool().unwrap_or(false);
                fee <= max_fee && rep >= min_rep && (!kyc_only || kyc)
            })
            .collect();

        Ok(serde_json::json!({
            "count": filtered.len(),
            "guardians": filtered,
        }))
    }

    /// nova_getGuardian — guardian detayı
    pub fn get_guardian(&self, params: &[Value]) -> RpcResult<Value> {
        let address = params.first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "guardian adresi gerekli".into(),
            })?;

        let guardians = self.mock_guardians();
        let found = guardians.iter()
            .find(|g| g["address"].as_str() == Some(address));

        match found {
            Some(g) => Ok(g.clone()),
            None => Err(RpcError::GuardianNotFound),
        }
    }

    /// nova_createInheritancePlan — miras planı oluştur
    pub fn create_inheritance_plan(&self, params: &[Value]) -> RpcResult<Value> {
        let plan = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "plan nesnesi gerekli".into(),
        })?;

        let owner = plan.get("owner").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "owner alanı gerekli".into(),
            })?;

        // Mock plan ID üret
        let mut h = blake3::Hasher::new();
        h.update(b"nova-plan");
        h.update(owner.as_bytes());
        let plan_id = u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap());

        Ok(serde_json::json!({
            "planId": plan_id,
            "owner": owner,
            "status": "created",
            "txHash": format!("0x{}", hex::encode(h.finalize().as_bytes())),
        }))
    }

    /// nova_sendHeartbeat — heartbeat gönder
    pub fn send_heartbeat(&self, params: &[Value]) -> RpcResult<Value> {
        let hb = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "heartbeat nesnesi gerekli".into(),
        })?;

        let account = hb.get("account").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "account alanı gerekli".into(),
            })?;

        let plan_id = hb.get("planId").and_then(|v| v.as_u64())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "planId alanı gerekli".into(),
            })?;

        Ok(serde_json::json!({
            "account": account,
            "planId": plan_id,
            "accepted": true,
            "slot": self.state.best_block_number(),
        }))
    }

    fn mock_guardians(&self) -> Vec<Value> {
        vec![
            serde_json::json!({
                "address": "0x0000000000000000000000000000000000000001",
                "name": "Guardian-Alpha",
                "category": "Institutional",
                "annualFee": 500,
                "kycVerified": true,
                "reputationScore": 95,
                "successfulRecoveries": 12,
                "active": true,
            }),
            serde_json::json!({
                "address": "0x0000000000000000000000000000000000000002",
                "name": "Guardian-Beta",
                "category": "Personal",
                "annualFee": 200,
                "kycVerified": true,
                "reputationScore": 78,
                "successfulRecoveries": 5,
                "active": true,
            }),
            serde_json::json!({
                "address": "0x0000000000000000000000000000000000000003",
                "name": "Guardian-Gamma",
                "category": "Hardware",
                "annualFee": 1000,
                "kycVerified": false,
                "reputationScore": 65,
                "successfulRecoveries": 2,
                "active": true,
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> GuardianHandler {
        GuardianHandler::new(ChainState::new())
    }

    #[test]
    fn test_search_guardians_all() {
        let h = handler();
        let result = h.search_guardians(&[]).unwrap();
        assert_eq!(result["count"], serde_json::json!(3));
    }

    #[test]
    fn test_search_guardians_max_fee() {
        let h = handler();
        let result = h.search_guardians(&[serde_json::json!({
            "maxAnnualFee": 500,
        })]).unwrap();
        // 500 ve 200 fee'li guardian'lar geçer (1000 elenir)
        assert_eq!(result["count"], serde_json::json!(2));
    }

    #[test]
    fn test_search_guardians_min_rep() {
        let h = handler();
        let result = h.search_guardians(&[serde_json::json!({
            "minReputation": 80,
        })]).unwrap();
        // sadece rep=95 geçer
        assert_eq!(result["count"], serde_json::json!(1));
    }

    #[test]
    fn test_search_guardians_kyc_only() {
        let h = handler();
        let result = h.search_guardians(&[serde_json::json!({
            "kycOnly": true,
        })]).unwrap();
        // kyc=false olan Gamma elenir
        assert_eq!(result["count"], serde_json::json!(2));
    }

    #[test]
    fn test_get_guardian_found() {
        let h = handler();
        let result = h.get_guardian(&[
            serde_json::json!("0x0000000000000000000000000000000000000001")
        ]).unwrap();
        assert_eq!(result["name"], serde_json::json!("Guardian-Alpha"));
    }

    #[test]
    fn test_get_guardian_not_found() {
        let h = handler();
        let result = h.get_guardian(&[
            serde_json::json!("0xdeadbeefdeadbeefdeadbeefdeadbeef00000099")
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_inheritance_plan() {
        let h = handler();
        let result = h.create_inheritance_plan(&[serde_json::json!({
            "owner": "0xaaaa",
            "heirs": [{"address": "0xbbbb", "share": 10000}],
            "inactivityPeriodSecs": 7776000,
        })]).unwrap();
        assert_eq!(result["status"], serde_json::json!("created"));
        assert!(result["planId"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_send_heartbeat() {
        let h = handler();
        let result = h.send_heartbeat(&[serde_json::json!({
            "account": "0xaaaa",
            "planId": 42,
            "signature": "0xsig",
        })]).unwrap();
        assert_eq!(result["accepted"], serde_json::json!(true));
        assert_eq!(result["planId"], serde_json::json!(42));
    }

    #[test]
    fn test_send_heartbeat_no_params() {
        let h = handler();
        assert!(h.send_heartbeat(&[]).is_err());
    }
}
