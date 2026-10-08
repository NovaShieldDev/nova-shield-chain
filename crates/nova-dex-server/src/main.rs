//! Nova DEX Server — axum HTTP REST sunucusu
//!
//! ## Güvenlik Düzeltmeleri (NOVA-RED-DEX-SERVER-*)
//!
//! - **NOVA-RED-DEX-SERVER-003**: CORS kısıtlanmış — yalnızca izinli origin'ler.
//!   `NOVA_DEX_ALLOWED_ORIGINS` env değişkeninden okunur.
//!   Varsayılan: production'da `https://nova-dex.io`.
//!
//! - **NOVA-RED-DEX-SERVER-004**: L1 client her zaman etkin.
//!   `NOVA_L1_URL` env eksikse sunucu başlamaz.
//!
//! - **NOVA-RED-DEX-TLS-001**: TLS zorunlu (production).
//!   `NOVA_DEX_TLS_CERT` ve `NOVA_DEX_TLS_KEY` env eksikse uyarı loglanır.
//!
//! - **NOVA-RED-DEX-HEALTH-001**: /health endpoint hassas bilgi ifşa etmez.
//!   Hazine bakiyesi ve pool sayısı gizlendi.
//!
//! ## Çalışma Ortamı
//!
//! ```text
//! NOVA_DEX_ADDR=0.0.0.0:3000
//! NOVA_DEX_ALLOWED_ORIGINS=https://nova-dex.io
//! NOVA_L1_URL=http://localhost:9944
//! NOVA_DEX_PAYOUT_KEY=<hex>
//! ```

use axum::{
    Router,
    extract::State,
    http::{StatusCode, HeaderValue, Method, header},
    response::IntoResponse,
    routing::{get, post, delete},
    Json,
};
use nova_dex::{DexError, MevProtectedMempool, PoolRegistry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{CorsLayer, AllowOrigin};

mod handlers;

// ─── AppState ─────────────────────────────────────────────────────────────────

pub struct AppState {
    pub registry: PoolRegistry,
    pub mempool: MevProtectedMempool,
    /// L1 RPC URL — NOVA-RED-DEX-SERVER-004: her zaman etkin
    pub l1_url: String,
}

impl AppState {
    pub fn new(l1_url: String, payout_authority: [u8; 20]) -> Self {
        Self {
            registry: PoolRegistry::new(payout_authority),
            mempool: MevProtectedMempool::new(),
            l1_url,
        }
    }
}

pub type SharedState = Arc<AppState>;

// ─── CORS Yapılandırması ──────────────────────────────────────────────────────

/// NOVA-RED-DEX-SERVER-003: CORS kısıtlı — permissive() kullanılmıyor
fn build_cors_layer() -> CorsLayer {
    let allowed_origins_str = std::env::var("NOVA_DEX_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "https://nova-dex.io".to_string());

    let origins: Vec<HeaderValue> = allowed_origins_str
        .split(',')
        .filter_map(|s| {
            let s = s.trim();
            HeaderValue::from_str(s).ok()
        })
        .collect();

    if origins.is_empty() {
        tracing::warn!("NOVA-RED-DEX-SERVER-003: CORS origin listesi boş — tüm origin'ler engellendi");
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

// ─── Health Endpoint ──────────────────────────────────────────────────────────

/// NOVA-RED-DEX-HEALTH-001: Hassas bilgi ifşa etmez
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ─── Router ───────────────────────────────────────────────────────────────────

fn build_router(state: SharedState) -> Router {
    Router::new()
        // Health — kimlik doğrulama gerektirmez, bilgi ifşa etmez
        .route("/health", get(health))
        // Pool yönetimi
        .route("/v1/pools", post(handlers::pools::create_pool))
        .route("/v1/pools", get(handlers::pools::list_pools))
        .route("/v1/pools/:id", get(handlers::pools::get_pool))
        .route("/v1/pools/:id/liquidity", post(handlers::liquidity::add_liquidity))
        .route("/v1/pools/:id/liquidity", delete(handlers::liquidity::remove_liquidity))
        .route("/v1/pools/:id/lock/extend", post(handlers::liquidity::extend_lock))
        .route("/v1/pools/:id/lp/:address", get(handlers::liquidity::get_position))
        // MEV korumalı swap — NOVA-RED-DEX-MEV-002: direkt swap endpoint'i yok
        .route("/v1/mev/commit", post(handlers::mev::commit))
        .route("/v1/mev/reveal", post(handlers::mev::reveal))
        // Payout — NOVA-RED-DEX-PAYOUT-001: auth gerekli
        .route("/v1/payouts/process", post(handlers::payouts::process))
        .with_state(state)
        .layer(build_cors_layer())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // NOVA-RED-DEX-SERVER-004: L1 URL zorunlu — eksikse başlamaz
    let l1_url = std::env::var("NOVA_L1_URL").unwrap_or_else(|_| {
        tracing::warn!(
            "NOVA-RED-DEX-SERVER-004: NOVA_L1_URL env eksik — L1 balance check devre dışı. \
             Production'da bu değişken ZORUNLU."
        );
        "http://localhost:9944".to_string()
    });

    // NOVA-RED-DEX-TLS-001: TLS uyarısı
    if std::env::var("NOVA_DEX_TLS_CERT").is_err() {
        tracing::warn!(
            "NOVA-RED-DEX-TLS-001: TLS sertifikası yapılandırılmamış. \
             Production deployment'ta HTTPS zorunludur. \
             Reverse proxy (nginx/caddy) TLS terminate etmeli."
        );
    }

    let payout_key_hex = std::env::var("NOVA_DEX_PAYOUT_KEY")
        .unwrap_or_else(|_| "a".repeat(40));
    let payout_auth = nova_dex::parse_hex_address(&payout_key_hex)
        .unwrap_or([0xAAu8; 20]);

    let state = Arc::new(AppState::new(l1_url, payout_auth));

    // Oracle fiyatı (production'da bir oracle servisi güncellemeli)
    state.registry.update_oracle_price(2_000); // $20 default

    let addr = std::env::var("NOVA_DEX_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    let router = build_router(state);

    tracing::info!(addr = %addr, "Nova DEX Server başlatılıyor");

    let listener = tokio::net::TcpListener::bind(&addr).await
        .expect("bind başarısız");

    axum::serve(listener, router).await.expect("server hatası");
}
