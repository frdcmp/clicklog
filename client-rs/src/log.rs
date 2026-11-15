//! Forward `warn!` and `error!` records as `log` events, while the app's own logger prints as
//! before.
//!
//! ```ignore
//! // instead of env_logger::init():
//! let stdout = env_logger::Builder::from_env(env_logger::Env::default()).build();
//! let level = stdout.filter();
//! clicklog::log::install(stdout, level);
//! clicklog::init_from_env();
//! ```

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::{Event, Severity};

/// Targets never forwarded: the HTTP client the sender itself uses.
const QUIET: &[&str] = &["reqwest", "hyper", "h2", "rustls", "clicklog"];

/// The app's logger, and the forwarding.
pub struct Tee<L: Log> {
    inner: L,
}

impl<L: Log> Tee<L> {
    pub fn new(inner: L) -> Self {
        Tee { inner }
    }
}

impl<L: Log> Log for Tee<L> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Warn || self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        // the inner logger applies its own filter (RUST_LOG for env_logger)
        if self.inner.enabled(record.metadata()) {
            self.inner.log(record);
        }
        forward(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// The event for a record, if it is a warning or an error from outside the sender's own client.
pub fn forward(record: &Record) {
    if record.level() > Level::Warn {
        return;
    }
    let target = record.target();
    if QUIET.iter().any(|q| target.starts_with(q)) {
        return;
    }
    let severity = match record.level() {
        Level::Error => Severity::Error,
        _ => Severity::Warn,
    };
    Event::new("log", record.level().as_str(), severity)
        .msg(record.args())
        .route(target)
        .emit();
}

/// Installs `inner` wrapped in a [`Tee`] as the global logger. `inner_level` is what the app's
/// logger prints; warnings and errors reach the tee whatever it is. False when a logger was
/// already installed.
pub fn install<L: Log + 'static>(inner: L, inner_level: LevelFilter) -> bool {
    if log::set_boxed_logger(Box::new(Tee::new(inner))).is_err() {
        return false;
    }
    log::set_max_level(inner_level.max(LevelFilter::Warn));
    true
}
