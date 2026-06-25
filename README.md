# frdcmp-infra

**Shared infrastructure services for the frdcmp project stacks.**

The backing services that every app stack (`app_one`, `app_two`, and
future projects) plugs into, kept in one repo so the conventions for connecting
to them live in one place:

| Service | Role | Container port | Default host port | Docs |
|---------|------|----------------|-------------------|------|
| [`clickhouse/`](clickhouse/) | Centralised **logging** store — one database + restricted user per project | `8123` (HTTP) | `46003` | [README](clickhouse/README.md) |
| [`valkey/`](valkey/) | Shared **broker / cache / locks** (Redis Streams + Pub/Sub) — one ACL user + key prefix per service | `6379` | `46004` | [README](valkey/README.md) |
| [`monitoring/`](monitoring/) | **Observability** — Prometheus scrape + Grafana dashboards | `9090` / `3000` | `9090` / `3001` | [README](monitoring/README.md) |

Each service is a **self-contained Docker Compose stack** with its own
`.env`, data volume, and README. They are independent — deploy all three on one
box or spread them across hosts; nothing here assumes a particular machine.

---

## Deployment topology (fill in per environment)

These services are **host-agnostic**. Where each one runs, what interface it
binds to, and which port it publishes are all set in that service's `.env` —
not baked into the repo. Record your actual layout here.

| Service | Host | Bind interface | Endpoint | `.env` knobs |
|---------|------|----------------|----------|--------------|
| clickhouse | _your host_ | _overlay IP_ | `http://<ip>:46003` | `CLICKHOUSE_BIND`, `CLICKHOUSE_EXT_PORT` |
| valkey | _your host_ | _overlay IP_ | `<ip>:46004` | `VK_BIND`, `VK_EXT_PORT` |
| monitoring | _your host_ | _host_ | Grafana `http://<ip>:3001`, Prom `:9090` | `GRAFANA_ROOT_URL`, `*_EXT_PORT` |

**Networking & security model (shared by all three):**

- Each service publishes **one** port, bound to whatever interface you set
  (`*_BIND`). Put them on a **private overlay** (the stacks here use the private overlay)
  and **never** bind to a public NIC.
- Auth is always on: ClickHouse per-tenant users, Valkey per-tenant ACL
  passwords, Grafana admin login. The native/internal protocols (ClickHouse
  TCP `9000`) stay inside the container network.
- A stack co-located on the same host still connects via the same overlay
  IP:port — no separate localhost wiring needed.

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

## Quick start

Each service is brought up independently:

```bash
# per service: clickhouse / valkey / monitoring
cd <service>
cp .env.example .env        # then edit: strong passwords + bind IP + tenants
docker compose up -d
docker compose logs -f
```

There is no top-level compose — start only the services you need, on whichever
host they belong to.

---

## Layout

```
frdcmp-infra/
├── README.md            ← you are here: the connection conventions
├── clickhouse/          ← logging store (self-contained compose stack)
├── valkey/              ← broker/cache/locks (self-contained compose stack)
└── monitoring/          ← Prometheus + Grafana (self-contained compose stack)
```

Secrets (`.env`) and data volumes (`*_data/`, `*_logs/`) are git-ignored — only
`.env.example` and config are tracked.
