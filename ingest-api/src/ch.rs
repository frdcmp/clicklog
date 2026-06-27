//! Minimal ClickHouse HTTP client (admin credentials, co-located server).
//!
//! Talks to the shared ClickHouse over the internal docker network
//! (`clickhouse:8123`) as the bootstrap admin user — it provisions per-tenant
//! databases/tables on demand and inserts event batches, so it needs access
//! management. Plain HTTP only; never crosses the overlay.

use reqwest::Client;

#[derive(Clone)]
pub struct Ch {
    client: Client,
    base: String,
    user: String,
    pass: String,
}

impl Ch {
    pub fn from_env() -> Self {
        Ch {
            client: Client::new(),
            base: std::env::var("CLICKHOUSE_URL")
                .unwrap_or_else(|_| "http://clickhouse:8123".to_string()),
            user: std::env::var("CLICKHOUSE_ADMIN_USER").unwrap_or_else(|_| "default".to_string()),
            pass: std::env::var("CLICKHOUSE_ADMIN_PASSWORD").unwrap_or_default(),
        }
    }

    /// Run a statement (DDL/INSERT-VALUES/etc.) with no row payload.
    pub async fn execute(&self, sql: &str) -> Result<(), String> {
        let res = self
            .client
            .post(&self.base)
            .basic_auth(&self.user, Some(&self.pass))
            .body(sql.to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.check(res).await.map(|_| ())
    }

    /// Run a statement scoped to a specific database (`?database=`), so
    /// unqualified DDL like `CREATE TABLE events` lands in that tenant's DB.
    pub async fn execute_db(&self, db: &str, sql: &str) -> Result<(), String> {
        let url = format!("{}/?database={}", self.base, urlencoding::encode(db));
        let res = self
            .client
            .post(url)
            .basic_auth(&self.user, Some(&self.pass))
            .body(sql.to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.check(res).await.map(|_| ())
    }

    /// Insert NDJSON rows into `{db}.{table}` via the HTTP `JSONEachRow` format.
    /// Unknown fields are skipped and timestamps parse best-effort so producers
    /// can evolve their event shape without breaking the insert.
    pub async fn insert_jsoneachrow(
        &self,
        db: &str,
        table: &str,
        ndjson: String,
    ) -> Result<(), String> {
        let query = format!("INSERT INTO {table} FORMAT JSONEachRow");
        let url = format!(
            "{}/?database={}&query={}&input_format_skip_unknown_fields=1&date_time_input_format=best_effort&async_insert=1&wait_for_async_insert=0",
            self.base,
            urlencoding::encode(db),
            urlencoding::encode(&query),
        );
        let res = self
            .client
            .post(url)
            .basic_auth(&self.user, Some(&self.pass))
            .body(ndjson)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.check(res).await.map(|_| ())
    }

    /// Run a SELECT, returning one parsed JSON object per result row
    /// (`FORMAT JSONEachRow` appended automatically).
    pub async fn query_rows(&self, sql: &str) -> Result<Vec<serde_json::Value>, String> {
        let full = format!("{sql} FORMAT JSONEachRow");
        let res = self
            .client
            .post(&self.base)
            .basic_auth(&self.user, Some(&self.pass))
            .body(full)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body = self.check(res).await?;
        let mut rows = Vec::new();
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            rows.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
        }
        Ok(rows)
    }

    async fn check(&self, res: reqwest::Response) -> Result<String, String> {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if status.is_success() {
            Ok(body)
        } else {
            Err(format!("clickhouse {status}: {body}"))
        }
    }
}

/// Canonical per-tenant events table. Created in each tenant's own database the
/// first time the drain loop sees rows for it (idempotent).
pub const EVENTS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    event_id      UUID DEFAULT generateUUIDv4(),
    ts            DateTime64(3) DEFAULT now64(3),
    received_at   DateTime64(3) DEFAULT now64(3),
    source        LowCardinality(String) DEFAULT '',
    category      LowCardinality(String) DEFAULT '',
    event_type    LowCardinality(String) DEFAULT '',
    severity      LowCardinality(String) DEFAULT 'info',
    user_id       String DEFAULT '',
    user_email    String DEFAULT '',
    session_id    String DEFAULT '',
    request_id    String DEFAULT '',
    entity_type   LowCardinality(String) DEFAULT '',
    entity_id     String DEFAULT '',
    message       String DEFAULT '',
    error_code    String DEFAULT '',
    model         LowCardinality(String) DEFAULT '',
    tokens_input  UInt32 DEFAULT 0,
    tokens_output UInt32 DEFAULT 0,
    duration_ms   UInt32 DEFAULT 0,
    http_status   UInt16 DEFAULT 0,
    route         String DEFAULT '',
    app_version   String DEFAULT '',
    server        String DEFAULT '',
    ip            String DEFAULT '',
    user_agent    String DEFAULT '',
    attributes    String DEFAULT ''
) ENGINE = MergeTree
PARTITION BY toYYYYMM(ts)
ORDER BY (category, event_type, ts)
TTL toDateTime(ts) + INTERVAL 180 DAY DELETE
"#;

/// Control-plane table for API keys. Lives in the `ingest` database. Keys are
/// stored as SHA-256 hashes; revocation/`last_used` are modelled as new
/// versions collapsed by `ReplacingMergeTree` (read with `FINAL`).
pub const KEYS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS ingest.ingest_keys (
    id          String,
    key_hash    String,
    tenant      LowCardinality(String),
    label       String DEFAULT '',
    scopes      String DEFAULT 'events:write',
    active      UInt8 DEFAULT 1,
    version     UInt64,
    created_at  DateTime DEFAULT now(),
    revoked_at  DateTime DEFAULT toDateTime(0)
) ENGINE = ReplacingMergeTree(version)
ORDER BY key_hash
"#;
