//! Nova Shield Chain — gerçek HTTP JSON-RPC istemcisi
//!
//! ## Mimari
//!
//! `NovaRpcClient` senkron `ureq` üzerinden JSON-RPC 2.0 çağrıları yapar.
//! - Atomik ID counter (thread-safe)
//! - Yapılandırılabilir timeout
//! - Bağlantı hatası → `SdkError::NetworkError`
//! - RPC hatası → `SdkError::RpcError`
//!
//! ## Pattern
//!
//! go-ethereum `ethclient.Client` + web3.py `HTTPProvider` pattern:
//! endpoint URL + request builder + response parse.
//!
//! ## Kullanım
//!
//! ```rust,ignore
//! let client = NovaRpcClient::new("http://127.0.0.1:9944");
//! let block_num = client.block_number()?;
//! let balance = client.get_balance("0xAbCd...")?;
//! ```

use crate::errors::{SdkError, SdkResult};
use crate::rpc::types::{NovaRpcMethod, RpcRequest, RpcResponse};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Varsayılan istek timeout'u (5 saniye)
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// Varsayılan maksimum retry sayısı
pub const DEFAULT_MAX_RETRIES: u32 = 3;

// ─── NovaRpcClient ────────────────────────────────────────────────────────────

/// Gerçek HTTP JSON-RPC istemcisi
///
/// Thread-safe: `Arc` içinde paylaşılabilir.
///
/// Pattern: go-ethereum `ethclient.Client { c *rpc.Client }`.
#[derive(Clone)]
pub struct NovaRpcClient {
    /// JSON-RPC endpoint URL'si (örn. "http://127.0.0.1:9944")
    endpoint: String,
    /// İstek timeout'u (milisaniye)
    timeout_ms: u64,
    /// Otomatik artan istek ID
    next_id: Arc<AtomicU64>,
}

impl NovaRpcClient {
    /// Varsayılan timeout ile istemci oluştur
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Özel timeout ile istemci oluştur
    pub fn with_timeout(endpoint: impl Into<String>, timeout_ms: u64) -> Self {
        Self {
            endpoint: endpoint.into(),
            timeout_ms,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Endpoint URL'si
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Timeout (ms)
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Sonraki istek ID'sini al (monoton artan)
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    // ─── Düşük Seviye ─────────────────────────────────────────────────────

    /// Ham JSON-RPC çağrısı yap
    ///
    /// Ağ hatası → `SdkError::NetworkError`
    /// RPC hata yanıtı → `SdkError::RpcError`
    pub fn call(&self, method: &str, params: Vec<Value>) -> SdkResult<RpcResponse> {
        let id = self.next_id();
        let req = RpcRequest::new(method, params, id);
        let body = serde_json::to_value(&req).map_err(|e| SdkError::SerializationError {
            reason: e.to_string(),
        })?;

        let timeout = Duration::from_millis(self.timeout_ms);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout(timeout)
            .build();

        let raw_resp = agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| SdkError::ConnectionError { endpoint: format!("{}: {}", self.endpoint, e) })?;

        let resp: RpcResponse = raw_resp
            .into_json()
            .map_err(|e| SdkError::SerializationError { reason: e.to_string() })?;

        // RPC katmanı hatası
        if let Some(err) = &resp.error {
            return Err(SdkError::RpcError {
                reason: format!("[{}] {}", err.code, err.message),
            });
        }

        Ok(resp)
    }

    // ─── Yüksek Seviye API ────────────────────────────────────────────────

    /// Güncel blok numarasını sorgula
    ///
    /// Pattern: `eth_blockNumber` → Nova Shield `nova_blockNumber`.
    pub fn block_number(&self) -> SdkResult<u64> {
        let resp = self.call(NovaRpcMethod::CHAIN_GET_BLOCK_NUMBER, vec![])?;
        let val = resp.unwrap_result()?;
        // Hex string ("0x...") veya integer
        let n = if let Some(s) = val.as_str() {
            let hex = s.trim_start_matches("0x");
            u64::from_str_radix(hex, 16).unwrap_or(0)
        } else {
            val.as_u64().unwrap_or(0)
        };
        Ok(n)
    }

    /// Adresin bakiyesini sorgula (NSC, ham integer)
    ///
    /// Pattern: `eth_getBalance(address, "latest")` analolu.
    pub fn get_balance(&self, address: &str) -> SdkResult<u128> {
        let resp = self.call(
            NovaRpcMethod::ACCOUNT_BALANCE,
            vec![serde_json::json!(address)],
        )?;
        let val = resp.unwrap_result()?;
        let n = if let Some(s) = val.as_str() {
            let hex = s.trim_start_matches("0x");
            u128::from_str_radix(hex, 16).unwrap_or(0)
        } else {
            val.as_u64().map(|v| v as u128).unwrap_or(0)
        };
        Ok(n)
    }

    /// Hesap bilgilerini sorgula (nonce, balance)
    pub fn get_account(&self, address: &str) -> SdkResult<Value> {
        let resp = self.call(
            NovaRpcMethod::ACCOUNT_GET,
            vec![serde_json::json!(address)],
        )?;
        resp.unwrap_result().cloned()
    }

    /// İşlemi ağa gönder (ham hex formatında)
    ///
    /// Return: tx_hash (0x prefixli hex)
    pub fn send_transaction(&self, tx_hex: &str) -> SdkResult<String> {
        let resp = self.call(
            NovaRpcMethod::TX_SEND,
            vec![serde_json::json!(tx_hex)],
        )?;
        let val = resp.unwrap_result()?;
        let hash = val
            .get("txHash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(hash)
    }

    /// TX detaylarını sorgula
    pub fn get_transaction(&self, tx_hash: &str) -> SdkResult<Value> {
        let resp = self.call(
            NovaRpcMethod::TX_GET,
            vec![serde_json::json!(tx_hash)],
        )?;
        resp.unwrap_result().cloned()
    }

    /// Blok detaylarını sorgula (yüksekliğe göre)
    pub fn get_block(&self, height: u64) -> SdkResult<Value> {
        let resp = self.call(
            NovaRpcMethod::CHAIN_GET_BLOCK,
            vec![serde_json::json!(height)],
        )?;
        resp.unwrap_result().cloned()
    }

    /// Sağlık kontrolü: bağlantı var mı?
    ///
    /// Return: `Ok(block_number)` ya da bağlantı hatası.
    pub fn health_check(&self) -> SdkResult<u64> {
        self.block_number()
    }
}

impl std::fmt::Debug for NovaRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NovaRpcClient")
            .field("endpoint", &self.endpoint)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Yapı testleri ────────────────────────────────────────────────────

    #[test]
    fn test_client_new_default_timeout() {
        let client = NovaRpcClient::new("http://127.0.0.1:9944");
        assert_eq!(client.endpoint(), "http://127.0.0.1:9944");
        assert_eq!(client.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn test_client_with_custom_timeout() {
        let client = NovaRpcClient::with_timeout("http://localhost:8545", 1_000);
        assert_eq!(client.timeout_ms(), 1_000);
    }

    #[test]
    fn test_client_id_monoton_artis() {
        let client = NovaRpcClient::new("http://127.0.0.1:9944");
        let id1 = client.next_id();
        let id2 = client.next_id();
        let id3 = client.next_id();
        assert!(id2 > id1);
        assert!(id3 > id2);
    }

    #[test]
    fn test_client_clone_shared_counter() {
        let client = NovaRpcClient::new("http://127.0.0.1:9944");
        let cloned = client.clone();
        let id1 = client.next_id();
        let id2 = cloned.next_id();
        // Klonlar aynı sayacı paylaşır → ardışık ID
        assert_eq!(id2, id1 + 1);
    }

    #[test]
    fn test_client_debug_format() {
        let client = NovaRpcClient::new("http://node.example.com:9944");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("NovaRpcClient"));
        assert!(dbg.contains("node.example.com"));
    }

    // ─── Bağlantı hatası testleri ─────────────────────────────────────────

    #[test]
    fn test_call_connection_refused_returns_network_error() {
        // Dinlemeyen bir porta bağlanmaya çalış → NetworkError
        let client = NovaRpcClient::with_timeout("http://127.0.0.1:19877", 300);
        let err = client.call("nova_blockNumber", vec![]).unwrap_err();
        assert!(
            matches!(err, SdkError::ConnectionError { .. }),
            "bağlantı reddedilince NetworkError beklenir, alınan: {err:?}"
        );
    }

    #[test]
    fn test_block_number_unreachable_returns_error() {
        let client = NovaRpcClient::with_timeout("http://127.0.0.1:19878", 300);
        assert!(client.block_number().is_err());
    }

    #[test]
    fn test_get_balance_unreachable_returns_error() {
        let client = NovaRpcClient::with_timeout("http://127.0.0.1:19879", 300);
        assert!(client.get_balance("0x1234").is_err());
    }

    #[test]
    fn test_health_check_unreachable_returns_error() {
        let client = NovaRpcClient::with_timeout("http://127.0.0.1:19880", 300);
        assert!(client.health_check().is_err());
    }

    // ─── Paralel istek ID testi ────────────────────────────────────────────

    #[test]
    fn test_concurrent_id_uniqueness() {
        use std::sync::Mutex;
        let client = NovaRpcClient::new("http://127.0.0.1:9944");
        let ids = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let c = client.clone();
                let ids = Arc::clone(&ids);
                std::thread::spawn(move || {
                    let id = c.next_id();
                    ids.lock().unwrap().push(id);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let mut all_ids = ids.lock().unwrap().clone();
        all_ids.sort_unstable();
        all_ids.dedup();
        assert_eq!(all_ids.len(), 8, "Tüm ID'ler benzersiz olmalı");
    }
}
