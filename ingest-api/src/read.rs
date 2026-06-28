//! Read API — tenant-scoped queries over each project's `events` table.
//!
//! Same auth as ingest: the API key resolves to ONE tenant, and every query is
//! hard-scoped to that tenant's `<tenant>.events` table. A key can never read
//! another tenant's data. Callers never send SQL — they send structured query
//! params; this module builds parameter-bound, read-only ClickHouse queries from
//! a fixed allowlist of columns. Filter VALUES are bound (`{p:Type}`); column
//! and dimension NAMES come only from the allowlists below.

use std::collections::HashMap;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::{json, Value};

use crate::handlers::{bearer_or_apikey, State};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const STATS_MAX_ROWS: usize = 5000;

/// Exact-match string filters: query-param name == column name.
const STRING_FILTERS: &[&str] = &[
    "category",
    "event_type",
    "severity",
    "source",
    "model",
    "user_id",
    "session_id",
    "request_id",
    "entity_type",
    "entity_id",
    "error_code",
    "route",
    "server",
    "app_version",
    "ip",
];

/// Numeric filters: column name + ClickHouse type for the bound param.
const NUMERIC_FILTERS: &[(&str, &str)] = &[("http_status", "UInt16")];

/// Dimensions allowed in `group_by` for /v1/stats.
const GROUP_DIMS: &[&str] = &[
    "category",
    "event_type",
    "severity",
    "source",
    "model",
    "entity_type",
    "http_status",
    "route",
    "server",
    "app_version",
];

/// Numeric columns allowed as the field in `sum:`/`avg:`/`min:`/`max:` metrics.
const METRIC_FIELDS: &[&str] = &["tokens_input", "tokens_output", "duration_ms", "http_status"];

// ── helpers ─────────────────────────────────────────────────────────────────

/// Accumulates bound query parameters and hands back their `{name:Type}`
/// placeholders, so no caller-supplied value is ever concatenated into SQL.
struct Params {
    items: Vec<(String, String)>,
}
impl Params {
    fn new() -> Self {
        Params { items: Vec::new() }
    }
    fn add(&mut self, ty: &str, val: impl Into<String>) -> String {
        let name = format!("p{}", self.items.len());
        self.items.push((name.clone(), val.into()));
        format!("{{{name}:{ty}}}")
    }
}

/// A safe SQL identifier (used for the tenant database name, which cannot be a
/// bound parameter). Tenants are admin-controlled slugs; validate anyway.
fn safe_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    s.len() <= 64 && s.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn bad(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(json!({ "error": msg }))
}

fn is_missing(err: &str) -> bool {
    err.contains("UNKNOWN_TABLE")
        || err.contains("UNKNOWN_DATABASE")
        || err.contains("doesn't exist")
}

/// Parse an absolute (`RFC3339` / `YYYY-MM-DD HH:MM:SS`) or relative
/// (`-15m`, `-1h`, `-7d`, `-30s`) time. Relative is resolved against `now`.
fn parse_time(s: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('-') {
        let split = rest.find(|c: char| !c.is_ascii_digit())?;
        let (num, unit) = rest.split_at(split);
        let n: i64 = num.parse().ok()?;
        let dur = match unit {
            "s" => Duration::seconds(n),
            "m" => Duration::minutes(n),
            "h" => Duration::hours(n),
            "d" => Duration::days(n),
            _ => return None,
        };
        return Some(now - dur);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    const FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ];
    for fmt in FORMATS {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
        }
    }
    None
}

fn fmt_ch(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Resolve the API key to its tenant (same path as ingest). Returns the tenant
/// or an error response to short-circuit on.
async fn resolve(req: &HttpRequest, state: &State) -> Result<String, HttpResponse> {
    let Some(key) = bearer_or_apikey(req) else {
        return Err(HttpResponse::Unauthorized().json(json!({ "error": "missing api key" })));
    };
    let Some(tenant) = state.keys.tenant_for(&key).await else {
        return Err(HttpResponse::Unauthorized().json(json!({ "error": "invalid api key" })));
    };
    if !safe_ident(&tenant) {
        return Err(HttpResponse::InternalServerError()
            .json(json!({ "error": "invalid tenant identifier" })));
    }
    Ok(tenant)
}

/// Build the shared WHERE clauses (time range + allowlisted filters). Does NOT
/// include cursor paging. Returns an error message on a bad filter value.
fn build_filters(
    q: &HashMap<String, String>,
    p: &mut Params,
    now: DateTime<Utc>,
) -> Result<Vec<String>, String> {
    let mut wh = Vec::new();

    let from = match q.get("from").map(|s| s.as_str()) {
        Some(s) => parse_time(s, now).ok_or("invalid 'from' (use -1h / -7d / RFC3339)")?,
        None => now - Duration::hours(1),
    };
    let to = match q.get("to").map(|s| s.as_str()) {
        Some(s) => parse_time(s, now).ok_or("invalid 'to' (use 'now' default / -1h / RFC3339)")?,
        None => now,
    };
    wh.push(format!("ts >= {}", p.add("DateTime64(3)", fmt_ch(from))));
    wh.push(format!("ts <= {}", p.add("DateTime64(3)", fmt_ch(to))));

    for f in STRING_FILTERS {
        if let Some(v) = q.get(*f) {
            let vals: Vec<&str> = v.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
            if vals.is_empty() {
                continue;
            }
            let ph: Vec<String> = vals.iter().map(|val| p.add("String", *val)).collect();
            wh.push(format!("{f} IN ({})", ph.join(", ")));
        }
    }

    for (f, ty) in NUMERIC_FILTERS {
        if let Some(v) = q.get(*f) {
            let vals: Vec<&str> = v.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
            let mut ph = Vec::new();
            for val in vals {
                if val.parse::<u64>().is_err() {
                    return Err(format!("filter '{f}' must be a number"));
                }
                ph.push(p.add(ty, val));
            }
            if !ph.is_empty() {
                wh.push(format!("{f} IN ({})", ph.join(", ")));
            }
        }
    }

    if let Some(qq) = q.get("q") {
        if !qq.is_empty() {
            wh.push(format!(
                "positionCaseInsensitive(message, {}) > 0",
                p.add("String", qq.clone())
            ));
        }
    }

    Ok(wh)
}

// ── GET /v1/events — search / list ──────────────────────────────────────────

pub async fn list_events(
    req: HttpRequest,
    state: web::Data<State>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let tenant = match resolve(&req, &state).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let q = qs.into_inner();
    let now = Utc::now();

    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let desc = q.get("order").map(|s| s != "asc").unwrap_or(true);

    let mut p = Params::new();
    let mut wh = match build_filters(&q, &mut p, now) {
        Ok(w) => w,
        Err(e) => return bad(&e),
    };

    // Keyset pagination on (ts, event_id). Cursor format: "<unix_millis>:<uuid>".
    if let Some(cur) = q.get("cursor") {
        if let Some((ms, id)) = cur.split_once(':') {
            if ms.parse::<i64>().is_ok() {
                let cmp = if desc { "<" } else { ">" };
                let ms_ph = p.add("Int64", ms);
                let id_ph = p.add("String", id);
                wh.push(format!(
                    "(toUnixTimestamp64Milli(ts) {cmp} {ms_ph} OR (toUnixTimestamp64Milli(ts) = {ms_ph} AND toString(event_id) {cmp} {id_ph}))"
                ));
            }
        }
    }

    let dir = if desc { "DESC" } else { "ASC" };
    let sql = format!(
        "SELECT *, toUnixTimestamp64Milli(ts) AS _ts_ms FROM `{tenant}`.events \
         WHERE {} ORDER BY ts {dir}, event_id {dir} LIMIT {limit} FORMAT JSONEachRow",
        wh.join(" AND ")
    );

    let mut events = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return HttpResponse::Ok().json(json!({ "events": [], "next_cursor": Value::Null }));
            }
            log::warn!("read query failed for '{tenant}': {e}");
            return HttpResponse::InternalServerError().json(json!({ "error": "query failed" }));
        }
    };

    // Build the next cursor from the last row, then drop the helper column.
    let next_cursor = if events.len() == limit {
        events.last().and_then(|r| {
            let ms = r.get("_ts_ms")?;
            let ms = ms.as_i64().or_else(|| ms.as_str().and_then(|s| s.parse().ok()))?;
            let id = r.get("event_id")?.as_str()?;
            Some(format!("{ms}:{id}"))
        })
    } else {
        None
    };
    for e in events.iter_mut() {
        if let Some(o) = e.as_object_mut() {
            o.remove("_ts_ms");
        }
    }

    HttpResponse::Ok().json(json!({ "events": events, "next_cursor": next_cursor }))
}

// ── GET /v1/events/{event_id} — single event ────────────────────────────────

pub async fn get_event(
    req: HttpRequest,
    state: web::Data<State>,
    path: web::Path<String>,
) -> impl Responder {
    let tenant = match resolve(&req, &state).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let id = path.into_inner();
    if uuid::Uuid::parse_str(&id).is_err() {
        return bad("event_id must be a UUID");
    }
    let mut p = Params::new();
    let id_ph = p.add("UUID", id);
    let sql =
        format!("SELECT * FROM `{tenant}`.events WHERE event_id = {id_ph} LIMIT 1 FORMAT JSONEachRow");

    let rows = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return HttpResponse::NotFound().json(json!({ "error": "not found" }));
            }
            log::warn!("read query failed for '{tenant}': {e}");
            return HttpResponse::InternalServerError().json(json!({ "error": "query failed" }));
        }
    };
    match rows.into_iter().next() {
        Some(r) => HttpResponse::Ok().json(r),
        None => HttpResponse::NotFound().json(json!({ "error": "not found" })),
    }
}

// ── GET /v1/stats — aggregates / timeseries ─────────────────────────────────

fn parse_metric(opt: Option<&String>) -> Result<String, String> {
    match opt.map(|s| s.as_str()).unwrap_or("count") {
        "count" => Ok("count()".to_string()),
        other => {
            let (agg, field) = other
                .split_once(':')
                .ok_or("metric must be 'count' or '<agg>:<field>'")?;
            if !METRIC_FIELDS.contains(&field) {
                return Err(format!("metric field '{field}' not allowed"));
            }
            match agg {
                "sum" | "avg" | "min" | "max" => Ok(format!("{agg}({field})")),
                _ => Err("metric agg must be sum|avg|min|max".to_string()),
            }
        }
    }
}

/// Map an `interval` token to a ClickHouse bucket expression.
fn interval_expr(s: &str) -> Option<&'static str> {
    Some(match s {
        "1m" => "toStartOfInterval(ts, INTERVAL 1 MINUTE)",
        "5m" => "toStartOfInterval(ts, INTERVAL 5 MINUTE)",
        "15m" => "toStartOfInterval(ts, INTERVAL 15 MINUTE)",
        "30m" => "toStartOfInterval(ts, INTERVAL 30 MINUTE)",
        "1h" => "toStartOfInterval(ts, INTERVAL 1 HOUR)",
        "6h" => "toStartOfInterval(ts, INTERVAL 6 HOUR)",
        "12h" => "toStartOfInterval(ts, INTERVAL 12 HOUR)",
        "1d" => "toStartOfInterval(ts, INTERVAL 1 DAY)",
        _ => return None,
    })
}

pub async fn stats(
    req: HttpRequest,
    state: web::Data<State>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let tenant = match resolve(&req, &state).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let q = qs.into_inner();
    let now = Utc::now();

    let group = match q.get("group_by") {
        Some(g) if GROUP_DIMS.contains(&g.as_str()) => g.clone(),
        Some(_) => return bad("invalid group_by"),
        None => return bad("group_by is required"),
    };
    let metric = match parse_metric(q.get("metric")) {
        Ok(m) => m,
        Err(e) => return bad(&e),
    };
    let bucket = match q.get("interval") {
        Some(i) => match interval_expr(i) {
            Some(expr) => Some(expr),
            None => return bad("invalid interval (1m|5m|15m|30m|1h|6h|12h|1d)"),
        },
        None => None,
    };

    let mut p = Params::new();
    let wh = match build_filters(&q, &mut p, now) {
        Ok(w) => w,
        Err(e) => return bad(&e),
    };

    let (mut select, mut group_by, order) = if let Some(b) = bucket {
        (
            format!("{group} AS group_value, {b} AS bucket"),
            "group_value, bucket".to_string(),
            "bucket ASC, value DESC".to_string(),
        )
    } else {
        (
            format!("{group} AS group_value"),
            "group_value".to_string(),
            "value DESC".to_string(),
        )
    };
    select.push_str(&format!(", {metric} AS value"));
    let _ = &mut group_by;

    let sql = format!(
        "SELECT {select} FROM `{tenant}`.events WHERE {} GROUP BY {group_by} ORDER BY {order} \
         LIMIT {STATS_MAX_ROWS} FORMAT JSONEachRow",
        wh.join(" AND ")
    );

    let rows = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return HttpResponse::Ok().json(json!({ "stats": [] }));
            }
            log::warn!("stats query failed for '{tenant}': {e}");
            return HttpResponse::InternalServerError().json(json!({ "error": "query failed" }));
        }
    };
    HttpResponse::Ok().json(json!({ "stats": rows }))
}
