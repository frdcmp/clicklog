//! Read API — queries over each project's `events` table.
//!
//! Two entry paths share the same query builders:
//!   • `/v1/events|stats` (app-facing): the API key resolves to ONE tenant, and
//!     every query is hard-scoped to that tenant's `<tenant>.events` table.
//!   • `/v1/admin/events|stats` (dashboard): a JWT admin passes an explicit
//!     `tenant` param — a single tenant, or `*`/`all` to query ACROSS every
//!     tenant that has an `events` table (UNION ALL, each row tagged `_tenant`).
//!
//! Callers never send SQL — they send structured query params; this module
//! builds parameter-bound, read-only ClickHouse queries from a fixed allowlist of
//! columns. Filter VALUES are bound (`{p:Type}`); column/dimension NAMES and the
//! tenant database identifiers come only from the allowlists / `safe_ident`.

use std::collections::HashMap;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::{json, Value};

use crate::handlers::{bearer_or_apikey, State};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const STATS_MAX_ROWS: usize = 5000;
/// Safety cap on how many tenant DBs a single cross-tenant query fans out over.
const MAX_UNION_TENANTS: usize = 64;

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

/// Dimensions allowed in `group_by` for stats.
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
pub(crate) fn safe_ident(s: &str) -> bool {
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
    if s.eq_ignore_ascii_case("now") {
        return Some(now);
    }
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

// ── tenant resolution for the admin path ──────────────────────────────────────

/// Resolve the admin `tenant` query param into `(tenant DBs, cross)`. A concrete
/// name → `([name], false)`. `*` / `all` / empty → every tenant that currently
/// has an `events` table (from `system.tables`), with `cross = true` — which
/// makes the read tag each row with `_tenant` and enables `group_by=tenant`, even
/// when only one tenant happens to have data.
pub(crate) async fn resolve_admin_tenants(
    state: &State,
    param: Option<&String>,
) -> Result<(Vec<String>, bool), HttpResponse> {
    match param.map(|s| s.trim()) {
        Some("*") | Some("all") | Some("") | None => {
            let mut tenants = existing_event_tenants(state).await;
            tenants.truncate(MAX_UNION_TENANTS);
            Ok((tenants, true))
        }
        Some(name) => {
            if !safe_ident(name) {
                return Err(bad("invalid tenant"));
            }
            Ok((vec![name.to_string()], false))
        }
    }
}

/// Databases (other than the control-plane `ingest` db) that have an `events`
/// table, i.e. tenants that have received at least one event.
pub(crate) async fn existing_event_tenants(state: &State) -> Vec<String> {
    let sql = "SELECT database FROM system.tables WHERE name = 'events' AND database NOT IN ('ingest','system','default') ORDER BY database";
    let rows = state.ch.query_rows(sql).await.unwrap_or_default();
    rows.into_iter()
        .filter_map(|r| r.get("database").and_then(|v| v.as_str()).map(String::from))
        .filter(|t| safe_ident(t))
        .collect()
}

// ── event search core (shared by v1 + admin) ─────────────────────────────────

/// Build the events SELECT. `tag=false` → plain single-tenant scan (no `_tenant`
/// column, so the v1 shape is unchanged). `tag=true` → UNION ALL with a `_tenant`
/// literal per branch (works for one or many tenants), ordered/limited globally.
fn events_sql(tenants: &[String], wh: &[String], dir: &str, limit: usize, tag: bool) -> String {
    let where_clause = wh.join(" AND ");
    if !tag {
        let t = &tenants[0];
        format!(
            "SELECT *, toUnixTimestamp64Milli(ts) AS _ts_ms FROM `{t}`.events \
             WHERE {where_clause} ORDER BY ts {dir}, event_id {dir} LIMIT {limit} FORMAT JSONEachRow"
        )
    } else {
        let branches: Vec<String> = tenants
            .iter()
            .map(|t| {
                format!(
                    "SELECT *, '{t}' AS _tenant, toUnixTimestamp64Milli(ts) AS _ts_ms \
                     FROM `{t}`.events WHERE {where_clause}"
                )
            })
            .collect();
        format!(
            "SELECT * FROM ({}) ORDER BY ts {dir}, event_id {dir} LIMIT {limit} FORMAT JSONEachRow",
            branches.join(" UNION ALL ")
        )
    }
}

/// Search events across `tenants`. `cross` tags rows with `_tenant` and is set by
/// the admin "all" selection. Returns the JSON body `{events, next_cursor}` or an
/// error `HttpResponse`.
pub(crate) async fn query_events(
    state: &State,
    tenants: &[String],
    cross: bool,
    q: &HashMap<String, String>,
) -> Result<Value, HttpResponse> {
    if tenants.is_empty() {
        return Ok(json!({ "events": [], "next_cursor": Value::Null }));
    }
    let now = Utc::now();
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let desc = q.get("order").map(|s| s != "asc").unwrap_or(true);

    let mut p = Params::new();
    let mut wh = build_filters(q, &mut p, now).map_err(|e| bad(&e))?;

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
    let sql = events_sql(tenants, &wh, dir, limit, cross);

    let mut events = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return Ok(json!({ "events": [], "next_cursor": Value::Null }));
            }
            log::warn!("read query failed for {tenants:?}: {e}");
            return Err(HttpResponse::InternalServerError().json(json!({ "error": "query failed" })));
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

    Ok(json!({ "events": events, "next_cursor": next_cursor }))
}

/// Fetch a single event by UUID from a single tenant.
pub(crate) async fn query_event(
    state: &State,
    tenant: &str,
    id: &str,
) -> Result<Value, HttpResponse> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(bad("event_id must be a UUID"));
    }
    let mut p = Params::new();
    let id_ph = p.add("UUID", id.to_string());
    let sql = format!(
        "SELECT * FROM `{tenant}`.events WHERE event_id = {id_ph} LIMIT 1 FORMAT JSONEachRow"
    );
    let rows = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return Err(HttpResponse::NotFound().json(json!({ "error": "not found" })));
            }
            log::warn!("read query failed for '{tenant}': {e}");
            return Err(HttpResponse::InternalServerError().json(json!({ "error": "query failed" })));
        }
    };
    match rows.into_iter().next() {
        Some(r) => Ok(r),
        None => Err(HttpResponse::NotFound().json(json!({ "error": "not found" }))),
    }
}

// ── stats core (shared by v1 + admin) ─────────────────────────────────────────

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

/// Build the stats FROM source. `tag=false` → the table directly (WHERE inline).
/// `tag=true` → a UNION ALL subquery, each branch tagged with a `_tenant` literal
/// so `group_by=tenant` works (also fine with a single tenant).
fn stats_from(tenants: &[String], where_clause: &str, tag: bool) -> String {
    if !tag {
        format!("`{}`.events WHERE {where_clause}", tenants[0])
    } else {
        let branches: Vec<String> = tenants
            .iter()
            .map(|t| format!("SELECT *, '{t}' AS _tenant FROM `{t}`.events WHERE {where_clause}"))
            .collect();
        format!("({})", branches.join(" UNION ALL "))
    }
}

/// Compute aggregates / timeseries across `tenants`. `cross` (admin "all")
/// enables the synthetic `tenant` dimension. Returns `{stats}` body.
pub(crate) async fn query_stats(
    state: &State,
    tenants: &[String],
    cross: bool,
    q: &HashMap<String, String>,
) -> Result<Value, HttpResponse> {
    if tenants.is_empty() {
        return Ok(json!({ "stats": [] }));
    }
    let now = Utc::now();

    // Resolve the group dimension. `tenant` is a synthetic dim only valid on a
    // cross-tenant query (the `_tenant` literal exists then).
    let group_col = match q.get("group_by").map(|s| s.as_str()) {
        Some("tenant") if cross => "_tenant".to_string(),
        Some("tenant") => return Err(bad("group_by=tenant requires the 'all' tenant selection")),
        Some(g) if GROUP_DIMS.contains(&g) => g.to_string(),
        Some(_) => return Err(bad("invalid group_by")),
        None => return Err(bad("group_by is required")),
    };
    let metric = parse_metric(q.get("metric")).map_err(|e| bad(&e))?;
    let bucket = match q.get("interval") {
        Some(i) => match interval_expr(i) {
            Some(expr) => Some(expr),
            None => return Err(bad("invalid interval (1m|5m|15m|30m|1h|6h|12h|1d)")),
        },
        None => None,
    };

    let mut p = Params::new();
    let wh = build_filters(q, &mut p, now).map_err(|e| bad(&e))?;

    let (mut select, group_by, order) = if let Some(b) = bucket {
        (
            format!("{group_col} AS group_value, {b} AS bucket"),
            "group_value, bucket".to_string(),
            "bucket ASC, value DESC".to_string(),
        )
    } else {
        (
            format!("{group_col} AS group_value"),
            "group_value".to_string(),
            "value DESC".to_string(),
        )
    };
    select.push_str(&format!(", {metric} AS value"));

    let from = stats_from(tenants, &wh.join(" AND "), cross);
    let sql = format!(
        "SELECT {select} FROM {from} GROUP BY {group_by} ORDER BY {order} \
         LIMIT {STATS_MAX_ROWS} FORMAT JSONEachRow"
    );

    let rows = match state.ch.query_rows_params(&sql, &p.items).await {
        Ok(r) => r,
        Err(e) => {
            if is_missing(&e) {
                return Ok(json!({ "stats": [] }));
            }
            log::warn!("stats query failed for {tenants:?}: {e}");
            return Err(HttpResponse::InternalServerError().json(json!({ "error": "query failed" })));
        }
    };
    Ok(json!({ "stats": rows }))
}

// ── v1 handlers (API-key scoped to one tenant) ────────────────────────────────

pub async fn list_events(
    req: HttpRequest,
    state: web::Data<State>,
    qs: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let tenant = match resolve(&req, &state).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    match query_events(&state, &[tenant], false, &qs.into_inner()).await {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => r,
    }
}

pub async fn get_event(
    req: HttpRequest,
    state: web::Data<State>,
    path: web::Path<String>,
) -> impl Responder {
    let tenant = match resolve(&req, &state).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    match query_event(&state, &tenant, &path.into_inner()).await {
        Ok(ev) => HttpResponse::Ok().json(ev),
        Err(r) => r,
    }
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
    match query_stats(&state, &[tenant], false, &qs.into_inner()).await {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(r) => r,
    }
}
