//! ingest-api — shared telemetry ingest gateway for the frdcmp stacks.
//!
//! Apps POST batches of events to `/v1/events` with an API key; the key resolves
//! to a tenant, events are buffered on the local Valkey `ingest:events` stream,
//! and a background task drains them into each tenant's ClickHouse `events`
//! table. Storage credentials never leave this host — apps only hold a key.

mod ch;
mod drain;
mod handlers;
mod keys;

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

    let state = State {
        keys: KeyStore::new(ch.clone()),
        valkey: cm,
        admin_token: std::env::var("INGEST_ADMIN_TOKEN").unwrap_or_default(),
    };
    if state.admin_token.is_empty() {
        log::warn!("INGEST_ADMIN_TOKEN is empty — admin key endpoints are disabled");
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
            .route("/v1/admin/keys", web::post().to(handlers::mint_key))
            .route("/v1/admin/keys", web::get().to(handlers::list_keys))
            .route("/v1/admin/keys/{id}", web::delete().to(handlers::revoke_key))
    })
    .bind(bind)?
    .run()
    .await
}
