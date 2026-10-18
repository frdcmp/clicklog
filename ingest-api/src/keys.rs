//! API-key minting, hashing, and tenant resolution backed by ClickHouse.
//!
//! A plaintext key (`ik_<hex>`) is shown exactly once at mint time; only its
//! SHA-256 hash is stored. A key is either a project key (one tenant, read +
//! write) or a reader key (tenant `*`, `events:read` only): one credential that
//! reads every tenant and can never ingest. Lookups are cached in-process (60s) so the hot
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

/// Tenant value of a reader key: it reads every tenant.
pub const ALL_TENANTS: &str = "*";
pub const SCOPE_READ: &str = "events:read";
pub const SCOPE_WRITE: &str = "events:write";
/// What a project key is minted with.
pub const PROJECT_SCOPES: &str = "events:read,events:write";

/// What a presented key resolves to.
#[derive(Clone, Debug)]
pub struct Grant {
    /// A tenant slug, or [`ALL_TENANTS`] for a reader key.
    pub tenant: String,
    scopes: Vec<String>,
}

impl Grant {
    fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn all_tenants(&self) -> bool {
        self.tenant == ALL_TENANTS
    }

    /// Writing needs `events:write` and one concrete tenant.
    pub fn can_write(&self) -> bool {
        self.has(SCOPE_WRITE) && !self.all_tenants()
    }

    /// A write scope implies read: the first keys were minted `events:write`
    /// alone, before the read API existed, and have always been able to read.
    pub fn can_read(&self) -> bool {
        self.has(SCOPE_READ) || self.has(SCOPE_WRITE)
    }
}

fn now_millis() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

#[derive(Clone)]
pub struct KeyStore {
    ch: Ch,
    /// hash → resolved grant (`None` = known-bad, cached to absorb probes).
    cache: moka::future::Cache<String, Option<Grant>>,
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

    /// Resolve a presented plaintext key to its grant, or `None` if unknown /
    /// inactive. Cached for 60s.
    pub async fn grant_for(&self, key: &str) -> Option<Grant> {
        let hash = hash_key(key);
        if let Some(hit) = self.cache.get(&hash).await {
            return hit;
        }
        let resolved = self.lookup(&hash).await;
        self.cache.insert(hash, resolved.clone()).await;
        resolved
    }

    async fn lookup(&self, hash: &str) -> Option<Grant> {
        let sql = format!(
            "SELECT tenant, scopes, active FROM ingest.ingest_keys FINAL WHERE key_hash = '{}'",
            esc(hash)
        );
        let rows = self.ch.query_rows(&sql).await.unwrap_or_default();
        let row = rows.into_iter().next()?;
        let active = row.get("active").and_then(|v| v.as_u64()).unwrap_or(0);
        if active != 1 {
            return None;
        }
        let tenant = row.get("tenant")?.as_str()?.to_string();
        let scopes = row
            .get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(Grant { tenant, scopes })
    }

    /// Mint a new key for `tenant` with `scopes` ([`PROJECT_SCOPES`] for a
    /// project key, [`SCOPE_READ`] with [`ALL_TENANTS`] for a reader key).
    /// Returns the plaintext key (shown once) and its row id.
    pub async fn mint(
        &self,
        tenant: &str,
        label: &str,
        scopes: &str,
    ) -> Result<(String, String), String> {
        let key = generate_key();
        let hash = hash_key(&key);
        let id = uuid::Uuid::new_v4().to_string();
        let version = now_millis();
        let sql = format!(
            "INSERT INTO ingest.ingest_keys (id, key_hash, tenant, label, scopes, active, version) VALUES ('{}','{}','{}','{}','{}',1,{})",
            esc(&id), esc(&hash), esc(tenant), esc(label), esc(scopes), version
        );
        self.ch.execute(&sql).await?;
        Ok((key, id))
    }

    /// Revoke a key by id (writes a higher-version tombstone row).
    pub async fn revoke(&self, id: &str) -> Result<bool, String> {
        // Look up the existing row's hash + tenant so the tombstone collapses
        // onto the same ReplacingMergeTree key.
        let sql = format!(
            "SELECT key_hash, tenant, label, scopes FROM ingest.ingest_keys FINAL WHERE id = '{}'",
            esc(id)
        );
        let rows = self.ch.query_rows(&sql).await?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(false);
        };
        let hash = row.get("key_hash").and_then(|v| v.as_str()).unwrap_or("");
        let tenant = row.get("tenant").and_then(|v| v.as_str()).unwrap_or("");
        let label = row.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let scopes = row.get("scopes").and_then(|v| v.as_str()).unwrap_or("");
        let version = now_millis();
        let ins = format!(
            "INSERT INTO ingest.ingest_keys (id, key_hash, tenant, label, scopes, active, version, revoked_at) VALUES ('{}','{}','{}','{}','{}',0,{},now())",
            esc(id), esc(hash), esc(tenant), esc(label), esc(scopes), version
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
