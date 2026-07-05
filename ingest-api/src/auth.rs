//! JWT auth for the admin dashboard.
//!
//! A single admin identity is seeded from env (`ADMIN_EMAIL` / `ADMIN_PASSWORD`).
//! `POST /v1/admin/login` checks those credentials and issues an HS256 JWT; every
//! other `/v1/admin/*` route (except login) requires that JWT in
//! `Authorization: Bearer <jwt>`.
//!
//! This is intentionally a single-tenant admin model: any valid token is fully
//! authorized. There are no per-scope checks.

use actix_web::{HttpRequest, HttpResponse};
use jsonwebtoken::{
    decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};

use crate::handlers::State;

/// JWT claims. `sub` is the admin email; `exp` is a Unix timestamp (seconds).
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

/// Mint a signed token for `email`, valid for `ttl_hours`.
pub fn issue_token(email: &str, secret: &str, ttl_hours: i64) -> Result<String, String> {
    let exp = (chrono::Utc::now() + chrono::Duration::hours(ttl_hours)).timestamp();
    let claims = Claims {
        sub: email.to_string(),
        exp: exp.max(0) as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| e.to_string())
}

/// Verify a token's signature + expiry against `secret`.
pub fn verify_token(token: &str, secret: &str) -> Result<Claims, String> {
    let data: TokenData<Claims> = decode(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(data.claims)
}

/// Extract a `Bearer` token from the Authorization header, if present.
fn bearer(req: &HttpRequest) -> Option<String> {
    let auth = req.headers().get("authorization")?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

/// Authorize an `/v1/admin/*` request: requires a valid dashboard JWT in
/// `Authorization: Bearer`. Returns the admin email on success, or a 401
/// response to return.
pub fn require_admin(req: &HttpRequest, state: &State) -> Result<String, HttpResponse> {
    if !state.jwt_secret.is_empty() {
        if let Some(tok) = bearer(req) {
            if let Ok(claims) = verify_token(&tok, &state.jwt_secret) {
                return Ok(claims.sub);
            }
        }
    }
    Err(HttpResponse::Unauthorized().json(serde_json::json!({ "error": "admin auth required" })))
}
