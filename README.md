# frdcmp-infra

**Shared infrastructure services for the frdcmp project stacks.**

The backing services that every app stack (`app_one`, `app_two`, and
future projects) plugs into, kept in one repo so the conventions for connecting
to them live in one place:

| Service | Role | Container port | Default host port | Docs |
|---------|------|----------------|-------------------|------|
| `clickhouse` | Centralised **logging** store — one database + restricted user per project | `8123` (HTTP) | `46003` | [README](clickhouse/README.md) |
| `valkey` | Shared **broker / cache / locks** (Redis Streams + Pub/Sub) — one ACL user + key prefix per service | `6379` | `46004` | [README](valkey/README.md) |
| `prometheus` + `grafana` | **Observability** — Prometheus scrape + Grafana dashboards | `9090` / `3000` | `9090` / `3001` | [README](monitoring/README.md) |

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
| Identity | a restricted **user** granted only its own DB | an **ACL user** locked to `~<svc>:* &<svc>:*` |
| Configured via | `CH_TENANTS` = `db:user:password` triples | `VK_TENANTS` = `name:password` pairs |
| Provisioned | init script on first boot (+ live `CREATE` to add later) | ACL file rebuilt from env on every boot |

> Valkey numbered DBs do **not** isolate tenants (Pub/Sub is global, no per-DB
> auth) — isolation is by prefix + ACL. See [valkey/README.md](valkey/README.md).
> ClickHouse keys/streams must follow the mandatory naming standard there.

---

## How an app stack connects to all three

Point the stack's env at the endpoints, use **tenant** credentials (never the
admin ones), and once it's wired up, **drop that repo's own clickhouse/redis
services** so it stops running its own. Example for `app_one`
(substitute your deployment's IPs/ports and the passwords from each
`*_TENANTS`):

```dotenv
# ── ClickHouse (logging) ──────────────────────────────────────────────
CLICKHOUSE_HOST="<infra-host>"
CLICKHOUSE_PORT="46003"            # main API
CLICKHOUSE_HTTP_PORT="46003"       # log_worker (app_one reads a separate var)
CLICKHOUSE_DB="app_one"
CLICKHOUSE_USER="app_one"
CLICKHOUSE_PASSWORD="<from CH_TENANTS>"

# ── Valkey (broker / cache / locks) ───────────────────────────────────
REDIS_HOST="<infra-host>"
REDIS_PORT="46004"
REDIS_USER="app_one"           # ACL username == key prefix
REDIS_PASSWORD="<from VK_TENANTS>"
# url form: redis://app_one:<pw>@<infra-host>:46004
# all keys/streams/channels MUST start with  app_one:

# ── Metrics (scraped, not pushed) ─────────────────────────────────────
# Expose /metrics on your API; add the target to monitoring/prometheus/prometheus.yml.
```

Onboarding a new project = add a tenant to **both** stores + a scrape target:

1. **ClickHouse** — add a `db:user:password` triple to `CH_TENANTS`.
2. **Valkey** — add a `name:password` pair to `VK_TENANTS` (that `name` becomes
   the mandatory key prefix).
3. **Monitoring** — add the API's `/metrics` target to
   [monitoring/prometheus/prometheus.yml](monitoring/prometheus/prometheus.yml).

(Use the **same** project name in all three.) See each service's README for the
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
curl -s -X POST http://172.25.125.233:46005/v1/admin/keys \
  -H "x-admin-token: $INGEST_ADMIN_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"tenant":"app_three","label":"app-three prod"}'
# → {"id":"…","tenant":"app_three","key":"ik_…"}   ← shown once, store it
```

📖 **Full guide — HTTP API, event schema, key management, retention,
integration, ops & troubleshooting: [ingest-api/README.md](ingest-api/README.md).**

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
└── monitoring/          ← Prometheus + Grafana: prometheus.yml, grafana/, README
```

Secrets (`.env`) and data volumes (`*_data/`, `*_logs/`) are git-ignored — only
`.env.example` and config are tracked.
