//! ingest-api — shared telemetry ingest gateway for the frdcmp stacks.
//!
//! Apps POST batches of events to `/v1/events` with an API key; the key resolves
//! to a tenant, events are buffered on the local Valkey `ingest:events` stream,
//! and a background task drains them into each tenant's ClickHouse `events`
//! table. Storage credentials never leave this host — apps only hold a key.

mod admin;
mod auth;
mod ch;
mod drain;
mod handlers;
mod keys;
mod read;
mod schema;

use std::time::Duration;

use actix_web::{web, App, HttpServer};
use redis::aio::ConnectionManager;

use ch::{Ch, KEYS_DDL};
use handlers::State;
use keys::KeyStore;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let ch = Ch::from_env();

    // Bootstrap the control-plane schema. ClickHouse may start after us, so
    // retry until it answers.
    loop {
        let r = async {
            ch.execute("CREATE DATABASE IF NOT EXISTS ingest").await?;
            ch.execute(KEYS_DDL).await
        }
        .await;
        match r {
            Ok(()) => break,
            Err(e) => {
                log::warn!("clickhouse not ready ({e}); retrying in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    // Valkey connection (admin/default user — full access to the internal
    // ingest:events stream).
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let client = redis::Client::open(redis_url).expect("invalid REDIS_URL");
    let cm = ConnectionManager::new(client)
        .await
        .expect("redis connect failed");

    // Spawn the background drain (Valkey → ClickHouse).
    tokio::spawn(drain::run(cm.clone(), ch.clone()));

    // Dashboard admin auth (single seeded identity + HS256 JWT signing secret).
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_default();
    if jwt_secret.is_empty() {
        log::warn!("JWT_SECRET is empty — dashboard login (/v1/admin/login) is disabled");
    }
    let jwt_ttl_hours = std::env::var("JWT_TTL_HOURS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|h| *h > 0)
        .unwrap_or(24);

    let state = State {
        keys: KeyStore::new(ch.clone()),
        valkey: cm,
        ch: ch.clone(),
        jwt_secret,
        admin_email: std::env::var("ADMIN_EMAIL").unwrap_or_default(),
        admin_password: std::env::var("ADMIN_PASSWORD").unwrap_or_default(),
        jwt_ttl_hours,
    };
    if state.admin_email.trim().is_empty() || state.admin_password.is_empty() {
        log::warn!("ADMIN_EMAIL/ADMIN_PASSWORD unset — dashboard login is disabled");
    }
    let bind = std::env::var("INGEST_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    log::info!("📥 ingest-api listening on {bind}");

    let data = web::Data::new(state);
    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            // Generous body cap for batched event payloads.
            .app_data(web::PayloadConfig::new(8 * 1024 * 1024))
            .route("/health", web::get().to(handlers::health))
            .route("/v1/events", web::post().to(handlers::ingest))
            // Read API (tenant-scoped, key → tenant; read + write share one key).
            .route("/v1/events", web::get().to(read::list_events))
            .route("/v1/events/{event_id}", web::get().to(read::get_event))
            .route("/v1/stats", web::get().to(read::stats))
            .route("/v1/admin/keys", web::post().to(handlers::mint_key))
            .route("/v1/admin/keys", web::get().to(handlers::list_keys))
            .route("/v1/admin/keys/{id}", web::delete().to(handlers::revoke_key))
            // Dashboard admin surface (JWT; login is public).
            .route("/v1/admin/login", web::post().to(admin::login))
            .route("/v1/admin/me", web::get().to(admin::me))
            .route("/v1/admin/tenants", web::get().to(admin::tenants))
            .route("/v1/admin/events", web::get().to(admin::list_events))
            .route("/v1/admin/events/{event_id}", web::get().to(admin::get_event))
            .route("/v1/admin/stats", web::get().to(admin::stats))
    })
    .bind(bind)?
    .run()
    .await
}
