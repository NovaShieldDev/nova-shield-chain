//! Likidite handler'ları
//!
//! ## NOVA-RED-DEX-AUTH-001 Düzeltmesi
//!
//! Tüm mutating handler'lar imza doğrulaması yapıyor.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use nova_dex::{
    LockDuration, SignedRequest, parse_hex_address, verify_signed_request,
};
use serde::{Deserialize, Serialize};
use crate::SharedState;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─── Add Liquidity ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct AddLiquidityBody {
    pub provider: String,
    pub amount_a: String,
    pub amount_b: String,
}

#[derive(Serialize)]
pub struct AddLiquidityResponse {
    pub lp_minted: String,
    pub actual_a: String,
    pub actual_b: String,
}

/// POST /v1/pools/:id/liquidity — Likidite ekle (imza gerekli)
pub async fn add_liquidity(
    Path(pool_id): Path<String>,
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<AddLiquidityBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let provider = match parse_hex_address(&req.body.provider) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz provider").into_response(),
    };
    if let Err(e) = verify_signed_request("add_liquidity", &body_json, &req.public_key, &req.signature, &provider) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let amount_a: u128 = req.body.amount_a.parse().unwrap_or(0);
    let amount_b: u128 = req.body.amount_b.parse().unwrap_or(0);

    match state.registry.add_liquidity(&pool_id, provider, amount_a, amount_b, now_ms()) {
        Ok((lp, a, b)) => Json(AddLiquidityResponse {
            lp_minted: lp.to_string(),
            actual_a: a.to_string(),
            actual_b: b.to_string(),
        }).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    }
}

// ─── Remove Liquidity ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoveLiquidityBody {
    pub provider: String,
    pub lp_to_burn: String,
}

/// DELETE /v1/pools/:id/liquidity — Likidite çek (imza gerekli)
pub async fn remove_liquidity(
    Path(pool_id): Path<String>,
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<RemoveLiquidityBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let provider = match parse_hex_address(&req.body.provider) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz provider").into_response(),
    };
    if let Err(e) = verify_signed_request("remove_liquidity", &body_json, &req.public_key, &req.signature, &provider) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let lp_to_burn: u128 = req.body.lp_to_burn.parse().unwrap_or(0);

    match state.registry.remove_liquidity(&pool_id, provider, lp_to_burn, now_ms()) {
        Ok((a, b)) => Json(serde_json::json!({ "amount_a": a.to_string(), "amount_b": b.to_string() })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    }
}

// ─── Extend Lock ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtendLockBody {
    pub caller: String,
    pub new_duration: crate::handlers::pools::LockDurationDto,
}

/// POST /v1/pools/:id/lock/extend — Kilit uzat (imza gerekli)
pub async fn extend_lock(
    Path(pool_id): Path<String>,
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<ExtendLockBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let caller = match parse_hex_address(&req.body.caller) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz caller").into_response(),
    };
    if let Err(e) = verify_signed_request("extend_lock", &body_json, &req.public_key, &req.signature, &caller) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let duration = LockDuration::from(req.body.new_duration);
    match state.registry.extend_lock(&pool_id, caller, duration, now_ms()) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    }
}

// ─── Get Position ─────────────────────────────────────────────────────────────

/// GET /v1/pools/:id/lp/:address — LP pozisyonu sorgula
pub async fn get_position(
    Path((pool_id, address)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let addr = match parse_hex_address(&address) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz adres").into_response(),
    };
    let balance = state.registry.get_lp_position(&pool_id, &addr);
    Json(serde_json::json!({ "lp_balance": balance.to_string() })).into_response()
}
