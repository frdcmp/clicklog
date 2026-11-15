# clicklog (Rust client)

Fire-and-forget telemetry from a Rust app to a clicklog gateway (`POST /v1/events`), so each app
does not carry its own copy of the sender.

- `Event::new(category, event_type, severity)…emit()`: never blocks, never fails. A background
  task sends batches about once a second; when the queue is full or the gateway answers with an
  error, events are dropped (durability is the gateway's job).
- Off without `TELEMETRY_INGEST_URL` or `TELEMETRY_API_KEY`: `emit()` does nothing.
- Feature `log`: every `warn!`/`error!` is also an event, and the app's own logger prints as before.
- Feature `actix`: an `http` event per request, logging the route pattern (`/items/{id}`), never
  the path, which can carry a token. Also the public endpoint pages report their errors to.
- Features `rustls` / `native-tls`: only for an `https://` gateway; `http://` needs neither.

## Add it

The crate is in this public repository; depend on a tag, so an app moves to a new version when
it chooses:

```toml
[dependencies]
clicklog = { git = "https://github.com/frdcmp/clicklog", tag = "client-rs-v0.1.1", features = ["log", "actix"] }
```

The environment, as for any clicklog client:

```dotenv
TELEMETRY_INGEST_URL="http://ingest-api.clicklog.svc.cluster.local:8080/v1/events"
TELEMETRY_API_KEY="ik_…"     # the tenant's key, minted in the dashboard
SERVER_NAME="…"              # optional, on every event
APP_VERSION="…"              # optional, on every event
```

## Wire it (Actix Web)

```rust
#[actix_web::main] // or #[tokio::main]
async fn main() -> std::io::Result<()> {
    // instead of env_logger::init(): prints as before, and forwards warnings and errors
    let stdout = env_logger::Builder::from_env(env_logger::Env::default()).build();
    let level = stdout.filter();
    clicklog::log::install(stdout, level);
    clicklog::init_from_env();

    HttpServer::new(|| {
        App::new()
            .wrap(my_auth_middleware())
            // after the auth middleware, so outside it: it sees its refusals, and who it let in
            .wrap(clicklog::actix::AccessLog::new().who(|req| {
                req.extensions().get::<MyUser>().map(|u| clicklog::Who::id(&u.id).email(&u.email))
            }))
            .route("/telemetry/client", web::post().to(clicklog::actix::client_errors))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
```

`AccessLog::skip(|req| …)` leaves out more requests (long-lived streams, say); OPTIONS, `/health`
and `/metrics` are never logged.

`client_errors` takes `{"events": [{category, event_type, severity, message, route, error_code,
attributes}]}` from a page and sends each as a `source = "frontend"` event. It is meant to be
public, so each address has a budget (60 a minute), a batch is cut to 20 events and every field to
length. To attach the signed-in user, write the handler yourself and call
`clicklog::actix::ingest_client_errors(&req, batch, who)`. The page must take tokens out of the
route it reports (`/join/<token>` → `/join/:token`).

## Domain events

```rust
use clicklog::{Event, Severity};

Event::new("llm", "completion", Severity::Info)
    .who(&clicklog::Who::id(user_id))
    .model("claude-sonnet-5-5")
    .tokens(input, output)
    .duration(ms)
    .emit();

Event::new("worker", "email_send", Severity::Error)
    .route("worker:email_send")
    .entity("invite", invite_id)
    .error_code("smtp_timeout")
    .attrs(serde_json::json!({ "attempt": 3 }))
    .emit();
```

Fields are the gateway's columns ([`ingest-api/src/schema.rs`](../ingest-api/src/schema.rs));
anything else goes in `attrs`. Message and attributes are cut at 16 KiB.

## Releasing

Change the version in `Cargo.toml`, run `cargo test --all-features`, commit, and tag
`client-rs-v<version>`; apps move by changing their `tag`.

AGPL-3.0-only, as the rest of clicklog.
