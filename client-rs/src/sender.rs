use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::Event;

/// Where and how to send.
#[derive(Debug, Clone)]
pub struct Config {
    /// The gateway's `…/v1/events`.
    pub url: String,
    /// The tenant's ingest key (`ik_…`).
    pub api_key: String,
    /// Put on events that don't set their own: the host or deployment, and the app's version.
    pub server: String,
    pub app_version: String,
    /// Events waiting to be sent; past it, new ones are dropped rather than waited for.
    pub queue_capacity: usize,
    /// A batch goes when it has this many, or every `flush_interval` (the gateway takes 1000).
    pub batch_max: usize,
    pub flush_interval: Duration,
}

impl Config {
    pub fn new(url: impl ToString, api_key: impl ToString) -> Self {
        Config {
            url: url.to_string(),
            api_key: api_key.to_string(),
            server: String::new(),
            app_version: String::new(),
            queue_capacity: 10_000,
            batch_max: 200,
            flush_interval: Duration::from_secs(1),
        }
    }

    /// `TELEMETRY_INGEST_URL` and `TELEMETRY_API_KEY`, with `SERVER_NAME` and `APP_VERSION`; None
    /// when the URL or the key is unset or empty.
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| {
            std::env::var(name)
                .map(|v| v.trim().trim_matches('"').to_string())
                .unwrap_or_default()
        };
        let (url, key) = (var("TELEMETRY_INGEST_URL"), var("TELEMETRY_API_KEY"));
        if url.is_empty() || key.is_empty() {
            return None;
        }
        let mut config = Config::new(url, key);
        config.server = var("SERVER_NAME");
        config.app_version = var("APP_VERSION");
        Some(config)
    }
}

struct Pipeline {
    tx: mpsc::Sender<Event>,
    server: String,
    app_version: String,
}

static PIPELINE: OnceLock<Pipeline> = OnceLock::new();

/// Telemetry from the environment ([`Config::from_env`]); says on stdout whether it is on.
/// Call once, inside the Tokio runtime. True when it is on.
pub fn init_from_env() -> bool {
    match Config::from_env() {
        Some(config) => init(config),
        None => {
            println!("📤 Telemetry off (TELEMETRY_INGEST_URL or TELEMETRY_API_KEY unset)");
            false
        }
    }
}

/// Starts the background sender. Call once, inside the Tokio runtime; a second call, or one
/// outside a runtime, changes nothing and answers false.
pub fn init(config: Config) -> bool {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        eprintln!("⚠️  Telemetry off: clicklog::init was called outside a Tokio runtime");
        return false;
    };
    let (tx, rx) = mpsc::channel(config.queue_capacity.max(1));
    let pipeline = Pipeline {
        tx,
        server: config.server.clone(),
        app_version: config.app_version.clone(),
    };
    if PIPELINE.set(pipeline).is_err() {
        return false;
    }
    println!("📤 Telemetry on → {}", config.url);
    runtime.spawn(flush_loop(rx, config));
    true
}

/// Whether telemetry is on.
pub fn enabled() -> bool {
    PIPELINE.get().is_some()
}

pub(crate) fn enqueue(mut event: Event) {
    let Some(pipeline) = PIPELINE.get() else { return };
    if event.server.is_empty() {
        event.server.clone_from(&pipeline.server);
    }
    if event.app_version.is_empty() {
        event.app_version.clone_from(&pipeline.app_version);
    }
    let _ = pipeline.tx.try_send(event);
}

async fn flush_loop(mut rx: mpsc::Receiver<Event>, config: Config) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let batch_max = config.batch_max.clamp(1, 1000);
    let mut batch = Vec::with_capacity(batch_max);
    let mut failing = false;
    let mut ticker = tokio::time::interval(config.flush_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            next = rx.recv() => match next {
                Some(event) => {
                    batch.push(event);
                    if batch.len() >= batch_max {
                        send(&client, &config, &mut batch, &mut failing).await;
                    }
                }
                None => {
                    send(&client, &config, &mut batch, &mut failing).await;
                    break;
                }
            },
            _ = ticker.tick() => send(&client, &config, &mut batch, &mut failing).await,
        }
    }
}

/// Says so once when sending starts failing and once when it works again, not every second.
/// Printed, not logged: with the `log` tee, a failing send would feed itself.
async fn send(client: &reqwest::Client, config: &Config, batch: &mut Vec<Event>, failing: &mut bool) {
    if batch.is_empty() {
        return;
    }
    let events = std::mem::take(batch);
    let problem = match client
        .post(&config.url)
        .bearer_auth(&config.api_key)
        .json(&events)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => None,
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            Some(format!("the gateway answered {status} {}", crate::cap(body, 300)))
        }
        Err(e) => Some(e.to_string()),
    };
    match &problem {
        Some(p) if !*failing => eprintln!("⚠️  Telemetry: {p}; events are dropped until it works again"),
        None if *failing => eprintln!("📤 Telemetry works again"),
        _ => {}
    }
    *failing = problem.is_some();
}
