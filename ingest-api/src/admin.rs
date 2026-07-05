//! Dashboard admin surface (`/v1/admin/*`).
//!
//! Auth: `login` is public; everything else requires `auth::require_admin` (a
//! dashboard JWT). Single-admin model — any valid
//! token is fully authorized. Key CRUD lives in `handlers.rs`; this module adds
//! login/me/tenants plus the admin (cross-tenant) read endpoints, which reuse the
//! query builders in `read.rs`.

use std::collections::{BTreeMap, HashMap};

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{issue_token, require_admin};
use crate::handlers::State;
use crate::read;

// ── POST /v1/admin/login ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginReq {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
}

fn creds_match(a: &str, b: &str) -> bool {
    // Length-checked byte compare (see auth::eq_secret rationale).
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

pub async fn login(state: web::Data<State>, body: web::Json<LoginReq>) -> impl Responder {
    // Fail safe: with no signing secret or no seeded credentials, login is
    // disabled entirely — never accept blank/blank against unset env.
    if state.jwt_secret.is_empty() || state.admin_email.trim().is_empty() || state.admin_password.is_empty() {
        return HttpResponse::ServiceUnavailable()
            .json(json!({ "error": "auth not configured (set JWT_SECRET, ADMIN_EMAIL, ADMIN_PASSWORD)" }));
    }
    let email = body.email.trim();
    let ok = creds_match(email, state.admin_email.trim())
        && creds_match(&body.password, &state.admin_password);
    if !ok {
        return HttpResponse::Unauthorized().json(json!({ "error": "invalid credentials" }));
    }
    match issue_token(email, &state.jwt_secret, state.jwt_ttl_hours) {
        Ok(token) => HttpResponse::Ok().json(json!({
            "token": token,
            "email": email,
            "expires_in": state.jwt_ttl_hours * 3600,
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e })),
    }
}

// ── GET /v1/admin/me ──────────────────────────────────────────────────────────

pub async fn me(req: HttpRequest, state: web::Data<State>) -> impl Responder {
    match require_admin(&req, &state) {
        Ok(email) => HttpResponse::Ok().json(json!({ "email": email })),
        Err(r) => r,
    }
}

// ── GET /v1/admin/tenants ─────────────────────────────────────────────────────

/// List every known tenant with its key counts and whether it has an `events`
/// table yet. A tenant is "known" if it has a key OR has received events.
pub async fn tenants(req: HttpRequest, state: web::Data<State>) -> impl Responder {
    if let Err(r) = require_admin(&req, &state) {
        return r;
    }

    #[derive(Default)]
    struct Agg {
        keys: u64,
        active_keys: u64,
        has_events: bool,
    }
    let mut map: BTreeMap<String, Agg> = BTreeMap::new();

    match state.keys.list().await {
        Ok(rows) => {
            for r in rows {
                let Some(t) = r.get("tenant").and_then(|v| v.as_str()) else {
                    continue;
                };
                let active = r.get("active").and_then(|v| v.as_u64()).unwrap_or(0) == 1;
                let e = map.entry(t.to_string()).or_default();
                e.keys += 1;
                if active {
                    e.active_keys += 1;
                }
            }
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(json!({ "error": e }));
        }
    }

    for t in read::existing_event_tenants(&state).await {
        map.entry(t).or_default().has_events = true;
    }

    let out: Vec<_> = map
        .into_iter()
        .map(|(tenant, a)| {
            json!({
                "tenant": tenant,
                "keys": a.keys,
                "active_keys": a.active_keys,
                "has_events": a.has_events,
            })
        })
        .collect();
    HttpResponse::Ok().json(json!({ "tenants": out }))
}

// ── GET /v1/admin/events ──────────────────────────────────────────────────────
// `tenant` param: a concrete name, or `*`/`all`/omitted for every tenant.

pub async fn list_events(
    req: HttpRequest,
    state: web::Data<State>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    if let Err(r) = require_admin(&req, &state) {
        return r;
    }
    let q = qs.into_inner();
    let (tenants, cross) = match read::resolve_admin_tenants(&state, q.get("tenant")).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    match read::query_events(&state, &tenants, cross, &q).await {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => r,
    }
}

// ── GET /v1/admin/events/{event_id} ── (requires a concrete `tenant` param) ────

pub async fn get_event(
    req: HttpRequest,
    state: web::Data<State>,
    path: web::Path<String>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    if let Err(r) = require_admin(&req, &state) {
        return r;
    }
    let q = qs.into_inner();
    let tenant = match q.get("tenant").map(|s| s.trim()) {
        Some(t) if !t.is_empty() && t != "*" && t != "all" && read::safe_ident(t) => t.to_string(),
        _ => {
            return HttpResponse::BadRequest()
                .json(json!({ "error": "a concrete 'tenant' query param is required" }))
        }
    };
    match read::query_event(&state, &tenant, &path.into_inner()).await {
        Ok(ev) => HttpResponse::Ok().json(ev),
        Err(r) => r,
    }
}

// ── GET /v1/admin/stats ───────────────────────────────────────────────────────

pub async fn stats(
    req: HttpRequest,
    state: web::Data<State>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    if let Err(r) = require_admin(&req, &state) {
        return r;
    }
    let q = qs.into_inner();
    let (tenants, cross) = match read::resolve_admin_tenants(&state, q.get("tenant")).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    match read::query_stats(&state, &tenants, cross, &q).await {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => r,
    }
}
