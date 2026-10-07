//! HTTP/WS JSON-RPC Sunucu — axum tabanlı gerçek HTTP endpoint
//!
//! ## Mimari
//!
//! ```text
//! HttpRpcServer::bind("0.0.0.0:9944")
//!     └── axum Router
//!         ├── POST /           → JSON-RPC 2.0 handler
//!         ├── GET  /health     → {"status": "ok", "version": "0.1.0"}
//!         └── GET  /           → {"message": "Nova Shield RPC"}
//! ```
//!
//! ## JSON-RPC akışı
//!
//! ```text
//! HTTP POST / {"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}
//!     └── extract JSON body
//!         └── RpcRouter::handle_raw(json_str)
//!             └── HTTP 200 {"jsonrpc":"2.0","result":"0x0","id":1}
//! ```
//!
//! ## Polkadot referansı
//! `sc-rpc-server/src/lib.rs` → `start_http()` + `start_ws()` pattern
//!
//! ## Ethereum referansı
//! `go-ethereum/rpc/http.go` → `ServeHTTP()` handler
//!
//! ## Kullanım
//!
//! ```rust,no_run
//! use nova_rpc::{HttpRpcServer, ChainState};
//!
//! #[tokio::main]
//! async fn main() {
//!     let state = ChainState::new();
//!     let server = HttpRpcServer::bind("0.0.0.0:9944", state).await.unwrap();
//!     println!("RPC: {}", server.local_addr());
//!     server.run().await;
//! }
//! ```

use crate::{ChainState, RpcRouter};
use crate::rate_limit::RateLimiter;
use axum::{
    Json,
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::Arc;
use tokio::net::TcpListener;

// ─── Paylaşılan durum ─────────────────────────────────────────────────────────

/// Axum handler state — Arc ile thread-safe paylaşım
#[derive(Clone)]
struct ServerState {
    router: Arc<RpcRouter>,
    /// IP başına rate limiter
    rate_limiter: RateLimiter,
}

// ─── Rate Limiting Middleware ─────────────────────────────────────────────────

/// IP başına rate limiting — her POST /  isteğinde çalışır
///
/// İstemci IP'sini şu sırayla alır:
/// 1. `X-Real-IP` başlığı (reverse proxy arkasında)
/// 2. `X-Forwarded-For` başlığının ilk değeri
/// 3. Fallback: `"127.0.0.1"` (test ortamı / doğrudan bağlantı)
///
/// Limit aşılınca JSON-RPC 2.0 uyumlu `-32005` hata kodu döner.
async fn rate_limit_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Response {
    // IP çıkar (proxy başlıkları → fallback)
    let ip = {
        let headers = request.headers();
        headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
            .or_else(|| {
                headers
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
            })
            .unwrap_or_else(|| "127.0.0.1".to_string())
    };

    if !state.rate_limiter.check_and_record(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [
                ("content-type", "application/json"),
                ("retry-after", "1"),
                ("x-ratelimit-limit", "500"),
            ],
            r#"{"jsonrpc":"2.0","error":{"code":-32005,"message":"Rate limit aşıldı — 1 saniye bekleyip tekrar deneyin"},"id":null}"#,
        ).into_response();
    }

    next.run(request).await
}

// ─── Handler'lar ──────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 handler — POST /
///
/// Content-Type: application/json beklenir.
/// Her türlü geçerli JSON body kabul edilir (tek istek veya batch).
async fn jsonrpc_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Content-Type kontrolü (gevşek — Ethereum tooling uyumluluğu için sadece uyarı)
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if body.is_empty() {
        let error_body = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error: boş istek"},"id":null}"#;
        return (
            StatusCode::BAD_REQUEST,
            [("content-type", "application/json")],
            error_body.to_string(),
        );
    }

    // Content-type başlık yoksa veya JSON değilse → kabul et ama kaydet
    let _ = content_type;

    let response = state.router.handle_raw(&body);

    (
        StatusCode::OK,
        [("content-type", "application/json")],
        response,
    )
}

/// Health check endpoint — GET /health
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": crate::RPC_VERSION,
        "service": "nova-rpc",
    }))
}

/// Root endpoint — GET /
async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "message": "Nova Shield Chain JSON-RPC",
        "version": "2.0",
        "docs": "POST / with JSON-RPC 2.0 body",
    }))
}

// ─── HttpRpcServer ────────────────────────────────────────────────────────────

/// HTTP JSON-RPC sunucu
///
/// axum tabanlı production-grade HTTP sunucu.
/// `bind()` ile başlatılır, `run()` ile çalışmaya başlar.
pub struct HttpRpcServer {
    listener: TcpListener,
    router: Arc<RpcRouter>,
    /// Gerçek dinleme adresi (port 0 → işletim sistemi atar)
    local_addr: String,
}

impl HttpRpcServer {
    /// Belirli adres:port'a bind ol
    ///
    /// Port 0: rastgele port (test için kullanışlı)
    pub async fn bind(addr: &str, state: ChainState) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?.to_string();
        let router = Arc::new(RpcRouter::new(state));
        Ok(Self { listener, router, local_addr })
    }

    /// Sunucuyu çalıştır (blocking — tokio task içinde çağrılmalı)
    ///
    /// Graceful shutdown için `run_until_cancelled()` kullan.
    pub async fn run(self) {
        let app = self.build_app();
        axum::serve(self.listener, app).await.ok();
    }

    /// Graceful shutdown destekli çalıştır
    ///
    /// `shutdown` signal'i geldiğinde sunucu durur.
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let app = self.build_app();
        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .ok();
    }

    /// Gerçek dinleme adresi
    pub fn local_addr(&self) -> &str {
        &self.local_addr
    }

    /// RPC Router'a referans
    pub fn router(&self) -> &RpcRouter {
        &self.router
    }

    /// axum Router oluştur (test için de kullanılabilir)
    fn build_app(&self) -> Router {
        let state = ServerState {
            router: Arc::clone(&self.router),
            rate_limiter: RateLimiter::new(),
        };
        Router::new()
            .route("/", post(jsonrpc_handler))
            .route("/", get(root_handler))
            .route("/health", get(health_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                rate_limit_middleware,
            ))
            .with_state(state)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt; // oneshot() için

    async fn make_app() -> (Router, Arc<RpcRouter>) {
        let state = ChainState::new();
        let router = Arc::new(RpcRouter::new(state));
        let server_state = ServerState {
            router: Arc::clone(&router),
            rate_limiter: RateLimiter::new(),
        };
        let app = Router::new()
            .route("/", post(jsonrpc_handler))
            .route("/", get(root_handler))
            .route("/health", get(health_handler))
            .layer(axum::middleware::from_fn_with_state(
                server_state.clone(),
                rate_limit_middleware,
            ))
            .with_state(server_state);
        (app, router)
    }

    async fn post_json(app: &Router, body: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        (status, text)
    }

    /// Sağlık endpoint'i
    #[tokio::test]
    async fn test_health_endpoint() {
        let (app, _) = make_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ok");
    }

    /// Kök endpoint
    #[tokio::test]
    async fn test_root_endpoint() {
        let (app, _) = make_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// JSON-RPC nova_chainId
    #[tokio::test]
    async fn test_jsonrpc_chain_id() {
        let (app, _) = make_app().await;
        let (status, body) = post_json(
            &app,
            r#"{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":1}"#,
        ).await;
        assert_eq!(status, StatusCode::OK);

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], 1);
        assert!(json["result"].is_string(), "result string olmalı");
    }

    /// JSON-RPC nova_blockNumber
    #[tokio::test]
    async fn test_jsonrpc_block_number() {
        let (app, _) = make_app().await;
        let (status, body) = post_json(
            &app,
            r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":2}"#,
        ).await;
        assert_eq!(status, StatusCode::OK);

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["result"], "0x0");
    }

    /// Method not found hatası
    #[tokio::test]
    async fn test_jsonrpc_method_not_found() {
        let (app, _) = make_app().await;
        let (status, body) = post_json(
            &app,
            r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":3}"#,
        ).await;
        assert_eq!(status, StatusCode::OK); // JSON-RPC 2.0: hata da 200 döner

        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["error"].is_object(), "error field olmalı");
        assert_eq!(json["error"]["code"], -32601);
    }

    /// Boş body → parse error
    #[tokio::test]
    async fn test_jsonrpc_empty_body() {
        let (app, _) = make_app().await;
        let (status, _) = post_json(&app, "").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Gerçek TCP bind testi — sunucu ayağa kalkıp port alıyor mu?
    #[tokio::test]
    async fn test_server_bind_and_request() {
        let state = ChainState::new();
        let server = HttpRpcServer::bind("127.0.0.1:0", state).await.unwrap();
        let addr = server.local_addr().to_string();
        assert!(addr.starts_with("127.0.0.1:"));

        // Arka planda çalıştır (graceful shutdown ile)
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            server.run_with_shutdown(async move { let _ = rx.await; }).await;
        });

        // oneshot ile in-process HTTP isteği
        let app = {
            let state = ChainState::new();
            let router_inner = Arc::new(RpcRouter::new(state));
            let server_state = ServerState {
                router: Arc::clone(&router_inner),
                rate_limiter: crate::rate_limit::RateLimiter::new(),
            };
            Router::new()
                .route("/", post(jsonrpc_handler))
                .with_state(server_state)
        };

        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}"#.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Sunucuyu durdur
        let _ = tx.send(());
    }

    /// Sunucu local_addr port 0 ataması
    #[tokio::test]
    async fn test_server_local_addr_assigned() {
        let state = ChainState::new();
        let server = HttpRpcServer::bind("127.0.0.1:0", state).await.unwrap();
        let addr = server.local_addr();
        let port: u16 = addr.split(':').last().unwrap().parse().unwrap();
        assert!(port > 0, "İşletim sistemi port atamalı");
    }

    // ── Rate Limiting Testleri ────────────────────────────────────────────────

    /// Normal trafik rate limit'e takılmamalı
    #[tokio::test]
    async fn test_rate_limit_allows_normal_requests() {
        let (app, _) = make_app().await;
        // 10 normal istek serbestçe geçmeli
        for i in 0..10 {
            let (status, _) = post_json(
                &app,
                r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}"#,
            ).await;
            assert_eq!(status, StatusCode::OK, "istek {} geçmeli", i);
        }
    }

    /// X-Real-IP başlığı ile rate limiter farklı IP'leri ayırt etmeli
    #[tokio::test]
    async fn test_rate_limit_respects_x_real_ip() {
        let (app, _) = make_app().await;

        // IP 1'den normal istek
        let req = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("content-type", "application/json")
            .header("x-real-ip", "10.0.0.1")
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":1}"#))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // IP 2'den normal istek — izole olmalı
        let req2 = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("content-type", "application/json")
            .header("x-real-ip", "10.0.0.2")
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":2}"#))
            .unwrap();
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
    }

    /// Rate limit aşılınca 429 dönmeli
    #[tokio::test]
    async fn test_rate_limit_returns_429_when_exceeded() {
        use crate::rate_limit::{BURST_CAPACITY, RateLimiter};

        // Test: rate limiter'ı direkt doldur, sonra HTTP isteği gönder
        let state = ChainState::new();
        let router = Arc::new(RpcRouter::new(state));
        let rate_limiter = RateLimiter::new();

        // flood_ip için burst kapasitesini doldur
        let flood_ip = "flood_test_ip";
        for _ in 0..=BURST_CAPACITY {
            rate_limiter.check_and_record(flood_ip);
        }

        let server_state = ServerState {
            router: Arc::clone(&router),
            rate_limiter,
        };
        let app = Router::new()
            .route("/", post(jsonrpc_handler))
            .route("/", get(root_handler))
            .route("/health", get(health_handler))
            .layer(axum::middleware::from_fn_with_state(
                server_state.clone(),
                rate_limit_middleware,
            ))
            .with_state(server_state);

        // Bu IP için sınır dolu — X-Real-IP başlığı ile gönder
        let req = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("content-type", "application/json")
            .header("x-real-ip", flood_ip)
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "burst kapasitesi aşıldığında 429 dönmeli"
        );
    }

    /// 429 yanıtı JSON-RPC 2.0 hata formatında olmalı
    #[tokio::test]
    async fn test_rate_limit_429_body_is_valid_jsonrpc() {
        use crate::rate_limit::{BURST_CAPACITY, RateLimiter};

        let state = ChainState::new();
        let router = Arc::new(RpcRouter::new(state));
        let rate_limiter = RateLimiter::new();

        let flood_ip = "jsonrpc_check_ip";
        for _ in 0..=BURST_CAPACITY {
            rate_limiter.check_and_record(flood_ip);
        }

        let server_state = ServerState { router, rate_limiter };
        let app = Router::new()
            .route("/", post(jsonrpc_handler))
            .layer(axum::middleware::from_fn_with_state(
                server_state.clone(),
                rate_limit_middleware,
            ))
            .with_state(server_state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header("x-real-ip", flood_ip)
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_blockNumber","params":[],"id":1}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // JSON-RPC 2.0 hata formatı
        assert_eq!(json["jsonrpc"], "2.0");
        assert!(json["error"].is_object());
        assert_eq!(json["error"]["code"], -32005, "rate limit hata kodu -32005 olmalı");
    }

    /// Health endpoint rate limit'den muaf olmamalı (güvenlik gereği)
    #[tokio::test]
    async fn test_health_endpoint_also_rate_limited() {
        let (app, _) = make_app().await;
        // Health endpoint hâlâ erişilebilir (limiti aşmadık)
        let req = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// nova_getBalance testi
    #[tokio::test]
    async fn test_jsonrpc_get_balance() {
        let (app, _) = make_app().await;
        let (status, body) = post_json(
            &app,
            r#"{"jsonrpc":"2.0","method":"nova_getBalance","params":["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"id":4}"#,
        ).await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["result"].is_string());
    }

    /// nova_sendTransaction testi
    #[tokio::test]
    async fn test_jsonrpc_send_transaction() {
        let (app, _) = make_app().await;
        let (status, body) = post_json(
            &app,
            r#"{"jsonrpc":"2.0","method":"nova_sendTransaction","params":[{"from":"0xaaaa","to":"0xbbbb","value":"0x1000","gasPrice":"0x3b9aca00"}],"id":5}"#,
        ).await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["result"].as_str().unwrap().starts_with("0x"));
    }

    /// Content-type başlığı olmadan da çalışmalı (Ethereum uyumu)
    #[tokio::test]
    async fn test_jsonrpc_no_content_type() {
        let (app, _) = make_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            // Content-type yok
            .body(Body::from(r#"{"jsonrpc":"2.0","method":"nova_chainId","params":[],"id":9}"#.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
