//! nova_dex_* RPC metodları — Nova DEX servisine HTTP proxy.
//!
//! ## Çalışma Prensibi
//!
//! ```text
//! JSON-RPC istemci → nova_rpc → nova_dex_server (HTTP REST)
//!                     ↑               ↑
//!                  port 9944       port 3000
//! ```
//!
//! DEX sunucu adresi `NOVA_DEX_URL` env değişkeninden okunur.
//! Varsayılan: `http://localhost:3000`
//!
//! ## Metodlar
//!
//! | JSON-RPC Metodu         | DEX Endpoint                         |
//! |-------------------------|--------------------------------------|
//! | nova_dex_health         | GET  /health                         |
//! | nova_dex_createPool     | POST /v1/pools                       |
//! | nova_dex_getPools       | GET  /v1/pools                       |
//! | nova_dex_getPool        | GET  /v1/pools/:id                   |
//! | nova_dex_swap           | POST /v1/pools/:id/swap              |
//! | nova_dex_addLiquidity   | POST /v1/pools/:id/liquidity         |
//! | nova_dex_removeLiquidity| DELETE /v1/pools/:id/liquidity       |
//! | nova_dex_extendLock     | POST /v1/pools/:id/lock/extend       |
//! | nova_dex_getPosition    | GET  /v1/pools/:id/lp/:address       |
//! | nova_dex_processPayouts | POST /v1/payouts/process             |

use crate::errors::{RpcError, RpcResult};
use serde_json::Value;

const DEFAULT_DEX_URL: &str = "http://localhost:3000";

/// Nova DEX JSON-RPC proxy handler.
pub struct DexHandler {
    dex_url: String,
}

impl DexHandler {
    /// Yeni handler oluştur. DEX URL `NOVA_DEX_URL` env'den okunur.
    pub fn new() -> Self {
        let dex_url = std::env::var("NOVA_DEX_URL")
            .unwrap_or_else(|_| DEFAULT_DEX_URL.to_string());
        Self { dex_url }
    }

    // ─── Yardımcı proxy metodları ─────────────────────────────────────────────

    fn proxy_get(&self, path: &str) -> RpcResult<Value> {
        let url = format!("{}{}", self.dex_url, path);
        let resp = ureq::get(&url)
            .call()
            .map_err(|e| RpcError::InternalError {
                reason: format!("DEX proxy hatası [{path}]: {e}"),
            })?;
        resp.into_json::<Value>().map_err(|e| RpcError::InternalError {
            reason: format!("DEX yanıt parse hatası: {e}"),
        })
    }

    fn proxy_post(&self, path: &str, body: &Value) -> RpcResult<Value> {
        let url = format!("{}{}", self.dex_url, path);
        let resp = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| RpcError::InternalError {
                reason: format!("DEX proxy hatası [{path}]: {e}"),
            })?;
        resp.into_json::<Value>().map_err(|e| RpcError::InternalError {
            reason: format!("DEX yanıt parse hatası: {e}"),
        })
    }

    fn proxy_delete(&self, path: &str, body: &Value) -> RpcResult<Value> {
        let url = format!("{}{}", self.dex_url, path);
        let resp = ureq::delete(&url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| RpcError::InternalError {
                reason: format!("DEX proxy hatası [{path}]: {e}"),
            })?;
        resp.into_json::<Value>().map_err(|e| RpcError::InternalError {
            reason: format!("DEX yanıt parse hatası: {e}"),
        })
    }

    fn require_str<'a>(params: &'a [Value], idx: usize, name: &str) -> RpcResult<&'a str> {
        params
            .get(idx)
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::InvalidParams {
                reason: format!("'{name}' parametresi gerekli (string)"),
            })
    }

    fn require_obj(params: &[Value], idx: usize, name: &str) -> RpcResult<Value> {
        params
            .get(idx)
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| RpcError::InvalidParams {
                reason: format!("'{name}' parametresi gerekli (object)"),
            })
    }

    // ─── Endpoint'ler ─────────────────────────────────────────────────────────

    /// nova_dex_health — DEX servisinin sağlık durumunu sorgula.
    pub fn health(&self, _params: &[Value]) -> RpcResult<Value> {
        self.proxy_get("/health")
    }

    /// nova_dex_createPool — Yeni likidite pool'u oluştur.
    ///
    /// Params: [{ creator, asset_a, asset_b, initial_a, initial_b,
    ///            lock_duration, creation_fee_paid, nova_price_cents }]
    pub fn create_pool(&self, params: &[Value]) -> RpcResult<Value> {
        let body = Self::require_obj(params, 0, "pool")?;
        self.proxy_post("/v1/pools", &body)
    }

    /// nova_dex_getPools — Tüm pool'ları listele.
    pub fn get_pools(&self, _params: &[Value]) -> RpcResult<Value> {
        self.proxy_get("/v1/pools")
    }

    /// nova_dex_getPool — Tek pool detayı.
    ///
    /// Params: [pool_id_hex]
    pub fn get_pool(&self, params: &[Value]) -> RpcResult<Value> {
        let pool_id = Self::require_str(params, 0, "pool_id")?;
        self.proxy_get(&format!("/v1/pools/{pool_id}"))
    }

    /// nova_dex_swap — MEV korumalı swap (commit-reveal protokolü).
    ///
    /// Params: [pool_id_hex, { sender, zero_for_one, amount_in, min_amount_out }]
    ///
    /// Dahili olarak commit-reveal akışını yönetir:
    /// 1. GET /v1/pools/{pool_id} → asset_a, asset_b
    /// 2. Rastgele nonce üret
    /// 3. Commitment hash hesapla
    /// 4. POST /v1/mev/commit
    /// 5. 500ms bekle (slot kapanması)
    /// 6. POST /v1/mev/reveal
    pub fn swap(&self, params: &[Value]) -> RpcResult<Value> {
        use sha2::Digest;
        use rand::RngCore;

        let pool_id = Self::require_str(params, 0, "pool_id")?;
        let body    = Self::require_obj(params, 1, "swap")?;

        let sender       = body["sender"].as_str().ok_or_else(|| crate::errors::RpcError::InvalidParams {
            reason: "'sender' alanı gerekli".to_string(),
        })?;
        let zero_for_one = body["zero_for_one"].as_bool().unwrap_or(true);
        let amount_in    = body["amount_in"].as_str().ok_or_else(|| crate::errors::RpcError::InvalidParams {
            reason: "'amount_in' alanı gerekli".to_string(),
        })?;
        let min_out      = body["min_amount_out"].as_str().unwrap_or("0");

        // 1. Pool detaylarını al (asset_a / asset_b için)
        let pool: Value = self.proxy_get(&format!("/v1/pools/{pool_id}"))?;
        let asset_a = pool["asset_a"].as_str().ok_or_else(|| crate::errors::RpcError::InternalError {
            reason: "asset_a okunamadı".to_string(),
        })?;
        let asset_b = pool["asset_b"].as_str().ok_or_else(|| crate::errors::RpcError::InternalError {
            reason: "asset_b okunamadı".to_string(),
        })?;

        let (asset_in, asset_out) = if zero_for_one {
            (asset_a, asset_b)
        } else {
            (asset_b, asset_a)
        };

        // 2. Rastgele nonce
        let mut nonce = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce);

        // 3. Commitment hash: SHA256("nova_dex_swap_intent_v1:" || sender || pool_id || asset_in || asset_out || amount_in_le16 || min_out_le16 || nonce)
        let sender_bytes  = hex::decode(sender).unwrap_or_default();
        let pool_bytes    = hex::decode(pool_id).unwrap_or_default();
        let in_bytes      = hex::decode(asset_in).unwrap_or_default();
        let out_bytes     = hex::decode(asset_out).unwrap_or_default();
        let amount_in_u128: u128 = amount_in.parse().unwrap_or(0);
        let min_out_u128:   u128 = min_out.parse().unwrap_or(0);

        let mut h = sha2::Sha256::new();
        h.update(b"nova_dex_swap_intent_v1:");
        h.update(&sender_bytes);
        h.update(&pool_bytes);
        h.update(&in_bytes);
        h.update(&out_bytes);
        h.update(amount_in_u128.to_le_bytes());
        h.update(min_out_u128.to_le_bytes());
        h.update(nonce);
        let commitment_hash = hex::encode(h.finalize());
        let nonce_hex       = hex::encode(nonce);

        // 4. Commit gönder
        let commit_body = serde_json::json!({
            "commitment_hash": commitment_hash,
            "sender": sender,
        });
        self.proxy_post("/v1/mev/commit", &commit_body)?;

        // 5. Slot kapanmasını bekle
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 6. Reveal gönder
        let reveal_body = serde_json::json!({
            "sender":         sender,
            "pool_id":        pool_id,
            "asset_in":       asset_in,
            "asset_out":      asset_out,
            "amount_in":      amount_in,
            "min_amount_out": min_out,
            "nonce":          nonce_hex,
        });
        self.proxy_post("/v1/mev/reveal", &reveal_body)
    }

    /// nova_dex_addLiquidity — Likidite ekle.
    ///
    /// Params: [pool_id_hex, { provider, amount_a, amount_b }]
    pub fn add_liquidity(&self, params: &[Value]) -> RpcResult<Value> {
        let pool_id = Self::require_str(params, 0, "pool_id")?;
        let body    = Self::require_obj(params, 1, "liquidity")?;
        self.proxy_post(&format!("/v1/pools/{pool_id}/liquidity"), &body)
    }

    /// nova_dex_removeLiquidity — Likidite çek.
    ///
    /// Params: [pool_id_hex, { provider, lp_to_burn }]
    pub fn remove_liquidity(&self, params: &[Value]) -> RpcResult<Value> {
        let pool_id = Self::require_str(params, 0, "pool_id")?;
        let body    = Self::require_obj(params, 1, "remove_liquidity")?;
        self.proxy_delete(&format!("/v1/pools/{pool_id}/liquidity"), &body)
    }

    /// nova_dex_extendLock — Kilit süresini uzat.
    ///
    /// Params: [pool_id_hex, { caller, new_duration }]
    pub fn extend_lock(&self, params: &[Value]) -> RpcResult<Value> {
        let pool_id = Self::require_str(params, 0, "pool_id")?;
        let body    = Self::require_obj(params, 1, "extend_lock")?;
        self.proxy_post(&format!("/v1/pools/{pool_id}/lock/extend"), &body)
    }

    /// nova_dex_getPosition — LP pozisyonunu sorgula.
    ///
    /// Params: [pool_id_hex, address_hex]
    pub fn get_position(&self, params: &[Value]) -> RpcResult<Value> {
        let pool_id = Self::require_str(params, 0, "pool_id")?;
        let address = Self::require_str(params, 1, "address")?;
        self.proxy_get(&format!("/v1/pools/{pool_id}/lp/{address}"))
    }

    /// nova_dex_processPayouts — Creator ödemelerini tetikle (24h cycle).
    pub fn process_payouts(&self, _params: &[Value]) -> RpcResult<Value> {
        self.proxy_post("/v1/payouts/process", &Value::Null)
    }
}
