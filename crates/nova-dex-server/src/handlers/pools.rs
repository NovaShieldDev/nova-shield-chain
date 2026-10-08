//! Pool handler'ları
//!
//! ## NOVA-RED-DEX-AUTH-001 Düzeltmesi
//!
//! Tüm mutating işlemler `SignedRequest<T>` wrapper'ı ile imza doğrulaması yapıyor.
//! `create_pool` handler'ı:
//! 1. `SignedRequest` body parse eder
//! 2. `verify_signed_request()` ile ed25519 imzayı doğrular
//! 3. Body'deki creator adresi imzalayan adresle eşleşmeli
//!
//! ## NOVA-RED-DEX-POOL-001 Düzeltmesi
//!
//! `nova_price_cents` artık `CreatePoolRequest`'te YOK.
//! Sunucu oracle'dan alır.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use nova_dex::{
    DexError, LockDuration, SignedRequest,
    parse_hex_address, verify_signed_request,
};
use serde::{Deserialize, Serialize};
use crate::SharedState;

// ─── DTO'lar ──────────────────────────────────────────────────────────────────

/// Pool oluşturma isteği — NOVA-RED-DEX-POOL-001: nova_price_cents YOK
#[derive(Debug, Deserialize, Serialize)]
pub struct CreatePoolBody {
    pub creator: String,
    pub asset_a: String,
    pub asset_b: String,
    pub initial_a: String, // u128 string
    pub initial_b: String,
    pub lock_duration: LockDurationDto,
    pub creation_fee_paid: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockDurationDto {
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
    OneYear,
    Permanent,
}

impl From<LockDurationDto> for LockDuration {
    fn from(d: LockDurationDto) -> Self {
        match d {
            LockDurationDto::OneDay => LockDuration::OneDay,
            LockDurationDto::ThreeDays => LockDuration::ThreeDays,
            LockDurationDto::OneWeek => LockDuration::OneWeek,
            LockDurationDto::OneMonth => LockDuration::OneMonth,
            LockDurationDto::OneYear => LockDuration::OneYear,
            LockDurationDto::Permanent => LockDuration::Permanent,
        }
    }
}

#[derive(Serialize)]
pub struct CreatePoolResponse {
    pub pool_id: String,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// POST /v1/pools — Pool oluştur (imza gerekli)
///
/// NOVA-RED-DEX-AUTH-001: İmza doğrulaması zorunlu.
/// NOVA-RED-DEX-POOL-001: nova_price_cents oracle'dan alınır.
pub async fn create_pool(
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<CreatePoolBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let creator = match parse_hex_address(&req.body.creator) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz creator adresi").into_response(),
    };

    // NOVA-RED-DEX-AUTH-001: İmza doğrula
    if let Err(e) = verify_signed_request("create_pool", &body_json, &req.public_key, &req.signature, &creator) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let initial_a: u128 = match req.body.initial_a.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "geçersiz initial_a").into_response(),
    };
    let initial_b: u128 = match req.body.initial_b.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "geçersiz initial_b").into_response(),
    };
    let creation_fee: u128 = match req.body.creation_fee_paid.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "geçersiz creation_fee_paid").into_response(),
    };
    let lock_duration = LockDuration::from(req.body.lock_duration);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    match state.registry.create_pool(
        creator, req.body.asset_a, req.body.asset_b,
        initial_a, initial_b, lock_duration, creation_fee, now_ms,
    ) {
        Ok(pool_id) => Json(CreatePoolResponse { pool_id }).into_response(),
        Err(DexError::InsufficientCreationFee { required, paid }) =>
            (StatusCode::PAYMENT_REQUIRED, format!("ücret yetersiz: gerekli {required} nNOVA, ödenen {paid}")).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    }
}

/// GET /v1/pools — Tüm pool'ları listele
pub async fn list_pools(State(state): State<SharedState>) -> impl IntoResponse {
    Json(state.registry.list_pools())
}

/// GET /v1/pools/:id — Tek pool
pub async fn get_pool(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    match state.registry.get_pool(&id) {
        Ok(pool) => Json(pool).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "pool bulunamadı").into_response(),
    }
}
