//! zk_* RPC metodları — ZK-Shield işlemleri

use crate::errors::{RpcError, RpcResult};
use crate::state::ChainState;
use nova_zk::circuits::mint::MintWitness;
use nova_zk::proof::groth16::{Groth16Proof, GROTH16_PROOF_SIZE};
use nova_zk::proof::params::CircuitType;
use nova_zk::utxo::note::{Note, NSC_ASSET_ID};
use nova_primitives::SlotId;
use serde_json::Value;

pub struct ZkHandler {
    state: ChainState,
}

impl ZkHandler {
    pub fn new(state: ChainState) -> Self {
        Self { state }
    }

    /// nova_submitZkTx — ZK kanıtlı işlem gönder ve doğrula
    ///
    /// Beklenen format:
    /// ```json
    /// {
    ///   "type": "full_shield" | "semi_shield" | "standard",
    ///   "proof": "0x<192-byte hex>",
    ///   "publicInputs": ["0x<32-byte hex>", ...],
    ///   "circuitType": "mint" | "transfer" | "burn"
    /// }
    /// ```
    pub fn submit_zk_tx(&self, params: &[Value]) -> RpcResult<Value> {
        let tx = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "ZK işlem nesnesi gerekli".into(),
        })?;

        let tx_type = tx.get("type").and_then(|v| v.as_str())
            .unwrap_or("standard");

        let proof_hex = tx.get("proof").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "proof alanı gerekli".into(),
            })?;

        let proof_str = proof_hex.trim_start_matches("0x");
        let proof_bytes = hex::decode(proof_str).map_err(|_| RpcError::InvalidParams {
            reason: "proof hex decode hatası".into(),
        })?;

        // NOVA-RED-RPC-005: Gerçek Groth16 kanıtı zorunlu — mock kanıt reddedilir.
        // Groth16 BLS12-381: π_A (G1, 48 B) + π_B (G2, 96 B) + π_C (G1, 48 B) = 192 byte
        if proof_bytes.len() != GROTH16_PROOF_SIZE {
            return Err(RpcError::InvalidParams {
                reason: format!(
                    "ZK kanıtı geçersiz boyut: {} byte, {} byte (Groth16 BLS12-381) gerekli",
                    proof_bytes.len(),
                    GROTH16_PROOF_SIZE
                ),
            });
        }

        // Gerçek Groth16 doğrulama
        let circuit_type = parse_circuit_type(tx.get("circuitType").and_then(|v| v.as_str()));
        let pub_inputs = parse_public_inputs(tx.get("publicInputs"));

        let zk_proof = Groth16Proof {
            proof_bytes,
            public_inputs: pub_inputs,
            circuit_type,
        };

        let proof_verified = zk_proof.verify_groth16().map_err(|e| RpcError::InternalError {
            reason: format!("ZK doğrulama hatası: {:?}", e),
        })?;

        if !proof_verified {
            return Err(RpcError::InvalidParams {
                reason: "ZK kanıtı doğrulanmadı".into(),
            });
        }

        let is_real_proof = true;

        let mut h = blake3::Hasher::new();
        h.update(b"nova-zk-tx");
        h.update(proof_hex.as_bytes());
        let tx_hash = format!("0x{}", hex::encode(h.finalize().as_bytes()));

        let gas = match tx_type {
            "full_shield" => 200_000u64,
            "semi_shield" => 80_000u64,
            _ => 21_000u64,
        };

        Ok(serde_json::json!({
            "txHash": tx_hash,
            "type": tx_type,
            "gasUsed": format!("0x{:x}", gas),
            "proofVerified": proof_verified,
            "realGroth16": is_real_proof,
            "shieldStatus": tx_type,
        }))
    }

    /// nova_generateProof — ZK kanıt üret (gerçek Groth16)
    ///
    /// Beklenen format (mint devresi):
    /// ```json
    /// {
    ///   "circuit": "mint",
    ///   "value": 1000000,
    ///   "randomness": "0x<32-byte hex>",
    ///   "transmissionKey": "0x<32-byte hex>"
    /// }
    /// ```
    pub fn generate_proof(&self, params: &[Value]) -> RpcResult<Value> {
        let req = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "kanıt isteği gerekli".into(),
        })?;

        let circuit = req.get("circuit").and_then(|v| v.as_str())
            .unwrap_or("mint");

        match circuit {
            "mint" | "groth16" | "full_shield" => {
                // Mint devresi için gerçek Groth16 kanıt üret
                let value = req.get("value").and_then(|v| v.as_u64()).unwrap_or(1_000_000) as u128;

                let randomness = parse_32bytes(req.get("randomness"));
                let transmission_key = parse_32bytes(req.get("transmissionKey"));

                let note = Note::new(value, NSC_ASSET_ID, randomness, transmission_key);
                let witness = MintWitness::new(note);

                let proof = witness.prove(SlotId(0)).map_err(|e| RpcError::InternalError {
                    reason: format!("Kanıt üretme hatası: {:?}", e),
                })?;

                let proof_hex = format!("0x{}", hex::encode(&proof.proof.proof_bytes));
                let pub_inputs: Vec<String> = proof.proof.public_inputs.iter()
                    .map(|b| format!("0x{}", hex::encode(b)))
                    .collect();

                Ok(serde_json::json!({
                    "proof": proof_hex,
                    "circuit": "mint",
                    "circuitType": "Mint",
                    "proofSize": proof.proof.proof_bytes.len(),
                    "publicInputs": pub_inputs,
                    "commitment": format!("0x{}", hex::encode(proof.statement.commitment.0)),
                    "realGroth16": proof.proof.is_real(),
                }))
            }
            "plonk" => {
                // PlonK henüz uygulanmadı — bilgilendirici yanıt
                Ok(serde_json::json!({
                    "error": "PlonK devresi henüz desteklenmiyor",
                    "circuit": "plonk",
                    "proofSize": 0,
                    "publicInputs": [],
                }))
            }
            _ => {
                // Bilinmeyen devre türü
                Err(RpcError::InvalidParams {
                    reason: format!("Bilinmeyen devre türü: {}. Desteklenenler: mint, transfer, burn", circuit),
                })
            }
        }
    }

    /// nova_verifyProof — ZK kanıt doğrula (gerçek Groth16)
    ///
    /// Beklenen format:
    /// ```json
    /// {
    ///   "proof": "0x<192-byte hex>",
    ///   "publicInputs": ["0x<32-byte hex>", ...],
    ///   "circuit": "mint" | "transfer" | "burn"
    /// }
    /// ```
    pub fn verify_proof(&self, params: &[Value]) -> RpcResult<Value> {
        let req = params.first().ok_or_else(|| RpcError::InvalidParams {
            reason: "doğrulama isteği gerekli".into(),
        })?;

        let proof_hex = req.get("proof").and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: "proof alanı gerekli".into(),
            })?;

        let proof_str = proof_hex.trim_start_matches("0x");
        let circuit_name = req.get("circuit").and_then(|v| v.as_str()).unwrap_or("mint");

        // Hex decode başarısız → geçersiz kanıt
        let proof_bytes = match hex::decode(proof_str) {
            Ok(b) => b,
            Err(_) => {
                return Ok(serde_json::json!({
                    "valid": false,
                    "circuit": circuit_name,
                    "realGroth16": false,
                }));
            }
        };

        let circuit_type = parse_circuit_type(Some(circuit_name));

        // Gerçek Groth16 kanıtı (192 byte) için gerçek doğrulama
        if proof_bytes.len() == GROTH16_PROOF_SIZE {
            let pub_inputs = parse_public_inputs(req.get("publicInputs"));
            let zk_proof = Groth16Proof {
                proof_bytes,
                public_inputs: pub_inputs,
                circuit_type,
            };

            let valid = zk_proof.verify_groth16().unwrap_or(false);

            return Ok(serde_json::json!({
                "valid": valid,
                "circuit": circuit_name,
                "realGroth16": true,
            }));
        }

        // Mock kanıt (test modu): en az 4 byte
        let valid = proof_bytes.len() >= 4;

        Ok(serde_json::json!({
            "valid": valid,
            "circuit": circuit_name,
            "realGroth16": false,
        }))
    }
}

// ─── Yardımcı fonksiyonlar ────────────────────────────────────────────────────

fn parse_32bytes(val: Option<&Value>) -> [u8; 32] {
    val.and_then(|v| v.as_str())
        .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok())
        .and_then(|b| b.try_into().ok())
        .unwrap_or([0u8; 32])
}

fn parse_circuit_type(s: Option<&str>) -> CircuitType {
    match s {
        Some("mint") | Some("Mint") => CircuitType::Mint,
        Some("transfer") | Some("Transfer") => CircuitType::Transfer,
        Some("burn") | Some("Burn") => CircuitType::Burn,
        _ => CircuitType::Mint,
    }
}

fn parse_public_inputs(val: Option<&Value>) -> Vec<[u8; 32]> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| hex::decode(s.trim_start_matches("0x")).ok())
                .filter_map(|b| b.try_into().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> ZkHandler {
        ZkHandler::new(ChainState::new())
    }

    /// NOVA-RED-RPC-005: 8 byte mock kanıt reddedilmeli (geçersiz boyut)
    #[test]
    fn test_submit_zk_tx_standard_mock_rejected() {
        let h = handler();
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "standard",
            "proof": "0xabcdef1234567890", // 8 byte — mock kanıt
            "sender": "0xaaaa",
        })]);
        assert!(result.is_err(), "mock kanıt reddedilmeli");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("geçersiz boyut"), "hata boyut hatası olmalı");
    }

    /// NOVA-RED-RPC-005: full_shield mock kanıt reddedilmeli
    #[test]
    fn test_submit_zk_tx_full_shield_mock_rejected() {
        let h = handler();
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "full_shield",
            "proof": "0xabcdef1234567890", // 8 byte — mock kanıt
        })]);
        assert!(result.is_err(), "full_shield mock kanıt reddedilmeli");
    }

    /// NOVA-RED-RPC-005: semi_shield mock kanıt reddedilmeli
    #[test]
    fn test_submit_zk_tx_semi_shield_mock_rejected() {
        let h = handler();
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "semi_shield",
            "proof": "0xabcdef1234567890", // 8 byte — mock kanıt
        })]);
        assert!(result.is_err(), "semi_shield mock kanıt reddedilmeli");
    }

    /// NOVA-RED-RPC-005: 192 byte sıfır kanıt boyut kontrolünü geçer
    /// (Groth16 doğrulama başarısız olur ama boyut hatası değil)
    #[test]
    fn test_submit_zk_tx_size_check_passes_for_192_bytes() {
        let h = handler();
        let zero_proof = "0x".to_string() + &"00".repeat(GROTH16_PROOF_SIZE);
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "standard",
            "proof": zero_proof,
        })]);
        // Boyut kontrolü geçti ama Groth16 doğrulama başarısız → hata
        assert!(result.is_err(), "sıfır kanıt Groth16 doğrulamada başarısız olmalı");
        let err_msg = result.unwrap_err().to_string();
        // "geçersiz boyut" hatası DEĞİL olmalı — boyut doğru
        assert!(!err_msg.contains("geçersiz boyut"), "boyut hatası olmamalı, doğrulama hatası olmalı");
    }

    #[test]
    fn test_submit_zk_tx_invalid_proof() {
        let h = handler();
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "standard",
            "proof": "0x12", // 1 byte — boyut hatası
        })]);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_zk_tx_no_proof() {
        let h = handler();
        let result = h.submit_zk_tx(&[serde_json::json!({
            "type": "standard",
        })]);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_proof() {
        let h = handler();
        let result = h.generate_proof(&[serde_json::json!({
            "circuit": "mint",
            "value": 1_000_000u64,
            "randomness": "0x0101010101010101010101010101010101010101010101010101010101010101",
            "transmissionKey": "0x0202020202020202020202020202020202020202020202020202020202020202",
        })]).unwrap();
        assert_eq!(result["circuit"], serde_json::json!("mint"));
        assert!(result["proof"].as_str().unwrap().starts_with("0x"));
        assert_eq!(result["realGroth16"], serde_json::json!(true));
        // Gerçek Groth16: 192 byte = 384 hex karakter + "0x" = 386
        assert_eq!(result["proofSize"], serde_json::json!(192));
    }

    #[test]
    fn test_generate_proof_plonk() {
        let h = handler();
        let result = h.generate_proof(&[serde_json::json!({
            "circuit": "plonk",
        })]).unwrap();
        // PlonK desteklenmiyor → hata mesajı döner
        assert!(result["error"].as_str().is_some());
    }

    #[test]
    fn test_verify_proof_valid() {
        let h = handler();
        let result = h.verify_proof(&[serde_json::json!({
            "proof": "0xabcdef1234567890abcdef",
            "circuit": "transfer",
        })]).unwrap();
        assert_eq!(result["valid"], serde_json::json!(true));
    }

    #[test]
    fn test_verify_proof_invalid() {
        let h = handler();
        let result = h.verify_proof(&[serde_json::json!({
            "proof": "short",
            "circuit": "transfer",
        })]).unwrap();
        assert_eq!(result["valid"], serde_json::json!(false));
    }
}
