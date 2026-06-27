//! HTTP surface: health, event ingest, and admin key management.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use redis::aio::ConnectionManager;
use serde::Deserialize;

use crate::drain::STREAM;
use crate::keys::KeyStore;

/// Max events accepted in a single request (producers should batch ≤ this).
const MAX_BATCH: usize = 1000;
/// Approximate stream cap so a ClickHouse outage drops oldest, not grows forever.
const STREAM_MAXLEN: usize = 5_000_000;

#[derive(Clone)]
pub struct State {
    pub keys: KeyStore,
    pub valkey: ConnectionManager,
    pub admin_token: String,
}

pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

// ── ingest ────────────────────────────────────────────────────────────────────

fn bearer_or_apikey(req: &HttpRequest) -> Option<String> {
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let auth = req.headers().get("authorization")?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

/// Parse a request body into a list of compact JSON event strings. Accepts a
/// JSON array, a single JSON object, or newline-delimited JSON.
fn parse_events(body: &str) -> Vec<String> {
    let body = body.trim();
    if body.is_empty() {
        return Vec::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        return match v {
            serde_json::Value::Array(items) => items
                .into_iter()
                .filter(|i| i.is_object())
                .map(|i| i.to_string())
                .collect(),
            serde_json::Value::Object(_) => vec![v.to_string()],
            _ => Vec::new(),
        };
    }
    // Fall back to NDJSON.
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.is_object())
        .map(|v| v.to_string())
        .collect()
}

pub async fn ingest(
    req: HttpRequest,
    state: web::Data<State>,
    body: web::Bytes,
) -> impl Responder {
    let Some(key) = bearer_or_apikey(&req) else {
        return HttpResponse::Unauthorized().json(serde_json::json!({ "error": "missing api key" }));
    };
    let Some(tenant) = state.keys.tenant_for(&key).await else {
        return HttpResponse::Unauthorized().json(serde_json::json!({ "error": "invalid api key" }));
    };

    let body = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return HttpResponse::BadRequest().json(serde_json::json!({ "error": "body not utf-8" }))
        }
    };
    let events = parse_events(body);
    if events.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "no events" }));
    }
    if events.len() > MAX_BATCH {
        return HttpResponse::PayloadTooLarge()
            .json(serde_json::json!({ "error": "too many events", "max": MAX_BATCH }));
    }

    let mut cm = state.valkey.clone();
    let mut pipe = redis::pipe();
    for ev in &events {
        pipe.cmd("XADD")
            .arg(STREAM)
            .arg("MAXLEN")
            .arg("~")
            .arg(STREAM_MAXLEN)
            .arg("*")
            .arg("tenant")
            .arg(&tenant)
            .arg("data")
            .arg(ev)
            .ignore();
    }
    match pipe.query_async::<()>(&mut cm).await {
        Ok(()) => HttpResponse::Accepted()
            .json(serde_json::json!({ "accepted": events.len() })),
        Err(e) => {
            log::warn!("xadd failed for tenant '{tenant}': {e}");
            HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "queue unavailable" }))
        }
    }
}

// ── admin ───────────────────────────────────────────────────────────────────

fn admin_ok(req: &HttpRequest, state: &State) -> bool {
    if state.admin_token.is_empty() {
        return false; // never allow admin ops without a configured token
    }
    let presented = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            req.headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|a| a.strip_prefix("Bearer ").map(|s| s.to_string()))
        });
    presented.as_deref() == Some(state.admin_token.as_str())
}

#[derive(Deserialize)]
pub struct MintReq {
    pub tenant: String,
    #[serde(default)]
    pub label: String,
}

pub async fn mint_key(
    req: HttpRequest,
    state: web::Data<State>,
    body: web::Json<MintReq>,
) -> impl Responder {
    if !admin_ok(&req, &state) {
        return HttpResponse::Unauthorized().json(serde_json::json!({ "error": "admin only" }));
    }
    let tenant = body.tenant.trim();
    if tenant.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "tenant required" }));
    }
    match state.keys.mint(tenant, body.label.trim()).await {
        Ok((key, id)) => HttpResponse::Ok().json(serde_json::json!({
            "id": id,
            "tenant": tenant,
            "key": key,
            "note": "store this key now — it is not recoverable",
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

pub async fn list_keys(req: HttpRequest, state: web::Data<State>) -> impl Responder {
    if !admin_ok(&req, &state) {
        return HttpResponse::Unauthorized().json(serde_json::json!({ "error": "admin only" }));
    }
    match state.keys.list().await {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

pub async fn revoke_key(
    req: HttpRequest,
    state: web::Data<State>,
    id: web::Path<String>,
) -> impl Responder {
    if !admin_ok(&req, &state) {
        return HttpResponse::Unauthorized().json(serde_json::json!({ "error": "admin only" }));
    }
    match state.keys.revoke(&id).await {
        Ok(found) => HttpResponse::Ok().json(serde_json::json!({ "revoked": found })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}
