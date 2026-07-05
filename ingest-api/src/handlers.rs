//! HTTP surface: health, event ingest, and admin key management.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use redis::aio::ConnectionManager;
use serde::Deserialize;

use crate::ch::Ch;
use crate::drain::STREAM;
use crate::keys::KeyStore;
use crate::schema::validate_event;

/// Max events accepted in a single request (producers should batch ≤ this).
const MAX_BATCH: usize = 1000;
/// Approximate stream cap so a ClickHouse outage drops oldest, not grows forever.
const STREAM_MAXLEN: usize = 5_000_000;
/// Cap how many per-event validation errors we echo back in one response.
const MAX_REPORTED_ERRORS: usize = 20;

#[derive(Clone)]
pub struct State {
    pub keys: KeyStore,
    pub valkey: ConnectionManager,
    pub ch: Ch,
    /// HS256 signing secret for dashboard JWTs.
    pub jwt_secret: String,
    /// Seeded single admin identity for `/v1/admin/login`.
    pub admin_email: String,
    pub admin_password: String,
    /// JWT lifetime in hours.
    pub jwt_ttl_hours: i64,
}

pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}

// ── ingest ────────────────────────────────────────────────────────────────────

pub(crate) fn bearer_or_apikey(req: &HttpRequest) -> Option<String> {
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

/// Parse a request body into a list of JSON values. Accepts a JSON array, a
/// single JSON object, or newline-delimited JSON. Strict: a malformed body is an
/// error, not a silently-dropped line (no fallback).
fn parse_events(body: &str) -> Result<Vec<serde_json::Value>, String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("empty body".to_string());
    }
    // Try a single JSON document first (array or object).
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::Array(items)) => return Ok(items),
        Ok(v @ serde_json::Value::Object(_)) => return Ok(vec![v]),
        Ok(_) => return Err("body must be a JSON object or an array of objects".to_string()),
        Err(_) => {} // fall through to NDJSON
    }
    // NDJSON: every non-blank line must be valid JSON.
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => out.push(v),
            Err(e) => return Err(format!("line {}: invalid JSON: {e}", i + 1)),
        }
    }
    if out.is_empty() {
        return Err("no events".to_string());
    }
    Ok(out)
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
    let values = match parse_events(body) {
        Ok(v) => v,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    if values.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "no events" }));
    }
    if values.len() > MAX_BATCH {
        return HttpResponse::PayloadTooLarge()
            .json(serde_json::json!({ "error": "too many events", "max": MAX_BATCH }));
    }

    // Enforce the event standard. ANY violation rejects the WHOLE batch — there
    // is no partial accept and no skip-unknown fallback. Errors are returned
    // synchronously so producers learn immediately what is off-standard.
    let mut details = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let errs = validate_event(v);
        if !errs.is_empty() {
            if details.len() < MAX_REPORTED_ERRORS {
                details.push(serde_json::json!({ "index": i, "errors": errs }));
            }
        }
    }
    if !details.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "validation failed — batch rejected",
            "rejected": values.len(),
            "details": details,
            "truncated": details.len() >= MAX_REPORTED_ERRORS,
        }));
    }

    let events: Vec<String> = values.iter().map(|v| v.to_string()).collect();
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
//
// Authorization for all key endpoints is handled by `auth::require_admin`,
// which requires a dashboard JWT (Authorization: Bearer). See auth.rs.

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
    if let Err(r) = crate::auth::require_admin(&req, &state) {
        return r;
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
    if let Err(r) = crate::auth::require_admin(&req, &state) {
        return r;
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
    if let Err(r) = crate::auth::require_admin(&req, &state) {
        return r;
    }
    match state.keys.revoke(&id).await {
        Ok(found) => HttpResponse::Ok().json(serde_json::json!({ "revoked": found })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}
