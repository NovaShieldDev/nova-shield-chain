//! Payout handler'ları
//!
//! ## NOVA-RED-DEX-PAYOUT-001 Düzeltmesi
//!
//! `process` handler'ı imza doğrulaması yapıyor ve yalnızca yetkili
//! payout_authority adresinin çağırmasına izin veriyor.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use nova_dex::{SignedRequest, parse_hex_address, verify_signed_request, address_to_hex};
use serde::{Deserialize, Serialize};
use crate::SharedState;

// ─── DTO'lar ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessPayoutsBody {
    /// Payout yetkisi adresi (hex)
    pub caller: String,
}

#[derive(Serialize)]
pub struct CreatorDistribution {
    pub address: String,
    pub amount_nNOVA: String,
}

#[derive(Serialize)]
pub struct ProcessPayoutsResponse {
    /// Creator'lara dağıtılan toplam (nNOVA)
    pub total_distributed: String,
    /// Treasury'ye ayrılan toplam (nNOVA)
    pub treasury_total: String,
    /// İşlenen pool sayısı
    pub pools_processed: usize,
    /// Creator bazlı dağılım detayı
    pub distributions: Vec<CreatorDistribution>,
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// POST /v1/payouts/process — Ödülleri dağıt (imza + yetki gerekli)
///
/// NOVA-RED-DEX-PAYOUT-001: Yalnızca `payout_authority` çağırabilir.
/// İmza doğrulaması zorunlu.
pub async fn process(
    State(state): State<SharedState>,
    Json(req): Json<SignedRequest<ProcessPayoutsBody>>,
) -> impl IntoResponse {
    let body_json = match serde_json::to_vec(&req.body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let caller = match parse_hex_address(&req.body.caller) {
        Some(a) => a,
        None => return (StatusCode::BAD_REQUEST, "geçersiz caller adresi").into_response(),
    };

    // NOVA-RED-DEX-AUTH-001: İmza doğrula
    if let Err(e) = verify_signed_request("process_payouts", &body_json, &req.public_key, &req.signature, &caller) {
        return (StatusCode::UNAUTHORIZED, format!("{e}")).into_response();
    }

    // NOVA-RED-DEX-PAYOUT-001: Yetki kontrolü — registry içinde yapılıyor
    match state.registry.process_payouts(caller) {
        Ok(result) => {
            let distributions = result.creator_distributions
                .into_iter()
                .map(|(addr, amount)| CreatorDistribution {
                    address: address_to_hex(&addr),
                    amount_nNOVA: amount.to_string(),
                })
                .collect();

            Json(ProcessPayoutsResponse {
                total_distributed: result.total_distributed.to_string(),
                treasury_total: result.treasury_total.to_string(),
                pools_processed: result.pools_processed,
                distributions,
            }).into_response()
        },
        Err(e) => (StatusCode::FORBIDDEN, format!("{e}")).into_response(),
    }
}
