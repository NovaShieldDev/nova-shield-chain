//! MEV Koruması handler'ları
//!
//! ## Güvenlik Düzeltmeleri
//!
//! - **NOVA-RED-DEX-AUTH-001**: commit ve reveal handler'ları imza doğrulaması yapıyor.
//! - **NOVA-RED-DEX-MEV-002**: Direkt swap endpoint'i YOK — tüm swap'lar MEV akışından geçmeli.
//! - **NOVA-RED-DEX-MEV-005**: reveal() artık swap'ı execute ediyor.
//! - **NOVA-RED-DEX-MEV-006**: commitment_hash sender adresini içeriyor.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use nova_dex::{
    MevProtectedMempool, SignedRequest, SwapIntent,
    parse_hex_address, verify_signed_request,
};
use serde::{Deserialize, Serialize};
use crate::SharedState;

// ─── DTO'lar ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct CommitBody {
    /// intent_hash (32 byte hex) — intent'in hash'i (henüz açıklanmamış)
    pub intent_hash: String,
    /// Commit yapan adres
    pub sender: String,
}

#[derive(Serialize)]
pub struct CommitResponse {
    pub slot_id: u64,
    pub commitment_hash: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RevealBody {
    pub sender: String,
    pub pool_id: String,
    pub zero_for_one: bool,
    pub amount_in: String,
    pub min_amount_out: String,
    pub nonce: String,
}

#[derive(Serialize)]
pub struct RevealResponse {
    pub ok: bool,
    pub amount_out: String,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// POST /v1/mev/commit — Intent commit et (imza gerekli)
///
/// NOVA-RED-DEX-AUTH-001: İmza doğrulaması.
/// NOVA-RED-DEX-MEV-006: commitment_hash = BLAKE3(sender || intent_hash).
pub async fn commit(
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<CommitBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let sender = match parse_hex_address(&req.body.sender) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz sender adresi").into_response(),
    };

    // NOVA-RED-DEX-AUTH-001: İmza doğrula
    if let Err(e) = verify_signed_request("mev_commit", &body_json, &req.public_key, &req.signature, &sender) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let intent_hash_bytes = match hex::decode(req.body.intent_hash.strip_prefix("0x").unwrap_or(&req.body.intent_hash)) {
        Ok(b) if b.len() == 32 => { let mut arr = [0u8; 32]; arr.copy_from_slice(&b); arr }
        _ => return (StatusCode::BAD_REQUEST, "geçersiz intent_hash (32 byte hex gerekli)").into_response(),
    };

    match state.mempool.submit_commit(intent_hash_bytes, sender, now_ms()) {
        Ok(slot_id) => {
            // commitment_hash'i hesapla ve döndür
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"nova-mev-commit-v1");
            hasher.update(&sender);
            hasher.update(&intent_hash_bytes);
            let commitment_hash = hex::encode(hasher.finalize().as_bytes());
            Json(CommitResponse { slot_id, commitment_hash }).into_response()
        }
        Err(e) => (StatusCode::CONFLICT, format!("{e}")).into_response(),
    }
}

/// POST /v1/mev/reveal — Intent reveal et ve swap execute et (imza gerekli)
///
/// NOVA-RED-DEX-MEV-005 düzeltmesi: Swap artık burada execute ediliyor.
/// NOVA-RED-DEX-AUTH-001: İmza doğrulaması zorunlu.
pub async fn reveal(
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<RevealBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let sender = match parse_hex_address(&req.body.sender) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz sender adresi").into_response(),
    };

    // NOVA-RED-DEX-AUTH-001: İmza doğrula
    if let Err(e) = verify_signed_request("mev_reveal", &body_json, &req.public_key, &req.signature, &sender) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    let amount_in: u128 = match req.body.amount_in.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "geçersiz amount_in").into_response(),
    };
    let min_amount_out: u128 = req.body.min_amount_out.parse().unwrap_or(0);

    let nonce_bytes = match hex::decode(req.body.nonce.strip_prefix("0x").unwrap_or(&req.body.nonce)) {
        Ok(b) if b.len() == 32 => { let mut arr = [0u8; 32]; arr.copy_from_slice(&b); arr }
        _ => return (StatusCode::BAD_REQUEST, "geçersiz nonce (32 byte hex gerekli)").into_response(),
    };

    let intent = SwapIntent {
        sender,
        pool_id: req.body.pool_id.clone(),
        zero_for_one: req.body.zero_for_one,
        amount_in,
        min_amount_out,
        nonce: nonce_bytes,
    };

    // NOVA-RED-DEX-MEV-005: reveal_and_swap swap'ı execute eder
    match state.mempool.reveal_and_swap(intent, &state.registry, now_ms()) {
        Ok(amount_out) => Json(RevealResponse { ok: true, amount_out: amount_out.to_string() }).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    }
}
