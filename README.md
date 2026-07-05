# frdcmp-infra

**Shared infrastructure services for the frdcmp project stacks.**

The backing services that every app stack (`app_one`, `app_two`, and
future projects) plugs into, kept in one repo so the conventions for connecting
to them live in one place:

| Service | Role | Container port | Default host port | Docs |
|---------|------|----------------|-------------------|------|
| `clickhouse` | Centralised **logging** store — one database per project | `8123` (HTTP) | **none (internal-only)** | [README](clickhouse/README.md) |
| `valkey` | Shared **broker / cache / locks** (Redis Streams + Pub/Sub) — one ACL user + key prefix per service | `6379` | `46004` | [README](valkey/README.md) |
| `ingest-api` | **The only way to write logs** — gateway that validates events against the standard and drains them to ClickHouse | `8080` | `46005` | (this README) |

> **Logging is gateway-only.** ClickHouse no longer publishes a host port — it is
> reachable only on the internal `frdcmp` network, in practice only by
> `ingest-api`. Apps **cannot** write to ClickHouse directly; all event data must
> go through `POST /v1/events`, which enforces the event standard and rejects
> anything off-spec. There is no fallback.
>
> **Monitoring (Prometheus + Grafana) was removed for now** — config is kept
> under `./monitoring/` and can be restored from this file's git history.

All four services run from **one combined Docker Compose stack** at the repo
root (`docker-compose.yml` + `.env`), sharing a single `frdcmp` bridge network.
They come up and down together on **one host**. Anything co-located on that host
(including app stacks attached to the `frdcmp` network) can reach them by
container name — `clickhouse:8123`, `valkey:6379`, `prometheus:9090` — with no
overlay hop. Each service keeps its own config/data subfolder and README.

---

## Deployment topology (fill in per environment)

The whole stack runs on **one host** (all services share the `frdcmp` network).
What interface each published port binds to and which port it uses are set in
the single root `.env` — not baked into the repo. Record your actual layout here.

| Service | Bind interface | Endpoint | `.env` knobs |
|---------|----------------|----------|--------------|
| clickhouse | _overlay IP_ | `http://<ip>:46003` | `CLICKHOUSE_BIND`, `CLICKHOUSE_EXT_PORT` |
| valkey | _overlay IP_ | `<ip>:46004` | `VK_BIND`, `VK_EXT_PORT` |
| prometheus / grafana | _host_ | Grafana `http://<ip>:3001`, Prom `:9090` | `GRAFANA_ROOT_URL`, `*_EXT_PORT` |

**Networking & security model:**

- Each service publishes **one** port, bound to whatever interface you set
  (`*_BIND`). Put them on a **private overlay** (the stacks here use the private overlay)
  and **never** bind to a public NIC.
- Auth is always on: ClickHouse per-tenant users, Valkey per-tenant ACL
  passwords, Grafana admin login. The native/internal protocols (ClickHouse
  TCP `9000`) stay inside the container network.
- A stack on **another** host connects via the overlay IP:port. A stack on
  **this** host can attach to the `frdcmp` network and use the container name
  directly — no overlay hop, no published-port round-trip.

---

## The tenant model (how isolation works)

Both data stores are **multi-tenant**: one shared server, carved up per
project. The tenant id is the **project/service name**, and it's the same
string everywhere — database name, ACL user, key prefix. Keep it consistent.

| | ClickHouse | Valkey |
|--|-----------|--------|
| Isolation unit | a **database** per project | a **key/channel prefix** per service (`<svc>:*`) |
| Identity | an **ingest API key** per project (gateway writes as admin) | an **ACL user** locked to `~<svc>:* &<svc>:*` |
| Configured via | a key minted into `ingest.ingest_keys` (see ingest-api) | `VK_TENANTS` = `name:password` pairs |
| Provisioned | DB + `events` table auto-created by the gateway on first write | ACL file rebuilt from env on every boot |

> Valkey numbered DBs do **not** isolate tenants (Pub/Sub is global, no per-DB
> auth) — isolation is by prefix + ACL. See [valkey/README.md](valkey/README.md).
> ClickHouse keys/streams must follow the mandatory naming standard there.

---

## How an app stack connects to all three

**Logging goes through the ingest-api gateway only** — apps no longer get
ClickHouse credentials (the port is closed). For **broker / cache / locks** apps
still connect to Valkey directly with **tenant** credentials (never admin ones).
Once wired up, **drop that repo's own clickhouse/redis services**. Example for
`app_one` (substitute your deployment's IPs/ports and the passwords from
`VK_TENANTS`):

```dotenv
# ── Logging → ingest-api gateway (NOT direct ClickHouse) ──────────────
TELEMETRY_INGEST_URL="http://<infra-host>:46005/v1/events"
TELEMETRY_API_KEY="ik_…"           # one key → one tenant; mint via admin endpoint
# Events MUST conform to the standard schema below or they are rejected (400).

# ── Valkey (broker / cache / locks) ───────────────────────────────────
REDIS_HOST="<infra-host>"
REDIS_PORT="46004"
REDIS_USER="app_one"           # ACL username == key prefix
REDIS_PASSWORD="<from VK_TENANTS>"
# url form: redis://app_one:<pw>@<infra-host>:46004
# all keys/streams/channels MUST start with  app_one:
```

Onboarding a new project:

1. **Logging** — mint an ingest API key for the tenant (see below). The tenant's
   ClickHouse database + `events` table are created automatically on first write
   (the gateway writes as admin internally). Nothing else to configure.
2. **Valkey** (only if the app needs broker/cache/locks) — add a `name:password`
   pair to `VK_TENANTS` (that `name` becomes the mandatory key prefix).

(Use the **same** project name everywhere.) See each service's README for the
add-a-tenant-live commands when the stack is already running.

---

## Logging via the ingest API (recommended)

The direct-Valkey/ClickHouse wiring above couples each app repo to the infra
(it needs the `redis` + ClickHouse clients, a `logs-worker`, and tenant
credentials). For **logging**, prefer the `ingest-api` gateway instead: apps
hold **only an API key + a URL** and POST event batches. The queue and the
drain-to-ClickHouse worker live here, so app repos stay clean and publishable.

```
app ──POST /v1/events (Bearer <key>)──▶ ingest-api ──▶ Valkey ingest:events ──▶ (drain) ──▶ ClickHouse <tenant>.events
```

The app config is just:

```dotenv
TELEMETRY_INGEST_URL="http://<infra-host>:46005/v1/events"   # or ingest-api:8080 on-host
TELEMETRY_API_KEY="ik_…"                                     # one key → one tenant
```

**Onboarding a project for logging** (no Valkey tenant needed — the gateway is
the only Valkey client): mint a key, set two env vars, POST events.

```bash
curl -s -X POST http://<overlay-ip>:46005/v1/admin/keys \
  -H "x-admin-token: $INGEST_ADMIN_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"tenant":"app_three","label":"app-three prod"}'
# → {"id":"…","tenant":"app_three","key":"ik_…"}   ← shown once, store it
```

📖 **Full guide — HTTP API, event schema, key management, retention,
integration, ops & troubleshooting: [ingest-api/README.md](ingest-api/README.md).**

### Client side — just POST, do NOT add your own queue

The durable buffer lives **here**, in the gateway: each POST is written to the
gateway's Valkey stream (5M cap, survives a ClickHouse outage) and drained to
ClickHouse in the background. So an app must **not** stand up its own Redis queue
+ worker for logs — that just queues the same events twice:

```
✅ app ─POST─▶ ingest-api ─▶ Valkey ─▶ drain ─▶ ClickHouse        (one queue, here)
❌ app ─▶ app's Redis ─▶ app worker ─POST─▶ ingest-api ─▶ Valkey…  (two queues, redundant)
```

Do this in the app, smallest first:

- **Minimum:** fire-and-forget `POST /v1/events` with a batch, short timeout,
  drop on failure. No Redis, no worker.
- **Recommended:** an in-**memory** bounded buffer + one background task that
  flushes every ~1s or ~500 events as a single POST (≤ 1000/batch). Non-blocking
  on the request path; at most ~1s of in-flight logs lost if the process dies —
  fine for telemetry.

An app-side **durable** (Redis) queue is only warranted if losing a few seconds
of logs during a gateway/network blip is unacceptable — rare for telemetry, and
the right fix then is gateway HA, not a queue in every app. (Apps still use the
shared Valkey for their *real* broker/cache/lock needs — that's separate from
logging.)

### The event standard (enforced — no fallback)

Every event is validated against this schema **before** it is buffered. Any
violation rejects the **whole batch** with `400` and a per-event error list;
nothing off-spec ever reaches ClickHouse. The contract lives in
[`ingest-api/src/schema.rs`](ingest-api/src/schema.rs) and mirrors the `events`
table in [`ingest-api/src/ch.rs`](ingest-api/src/ch.rs).

- **Required** (non-empty strings): `category`, `event_type`.
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
cp .env.example .env        # then edit: strong passwords + bind IP + tenants
docker compose up -d
docker compose logs -f

# operate one service at a time when needed:
docker compose up -d clickhouse valkey
docker compose restart grafana
docker compose logs -f clickhouse
```

---

## Layout

```
frdcmp-infra/
├── README.md            ← you are here: the connection conventions
├── docker-compose.yml   ← the combined stack (all four services)
├── .env / .env.example  ← single env for the whole stack
├── clickhouse/          ← logging store: config.d/, init/, README, data dirs
├── valkey/              ← broker/cache/locks: valkey.conf, entrypoint.sh, README
├── ingest-api/          ← telemetry gateway (Rust): src/, Dockerfile, README
├── frontend/            ← admin dashboard (React+nginx, profile: dashboard), README
└── monitoring/          ← Prometheus + Grafana: prometheus.yml, grafana/, README
```

> **Admin dashboard (optional):** a React UI for API-key CRUD, cross-tenant log
> search, and docs, served by nginx and gated behind the `dashboard` compose
> profile. Start the stack with it via `docker compose --profile dashboard up -d`
> (default `127.0.0.1:46006`). Login is a single seeded admin (JWT) — set
> `JWT_SECRET` + `ADMIN_PASSWORD` in `.env`. See [frontend/README.md](frontend/README.md).

Secrets (`.env`) and data volumes (`*_data/`, `*_logs/`) are git-ignored — only
`.env.example` and config are tracked.
