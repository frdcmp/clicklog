//! Actix Web: an `http` event per request ([`AccessLog`]), and the endpoint the pages report
//! their errors to ([`client_errors`], [`ingest_client_errors`]).
//!
//! ```ignore
//! App::new()
//!     .wrap(auth_middleware)
//!     // after the auth middleware, so it is outside it: it sees its refusals, and who it let in
//!     .wrap(clicklog::actix::AccessLog::new().who(|req| {
//!         req.extensions().get::<MyUser>().map(|u| clicklog::Who::id(&u.id))
//!     }))
//!     .route("/telemetry/client", web::post().to(clicklog::actix::client_errors))
//! ```

use std::collections::HashMap;
use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::http::header::HeaderMap;
use actix_web::{web, Error, HttpRequest, HttpResponse};
use serde::Deserialize;

use crate::{cap, Event, Severity, Who};

type WhoFn = Rc<dyn Fn(&HttpRequest) -> Option<Who>>;
type SkipFn = Rc<dyn Fn(&HttpRequest) -> bool>;

/// The visitor's address: Cloudflare's header, then a proxy's (`X-Real-IP`, the first of
/// `X-Forwarded-For`), then the connection's.
pub fn client_ip(req: &HttpRequest) -> String {
    ip_from(req.headers(), req.connection_info().peer_addr())
}

fn ip_from(headers: &HeaderMap, peer: Option<&str>) -> String {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
            .filter(|v| !v.is_empty())
    };
    header("cf-connecting-ip")
        .or_else(|| header("x-real-ip"))
        .or_else(|| header("x-forwarded-for"))
        .unwrap_or_else(|| peer.unwrap_or("").to_string())
}

fn user_agent(req: &HttpRequest) -> String {
    req.headers()
        .get(actix_web::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// One `http` event per response: method, status, time, address, user agent, and the route as
/// the pattern it matched (`/items/{id}`), never the path, which can carry a token; `(no route)`
/// when none matched. OPTIONS, `/health` and `/metrics` are not logged; [`AccessLog::skip`] adds
/// more.
#[derive(Clone)]
pub struct AccessLog {
    who: Option<WhoFn>,
    skip: Option<SkipFn>,
}

impl Default for AccessLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessLog {
    pub fn new() -> Self {
        AccessLog { who: None, skip: None }
    }

    /// Who made the request, read after the handler ran (from the request's extensions).
    pub fn who(mut self, f: impl Fn(&HttpRequest) -> Option<Who> + 'static) -> Self {
        self.who = Some(Rc::new(f));
        self
    }

    /// Requests not to log, besides OPTIONS, `/health` and `/metrics` (long-lived streams, say).
    pub fn skip(mut self, f: impl Fn(&HttpRequest) -> bool + 'static) -> Self {
        self.skip = Some(Rc::new(f));
        self
    }

    fn skipped(&self, req: &HttpRequest) -> bool {
        let path = req.path();
        req.method() == actix_web::http::Method::OPTIONS
            || path == "/health"
            || path == "/metrics"
            || self.skip.as_ref().is_some_and(|skip| skip(req))
    }
}

/// What the event needs from the request, read before the handler has it.
struct Seen {
    method: String,
    ip: String,
    user_agent: String,
    started: Instant,
}

impl<S, B> Transform<S, ServiceRequest> for AccessLog
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AccessLogService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AccessLogService { service: Rc::new(service), log: self.clone() }))
    }
}

pub struct AccessLogService<S> {
    service: Rc<S>,
    log: AccessLog,
}

impl<S, B> Service<ServiceRequest> for AccessLogService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        if self.log.skipped(req.request()) {
            return Box::pin(service.call(req));
        }
        // read now, not kept: a clone of the request would stop the router from matching it
        let seen = Seen {
            method: req.method().to_string(),
            ip: client_ip(req.request()),
            user_agent: user_agent(req.request()),
            started: Instant::now(),
        };
        let who = self.log.who.clone();
        Box::pin(async move {
            let result = service.call(req).await;
            let (status, route, who) = match &result {
                Ok(res) => (
                    res.status().as_u16(),
                    res.request().match_pattern().unwrap_or_else(|| "(no route)".to_string()),
                    who.and_then(|f| f(res.request())),
                ),
                Err(e) => (e.as_response_error().status_code().as_u16(), "(error)".to_string(), None),
            };
            let mut event = Event::new("http", seen.method, Severity::of_status(status))
                .route(route)
                .status(status)
                .duration(seen.started.elapsed().as_millis().min(u32::MAX as u128) as u32)
                .ip(seen.ip)
                .user_agent(seen.user_agent);
            if let Some(who) = who {
                event = event.who(&who);
            }
            event.emit();
            result
        })
    }
}

// ── The pages' errors ────────────────────────────────────────────────────────────────────────

/// Reports one address may send per minute; past it a page looping on an error is ignored.
const MAX_REPORTS: u32 = 60;
const WINDOW: Duration = Duration::from_secs(60);
const MAX_BATCH: usize = 20;
const MAX_MESSAGE: usize = 4_000;
const MAX_SHORT: usize = 200;

static BUDGETS: Mutex<Option<HashMap<String, (Instant, u32)>>> = Mutex::new(None);

fn within_budget(ip: &str) -> bool {
    let mut guard = BUDGETS.lock().unwrap_or_else(|e| e.into_inner());
    let budgets = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    if budgets.len() > 10_000 {
        budgets.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
    }
    let (start, count) = budgets.entry(ip.to_string()).or_insert((now, 0));
    if now.duration_since(*start) >= WINDOW {
        *start = now;
        *count = 0;
    }
    *count += 1;
    *count <= MAX_REPORTS
}

/// One error a page reports.
#[derive(Debug, Default, Deserialize)]
pub struct ClientEvent {
    /// What broke: "window", "render", "api"… (default "client").
    #[serde(default)]
    pub category: String,
    /// "error", "unhandledrejection"… (default "error").
    #[serde(default)]
    pub event_type: String,
    /// "warn", or else it is an error.
    #[serde(default)]
    pub severity: String,
    /// The message, with its stack.
    #[serde(default)]
    pub message: String,
    /// The page it happened on; the page takes tokens out of it before sending.
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
}

/// What the page POSTs: `{"events": [...]}`.
#[derive(Debug, Default, Deserialize)]
pub struct ClientBatch {
    pub events: Vec<ClientEvent>,
}

/// Sends a page's errors on as `source = "frontend"` events, with the caller's address, user
/// agent and, if given, who. The endpoint is meant to be public (sign-in pages break too), so
/// each address has a budget (60 a minute: 429 past it), a batch is cut to 20 and each field to
/// length. Answers 204.
pub fn ingest_client_errors(req: &HttpRequest, batch: ClientBatch, who: Option<Who>) -> HttpResponse {
    let ip = client_ip(req);
    if !within_budget(&ip) {
        return HttpResponse::TooManyRequests().finish();
    }
    let ua = user_agent(req);
    let or = |s: String, fallback: &str| {
        if s.is_empty() { fallback.to_string() } else { cap(s, MAX_SHORT) }
    };
    for ev in batch.events.into_iter().take(MAX_BATCH) {
        let severity = if ev.severity == "warn" { Severity::Warn } else { Severity::Error };
        let mut event = Event::new(or(ev.category, "client"), or(ev.event_type, "error"), severity)
            .source("frontend")
            .msg(cap(ev.message, MAX_MESSAGE))
            .route(cap(ev.route, MAX_SHORT))
            .error_code(cap(ev.error_code, MAX_SHORT))
            .ip(&ip)
            .user_agent(&ua);
        if let Some(attrs) = ev.attributes {
            event = event.attrs(attrs);
        }
        if let Some(who) = &who {
            event = event.who(who);
        }
        event.emit();
    }
    HttpResponse::NoContent().finish()
}

/// The endpoint as a handler, without who: `.route("/telemetry/client", web::post().to(client_errors))`.
/// To attach the user, write a handler that finds them and calls [`ingest_client_errors`].
pub async fn client_errors(req: HttpRequest, batch: web::Json<ClientBatch>) -> HttpResponse {
    ingest_client_errors(&req, batch.into_inner(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::header::{HeaderName, HeaderValue};

    #[test]
    fn ip_prefers_cloudflare_then_proxies_then_peer() {
        let mut h = HeaderMap::new();
        assert_eq!(ip_from(&h, Some("10.0.0.1")), "10.0.0.1");
        h.insert(HeaderName::from_static("x-forwarded-for"), HeaderValue::from_static("1.1.1.1, 10.0.0.2"));
        assert_eq!(ip_from(&h, Some("10.0.0.1")), "1.1.1.1");
        h.insert(HeaderName::from_static("cf-connecting-ip"), HeaderValue::from_static("2.2.2.2"));
        assert_eq!(ip_from(&h, Some("10.0.0.1")), "2.2.2.2");
    }

    #[test]
    fn budget_stops_at_the_limit() {
        for _ in 0..MAX_REPORTS {
            assert!(within_budget("test-budget"));
        }
        assert!(!within_budget("test-budget"));
        assert!(within_budget("someone-else"));
    }
}
