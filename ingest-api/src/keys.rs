//! API-key minting, hashing, and tenant resolution backed by ClickHouse.
//!
//! A plaintext key (`ik_<hex>`) is shown exactly once at mint time; only its
//! SHA-256 hash is stored. Lookups are cached in-process (60s) so the hot
//! ingest path doesn't hit ClickHouse per request — a revoke therefore takes
//! effect within the cache TTL.

use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::ch::Ch;

pub fn hash_key(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    hex::encode(h.finalize())
}

/// Generate a fresh opaque key: `ik_` + 32 random bytes, hex-encoded.
pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("ik_{}", hex::encode(bytes))
}

fn now_millis() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

#[derive(Clone)]
pub struct KeyStore {
    ch: Ch,
    /// hash → resolved tenant (`None` = known-bad, cached to absorb probes).
    cache: moka::future::Cache<String, Option<String>>,
}

impl KeyStore {
    pub fn new(ch: Ch) -> Self {
        KeyStore {
            ch,
            cache: moka::future::Cache::builder()
                .max_capacity(10_000)
                .time_to_live(std::time::Duration::from_secs(60))
                .build(),
        }
    }

    /// Resolve a presented plaintext key to its tenant, or `None` if unknown /
    /// inactive. Cached for 60s.
    pub async fn tenant_for(&self, key: &str) -> Option<String> {
        let hash = hash_key(key);
        if let Some(hit) = self.cache.get(&hash).await {
            return hit;
        }
        let resolved = self.lookup(&hash).await;
        self.cache.insert(hash, resolved.clone()).await;
        resolved
    }

    async fn lookup(&self, hash: &str) -> Option<String> {
        let sql = format!(
            "SELECT tenant, active FROM ingest.ingest_keys FINAL WHERE key_hash = '{}'",
            esc(hash)
        );
        let rows = self.ch.query_rows(&sql).await.unwrap_or_default();
        let row = rows.into_iter().next()?;
        let active = row.get("active").and_then(|v| v.as_u64()).unwrap_or(0);
        if active == 1 {
            row.get("tenant")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Mint a new key for `tenant`. Returns the plaintext key (shown once) and
    /// its row id.
    pub async fn mint(&self, tenant: &str, label: &str) -> Result<(String, String), String> {
        let key = generate_key();
        let hash = hash_key(&key);
        let id = uuid::Uuid::new_v4().to_string();
        let version = now_millis();
        let sql = format!(
            "INSERT INTO ingest.ingest_keys (id, key_hash, tenant, label, scopes, active, version) VALUES ('{}','{}','{}','{}','events:write',1,{})",
            esc(&id), esc(&hash), esc(tenant), esc(label), version
        );
        self.ch.execute(&sql).await?;
        Ok((key, id))
    }

    /// Revoke a key by id (writes a higher-version tombstone row).
    pub async fn revoke(&self, id: &str) -> Result<bool, String> {
        // Look up the existing row's hash + tenant so the tombstone collapses
        // onto the same ReplacingMergeTree key.
        let sql = format!(
            "SELECT key_hash, tenant, label FROM ingest.ingest_keys FINAL WHERE id = '{}'",
            esc(id)
        );
        let rows = self.ch.query_rows(&sql).await?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(false);
        };
        let hash = row.get("key_hash").and_then(|v| v.as_str()).unwrap_or("");
        let tenant = row.get("tenant").and_then(|v| v.as_str()).unwrap_or("");
        let label = row.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let version = now_millis();
        let ins = format!(
            "INSERT INTO ingest.ingest_keys (id, key_hash, tenant, label, scopes, active, version, revoked_at) VALUES ('{}','{}','{}','{}','events:write',0,{},now())",
            esc(id), esc(hash), esc(tenant), esc(label), version
        );
        self.ch.execute(&ins).await?;
        self.cache.invalidate(hash).await;
        Ok(true)
    }

    /// List keys (no plaintext — only metadata).
    pub async fn list(&self) -> Result<Vec<serde_json::Value>, String> {
        self.ch
            .query_rows(
                "SELECT id, tenant, label, scopes, active, created_at, revoked_at \
                 FROM ingest.ingest_keys FINAL ORDER BY created_at DESC",
            )
            .await
    }
}

/// Escape a single-quoted string literal for inline ClickHouse SQL. Values here
/// are server-generated (hashes, uuids) or short admin-supplied labels.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
