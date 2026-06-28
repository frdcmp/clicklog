//! The canonical event standard — the single source of truth for what the
//! gateway accepts. Every event POSTed to `/v1/events` is validated against this
//! BEFORE it is buffered; anything that violates it is rejected synchronously
//! with a 400 and never reaches the stream. There is no fallback: producers must
//! emit standard events or get an error.
//!
//! The field set here mirrors `ch::EVENTS_DDL` exactly. Keep the two in sync —
//! the validator is also the poison-pill guard for the async drain (a value the
//! drain's ClickHouse INSERT would choke on is rejected here first).

use chrono::{DateTime, NaiveDateTime};
use serde_json::Value;

/// Fields that MUST be present and non-empty on every event.
const REQUIRED: &[&str] = &["category", "event_type"];

/// String-typed columns (free-text `String` + `LowCardinality(String)`), plus
/// the `attributes` JSON-blob escape hatch. `event_id` is validated separately
/// (must be a UUID). Anything custom goes inside `attributes`, not at top level.
const STRING_FIELDS: &[&str] = &[
    "source",
    "category",
    "event_type",
    "severity",
    "user_id",
    "user_email",
    "session_id",
    "request_id",
    "entity_type",
    "entity_id",
    "message",
    "error_code",
    "model",
    "route",
    "app_version",
    "server",
    "ip",
    "user_agent",
    "attributes",
];

/// Unsigned-integer columns and their inclusive upper bound (the ClickHouse
/// column width). Sent as JSON numbers, never quoted strings.
const NUMERIC_FIELDS: &[(&str, u64)] = &[
    ("tokens_input", u32::MAX as u64),
    ("tokens_output", u32::MAX as u64),
    ("duration_ms", u32::MAX as u64),
    ("http_status", u16::MAX as u64),
];

/// `DateTime64(3)` columns. Accept RFC3339 / common datetime strings or a
/// numeric epoch.
const TS_FIELDS: &[&str] = &["ts", "received_at"];

/// Allowed `severity` values.
const SEVERITIES: &[&str] = &["debug", "info", "warn", "error"];

fn is_known(field: &str) -> bool {
    field == "event_id"
        || STRING_FIELDS.contains(&field)
        || TS_FIELDS.contains(&field)
        || NUMERIC_FIELDS.iter().any(|(f, _)| *f == field)
}

fn valid_timestamp(v: &Value) -> bool {
    if v.is_number() {
        return true; // epoch seconds / millis
    }
    let Some(s) = v.as_str() else { return false };
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if DateTime::parse_from_rfc3339(s).is_ok() {
        return true;
    }
    const FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ];
    FORMATS
        .iter()
        .any(|fmt| NaiveDateTime::parse_from_str(s, fmt).is_ok())
}

/// Validate a single event against the standard. Returns the list of violations
/// (empty == valid).
pub fn validate_event(v: &Value) -> Vec<String> {
    let Some(obj) = v.as_object() else {
        return vec!["event must be a JSON object".to_string()];
    };

    let mut errs = Vec::new();

    // No unknown fields — anything not in the standard belongs in `attributes`.
    for key in obj.keys() {
        if !is_known(key) {
            errs.push(format!(
                "unknown field '{key}' (custom data must go inside the 'attributes' string)"
            ));
        }
    }

    // Required fields present and non-empty.
    for req in REQUIRED {
        let ok = obj
            .get(*req)
            .and_then(|x| x.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !ok {
            errs.push(format!("missing required string field '{req}'"));
        }
    }

    // String columns must be strings.
    for f in STRING_FIELDS {
        if let Some(val) = obj.get(*f) {
            if !val.is_string() {
                errs.push(format!("field '{f}' must be a string"));
            }
        }
    }

    // severity must be one of the allowed values.
    if let Some(val) = obj.get("severity") {
        if let Some(sev) = val.as_str() {
            if !SEVERITIES.contains(&sev) {
                errs.push(format!(
                    "severity '{sev}' invalid; allowed: {}",
                    SEVERITIES.join(", ")
                ));
            }
        }
    }

    // event_id, if supplied, must be a UUID (the column type is UUID).
    if let Some(val) = obj.get("event_id") {
        match val.as_str() {
            Some(s) if uuid::Uuid::parse_str(s).is_ok() => {}
            _ => errs.push("field 'event_id' must be a UUID string".to_string()),
        }
    }

    // Numeric columns must be non-negative integers within range.
    for (f, max) in NUMERIC_FIELDS {
        if let Some(val) = obj.get(*f) {
            match val.as_u64() {
                Some(n) if n <= *max => {}
                Some(n) => errs.push(format!("field '{f}'={n} exceeds max {max}")),
                None => errs.push(format!("field '{f}' must be a non-negative integer")),
            }
        }
    }

    // Timestamp columns must parse.
    for f in TS_FIELDS {
        if let Some(val) = obj.get(*f) {
            if !valid_timestamp(val) {
                errs.push(format!(
                    "field '{f}' must be an RFC3339 datetime string or an epoch number"
                ));
            }
        }
    }

    errs
}
