#![cfg(feature = "log")]
mod common;

use std::time::Duration;

use clicklog::Config;

/// Prints nothing and keeps nothing: the app's own logger in the test.
struct Quiet;
impl log::Log for Quiet {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Error
    }
    fn log(&self, _: &log::Record) {}
    fn flush(&self) {}
}

#[tokio::test]
async fn warnings_and_errors_are_forwarded() {
    let (url, mut rx) = common::gateway().await;
    let mut config = Config::new(&url, "ik_test");
    config.flush_interval = Duration::from_millis(100);
    assert!(clicklog::init(config));
    assert!(clicklog::log::install(Quiet, log::LevelFilter::Error));

    log::info!("not this");
    log::warn!("disk almost full");
    log::error!(target: "reqwest::connect", "not the sender's own client");
    log::error!("it broke");

    let (_, events) = common::events(&mut rx, Duration::from_secs(2)).await;
    let messages: Vec<_> = events.iter().map(|e| e["message"].as_str().unwrap()).collect();
    assert_eq!(messages, ["disk almost full", "it broke"]);
    assert_eq!(events[0]["category"], "log");
    assert_eq!(events[0]["severity"], "warn");
    assert_eq!(events[1]["severity"], "error");
}
