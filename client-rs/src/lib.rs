//! Fire-and-forget telemetry to a [clicklog](https://github.com/frdcmp/clicklog) gateway.
//!
//! ```no_run
//! # #[tokio::main] async fn main() {
//! // once, inside the Tokio runtime: reads TELEMETRY_INGEST_URL and TELEMETRY_API_KEY
//! clicklog::init_from_env();
//!
//! clicklog::Event::new("auth", "login_failed", clicklog::Severity::Warn)
//!     .user_email("someone@example.com")
//!     .msg("wrong password")
//!     .emit();
//! # }
//! ```
//!
//! `emit()` never blocks and never fails: the event goes on a bounded channel and a background
//! task sends batches to the gateway about once a second. When the queue is full, or the gateway
//! answers with an error, events are dropped (the gateway is where durability lives). Without a
//! URL or key, telemetry is off: `emit()` does nothing, so dev runs with no gateway at all.
//!
//! Features: `log` (forward `warn!`/`error!` records, [`log`]), `actix` (access-log middleware
//! and the browser-error endpoint, [`actix`]), `rustls` or `native-tls` (an `https://` gateway).

mod event;
mod sender;

#[cfg(feature = "actix")]
pub mod actix;
#[cfg(feature = "log")]
pub mod log;

pub use event::{cap, Event, Severity, Who};
pub use sender::{enabled, init, init_from_env, Config};
