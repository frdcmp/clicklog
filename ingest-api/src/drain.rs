//! Background drain: Valkey `ingest:events` stream → per-tenant ClickHouse.
//!
//! Runs as a spawned task inside the ingest-api process. Reads via consumer
//! group `ingest`, groups a batch by tenant, ensures each tenant's
//! `events` table exists, inserts (`JSONEachRow`), then `XACK`s. On a
//! ClickHouse error the batch is left unacked (redelivered) and the loop backs
//! off, so a CH outage buffers in Valkey rather than losing events.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::AsyncCommands;

use crate::ch::{Ch, EVENTS_DDL};

pub const STREAM: &str = "ingest:events";
const GROUP: &str = "ingest";
const BATCH: usize = 5000;
const BLOCK_MS: usize = 2000;

pub async fn run(mut cm: ConnectionManager, ch: Ch) {
    let consumer = std::env::var("HOSTNAME").unwrap_or_else(|_| "ingest-api".to_string());

    // Idempotent consumer-group create (ignore BUSYGROUP if it already exists).
    let _: Result<String, _> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(STREAM)
        .arg(GROUP)
        .arg("$")
        .arg("MKSTREAM")
        .query_async(&mut cm)
        .await;

    log::info!("🪵 drain '{consumer}' reading {STREAM} → clickhouse");

    let mut ensured: HashSet<String> = HashSet::new();

    loop {
        let opts = StreamReadOptions::default()
            .group(GROUP, &consumer)
            .count(BATCH)
            .block(BLOCK_MS);
        let reply: StreamReadReply = match cm.xread_options(&[STREAM], &[">"], &opts).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("xreadgroup failed: {e}; retrying in 1s");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        let mut ids: Vec<String> = Vec::new();
        // tenant → accumulated NDJSON
        let mut by_tenant: HashMap<String, String> = HashMap::new();
        for skey in &reply.keys {
            for entry in &skey.ids {
                ids.push(entry.id.clone());
                let tenant = entry.get::<String>("tenant").unwrap_or_default();
                let data = entry.get::<String>("data").unwrap_or_default();
                if tenant.is_empty() || data.is_empty() {
                    continue;
                }
                let buf = by_tenant.entry(tenant).or_default();
                buf.push_str(&data);
                buf.push('\n');
            }
        }

        if ids.is_empty() {
            continue; // BLOCK timed out, nothing new
        }

        // Insert each tenant's batch. If any fails, leave the WHOLE batch
        // unacked for redelivery (at-least-once; ClickHouse insert is the only
        // non-idempotent step, accepted trade-off for a logging pipeline).
        let mut ok = true;
        for (tenant, ndjson) in &by_tenant {
            if !ensured.contains(tenant) {
                match ensure_tenant(&ch, tenant).await {
                    Ok(()) => {
                        ensured.insert(tenant.clone());
                    }
                    Err(e) => {
                        log::warn!("ensure tenant '{tenant}' failed: {e}");
                        ok = false;
                        break;
                    }
                }
            }
            if let Err(e) = ch.insert_jsoneachrow(tenant, "events", ndjson.clone()).await {
                log::warn!("insert into '{tenant}'.events failed: {e}");
                ok = false;
                break;
            }
        }

        if ok {
            let _: Result<i64, _> = cm.xack(STREAM, GROUP, &ids).await;
        } else {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn ensure_tenant(ch: &Ch, tenant: &str) -> Result<(), String> {
    ch.execute(&format!("CREATE DATABASE IF NOT EXISTS `{tenant}`"))
        .await?;
    ch.execute_db(tenant, EVENTS_DDL).await
}
