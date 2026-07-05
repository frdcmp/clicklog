<p align="center">
  <img src="frontend/public/logo.svg" alt="clicklog — a pixel-art wood log" width="128">
</p>

<h1 align="center">clicklog</h1>

**Centralised log ingestion: apps POST events, clicklog validates, queues, and
stores them in ClickHouse.**

One pipeline, one entry point. Every app stack ships its telemetry here with
nothing but a URL and an API key:

```
app ──POST /v1/events (Bearer <key>)──▶ ingest-api ──▶ valkey queue ──▶ ClickHouse <tenant>.events
```

| Service | Role | Container port | Default host port | Docs |
|---------|------|----------------|-------------------|------|
| `ingest-api` | **The only entry point** — validates events against the standard, queues them, drains them to ClickHouse | `8080` | `46005` | [README](ingest-api/README.md) |
| `valkey` | **Internal queue** — buffers accepted events between ingest-api and ClickHouse. Used exclusively by ingest-api | `6379` | **none (internal-only)** | [README](valkey/README.md) |
| `clickhouse` | **The log store** — one database per project | `8123` (HTTP) | **none (internal-only)** | [README](clickhouse/README.md) |

> **Everything is gateway-only.** Neither ClickHouse nor Valkey publishes a host
> port — both are reachable only on the internal `clicklog` network, in practice
> only by `ingest-api`. Apps **cannot** write to ClickHouse or touch the queue
> directly; all event data must go through `POST /v1/events`, which enforces the
> event standard and rejects anything off-spec. There is no fallback.

The services run from **one combined Docker Compose stack** at the repo root
(`docker-compose.yml` + `.env`), sharing a single `clicklog` bridge network.
They come up and down together on **one host**. Each service keeps its own
config/data subfolder and README.

---

## Deployment topology (fill in per environment)

The whole stack runs on **one host** (all services share the `clicklog` network).
What interface each published port binds to and which port it uses are set in
the single root `.env` — not baked into the repo. Record your actual layout here.

| Service | Bind interface | Endpoint | `.env` knobs |
|---------|----------------|----------|--------------|
| ingest-api | _overlay IP_ | `http://<ip>:46005` | `INGEST_BIND`, `INGEST_EXT_PORT` |
| frontend (dashboard, optional) | _overlay IP_ | `http://<ip>:46006` | `FRONTEND_BIND`, `FRONTEND_EXT_PORT` |
| clickhouse | — | internal-only (`clickhouse:8123`) | — |
| valkey | — | internal-only (`valkey:6379`) | — |

**Networking & security model:**

- Only `ingest-api` (and the optional dashboard) publish a port, bound to
  whatever interface you set (`*_BIND`). Put them on a **private overlay** (the
  stacks here use one) and **never** bind to a public NIC.
- ClickHouse and Valkey publish **nothing** — they live entirely on the internal
  `clicklog` network, credentialed and reachable only by `ingest-api`.
- Auth is always on: apps authenticate to the gateway with an API key; the
  dashboard with a JWT login.

---

## The tenant model (how isolation works)

ClickHouse is **multi-tenant**: one shared server, carved up per project. The
tenant id is the **project name** — it names both the ClickHouse database and
the API key's scope. Keep it consistent everywhere.

| | ClickHouse |
|--|-----------|
| Isolation unit | a **database** per project |
| Identity | an **ingest API key** per project (only the gateway can reach ClickHouse) |
| Configured via | a key minted into `ingest.ingest_keys` (see ingest-api) |
| Provisioned | DB + `events` table auto-created by the gateway on first write |

Valkey has **no tenants** — it is the gateway's private queue, not a shared
store. See [valkey/README.md](valkey/README.md).

---

## How an app connects

Apps hold **only an API key + a URL** and POST event batches. The queue and the
drain-to-ClickHouse worker live here, so app repos stay clean and publishable —
no ClickHouse client, no Redis client, no logs-worker.

```
app ──POST /v1/events (Bearer <key>)──▶ ingest-api ──▶ Valkey ingest:events ──▶ (drain) ──▶ ClickHouse <tenant>.events
```

The app config is just:

```dotenv
TELEMETRY_INGEST_URL="http://<infra-host>:46005/v1/events"   # or ingest-api:8080 on-host
TELEMETRY_API_KEY="ik_…"                                     # one key → one tenant
```

**Onboarding a project**: mint a key, set two env vars, POST events. The
tenant's ClickHouse database + `events` table are created automatically on
first write — nothing else to configure.

Keys are minted in the **admin dashboard** (API Keys page — the key is shown
once, store it). For scripted flows, log in for a JWT and hit the same admin
API the dashboard uses (see [ingest-api/README.md](ingest-api/README.md)).

📖 **Full guide — HTTP API, event schema, key management, retention,
integration, ops & troubleshooting: [ingest-api/README.md](ingest-api/README.md).**

### Client side — just POST

That's the whole integration: send events to `POST /v1/events` and move on.
Queuing, durability, retries, and the ClickHouse write all live **here** in the
gateway (events land on its Valkey stream and survive a ClickHouse outage) —
an app needs no Redis, no worker, no local queue. Fire-and-forget is a
perfectly good client; batching (up to 1000 events per request) is optional if
you'd rather send once a second than per event.

### The event standard (enforced — no fallback)

Every event is validated against this schema **before** it is buffered. Any
violation rejects the **whole batch** with `400` and a per-event error list;
nothing off-spec ever reaches ClickHouse. The contract lives in
[`ingest-api/src/schema.rs`](ingest-api/src/schema.rs) and mirrors the `events`
table in [`ingest-api/src/ch.rs`](ingest-api/src/ch.rs).

- **Required** (non-empty strings): `category`, `event_type`.
- **Soft-required: `route`** on any request/operation-scoped event — the HTTP
  path (`/api/v1/orders`) or a logical operation name (`worker:email_send`).
  Not validated, but it's a dashboard column and a server-side filter — set it.
- **No unknown fields.** Anything not in the standard is rejected — put custom
  data inside the `attributes` field (a JSON **string**).
- **String fields:** `source`, `category`, `event_type`, `severity`, `user_id`,
  `user_email`, `session_id`, `request_id`, `entity_type`, `entity_id`,
  `message`, `error_code`, `model`, `route`, `app_version`, `server`, `ip`,
  `user_agent`, `attributes`.
- **`severity`** ∈ `debug | info | warn | error`.
- **`event_id`** (optional): must be a UUID string.
- **Integer fields** (non-negative, in range): `tokens_input`, `tokens_output`,
  `duration_ms` (UInt32), `http_status` (UInt16).
- **Timestamps** `ts`, `received_at` (optional): RFC3339 string or epoch number.

Body may be a JSON object, an array of objects, or NDJSON (≤ 1000 events/batch).
Example valid event:

```json
{"category":"http","event_type":"GET","severity":"info","route":"/widgets/summary","http_status":200,"duration_ms":12,"attributes":"{\"region\":\"eu\"}"}
```

### Reading events back

The **same API key** that writes also reads — one key per project, full
read+write, scoped to that project's own `events`. Apps just hold a URL + key.
Callers never send SQL; they pass structured params, the gateway builds
parameter-bound, read-only ClickHouse queries from a fixed column allowlist.

| Endpoint | Purpose |
|----------|---------|
| `GET /v1/events` | Search/list. Params: `from`,`to` (`-1h`/`-7d`/RFC3339, default last 1h → now), `category`/`event_type`/`severity`/`source`/`model`/`user_id`/… (exact, comma = OR), `http_status`, `q` (substring on `message`), `order` (`asc`/`desc`), `limit` (≤1000), `cursor`. Returns `{events, next_cursor}`. |
| `GET /v1/events/{event_id}` | Fetch one event by UUID. |
| `GET /v1/stats` | Aggregates. Params: `group_by` (`category`/`event_type`/`severity`/`source`/`model`/`http_status`/…), `interval` (`1m`/`5m`/`1h`/`1d` → timeseries; omit → totals), `metric` (`count`, `sum:tokens_input`, `avg:duration_ms`, …), + same filters. |

```bash
# last 24h of warnings on http routes
curl -H "x-api-key: ik_…" "$URL/v1/events?from=-24h&category=http&severity=warn&limit=50"
# requests per hour, last day
curl -H "x-api-key: ik_…" "$URL/v1/stats?from=-24h&group_by=event_type&interval=1h"
# LLM input tokens by model
curl -H "x-api-key: ik_…" "$URL/v1/stats?from=-30d&group_by=model&metric=sum:tokens_input"
```

---

## Quick start

The whole stack comes up from one compose file at the repo root:

```bash
cp .env.example .env        # then edit: strong passwords + bind IP
docker compose up -d
docker compose logs -f

# operate one service at a time when needed:
docker compose restart ingest-api
docker compose logs -f clickhouse
```

---

## Layout

```
clicklog/
├── README.md            ← you are here: the connection conventions
├── docker-compose.yml   ← the combined stack (all four services)
├── .env / .env.example  ← single env for the whole stack
├── clickhouse/          ← log store: config.d/, init/, README, data dirs
├── valkey/              ← internal log queue: valkey.conf, README, data dir
├── ingest-api/          ← telemetry gateway (Rust): src/, Dockerfile, README
└── frontend/            ← admin dashboard (React+nginx, profile: dashboard), README
```

> **Admin dashboard (optional):** a React UI for API-key CRUD, cross-tenant log
> search, and docs, served by nginx and gated behind the `dashboard` compose
> profile. Start the stack with it via `docker compose --profile dashboard up -d`
> (default `127.0.0.1:46006`). Login is a single seeded admin (JWT) — set
> `JWT_SECRET` + `ADMIN_PASSWORD` in `.env`. See [frontend/README.md](frontend/README.md).

Secrets (`.env`) and data volumes (`*_data/`, `*_logs/`) are git-ignored — only
`.env.example` and config are tracked.
