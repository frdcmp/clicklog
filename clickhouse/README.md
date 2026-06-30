# clickhouse

**Centralised logging ClickHouse for the frdcmp project stacks.**

A single ClickHouse server that holds the log data for every project
(`app_one`, `app_two`, and future stacks). Each project gets its **own
database** and its **own restricted user**; the per-project `log_worker`
processes create their tables and write into their own database over HTTP.

This consolidates what used to be one ClickHouse instance per repo into a single
shared instance.

## Where it runs

Part of the combined stack at the repo root — brought up by the top-level
`docker-compose.yml` alongside `valkey`, `prometheus`, and `grafana`. Run all
`docker compose` commands **from the repo root** (one service at a time with
e.g. `docker compose up -d clickhouse`). What it binds to and which port it
publishes are set in the single root `.env`.

| | |
|--|--|
| **HTTP endpoint** | `http://<CLICKHOUSE_BIND>:<CLICKHOUSE_EXT_PORT>` (published port → container `8123`) |
| **Bind** | `CLICKHOUSE_BIND` — the interface the published port binds to |
| **Default port** | `46003` (`CLICKHOUSE_EXT_PORT`) |

Below, the deployed endpoint is written `<infra-host>:46003` — substitute your
`CLICKHOUSE_BIND:CLICKHOUSE_EXT_PORT`.

Bind to a **private overlay** IP (these stacks use the private overlay) so the project
stacks reach it over the private network — **never** a public NIC.

```bash
# Operate it:
docker compose ps
docker compose logs -f clickhouse

# Reach it over the overlay from anywhere:
curl -s "http://<infra-host>:46003/?query=SHOW%20DATABASES" -u "admin:<admin-pw>"
```

```
  app_one log_worker ─┐
  app_two log_worker ─┤── HTTP :46003 ──▶  ┌──────────────────────────────────┐
  (future) …              ─┘  (private overlay) │  clickhouse                       │
                                                 │   ├── db `app_one`  (user …)  │
                                                 │   ├── db `app_two` (user …)  │
                                                 │   └── …                           │
                                                 └──────────────────────────────────┘
```

---

## Layout

All paths below are relative to this `clickhouse/` folder (the compose file and
`.env` live one level up at the repo root).

| Path | Purpose |
|------|---------|
| `../docker-compose.yml` | Defines the `clickhouse` service (combined stack). |
| `../.env` / `../.env.example` | Secrets + config for the whole stack. `.env` is git-ignored. |
| `clickhouse/config.d/low-resources.xml` | Memory caps (cgroup-aware, 0.8× of `mem_limit`). |
| `clickhouse/config.d/network-and-logging.xml` | `listen_host`, log rotation, capped `system.*_log`. |
| `clickhouse_data/` | Data directory (local volume, git-ignored). |
| `clickhouse_logs/` | Server logs (local, git-ignored). |

---

## Quick start

From the **repo root** (one root `.env` covers every service):

```bash
cp .env.example .env          # then edit: set strong passwords + CLICKHOUSE_BIND
docker compose up -d clickhouse
docker compose logs -f clickhouse
```

Tenant databases are created on demand by the ingest-api gateway on first write
— there's no first-boot provisioning step. Verify the server is up:

```bash
source .env
curl -s "http://127.0.0.1:${CLICKHOUSE_EXT_PORT}/?query=SHOW%20DATABASES" \
  -u "${CLICKHOUSE_ADMIN_USER}:${CLICKHOUSE_ADMIN_PASSWORD}"
```

---

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `CLICKHOUSE_ADMIN_USER` / `CLICKHOUSE_ADMIN_PASSWORD` | Bootstrap admin (access management). Used by the ingest-api gateway and the healthcheck. |
| `CLICKHOUSE_EXT_PORT` | External HTTP port (a single random high port; default `46003`). Maps to container `8123`. |
| `CLICKHOUSE_BIND` | Interface the published port binds to. `127.0.0.1` by default. **In production set the private-overlay IP** of the host you deploy on. Never a public interface. |
| `CLICKHOUSE_MEM_LIMIT` / `CLICKHOUSE_CPUS` | Container resource caps. ClickHouse self-caps memory at 0.8× `MEM_LIMIT`. |

### Networking / security model

- Only the **HTTP** port (8123) is published — on **one** port, bound to
  `CLICKHOUSE_BIND`. The native TCP protocol (9000) stays internal to the
  container network.
- Inside the container ClickHouse listens on `0.0.0.0` so Docker can forward to
  it; the only reachable surface is the single published port.
- In production, `CLICKHOUSE_BIND` is the host's **private-overlay** IP so the
  project stacks reach it over the private network — never the public NIC.

---

## How the per-project stacks connect

Projects do **not** connect to ClickHouse directly. They POST their telemetry to
the **ingest-api** gateway with a per-project ingest key; the gateway is the only
writer (it connects here as the admin user) and ClickHouse is reachable only on
the internal `frdcmp` network. Each app sets just two values in its own `.env`:

```dotenv
TELEMETRY_INGEST_URL="http://<infra-host>:<ingest-port>/v1/events"
TELEMETRY_API_KEY="ik_…"   # minted per project — see ingest-api/README.md
```

The `<tenant>.events` table is created automatically on the first insert. See
[../ingest-api/README.md](../ingest-api/README.md) for minting/revoking keys.

---

## Adding a new tenant

No ClickHouse-side step is needed — mint an ingest key for the project (see
[../ingest-api/README.md](../ingest-api/README.md)) and its database + `events`
table are created on the first write.

---

## Operations

```bash
docker compose ps                 # status + health (whole stack)
docker compose logs -f clickhouse # server logs
docker compose stop clickhouse    # stop just this service (data persists in clickhouse/clickhouse_data)
docker compose rm -sf clickhouse && sudo rm -rf clickhouse/clickhouse_data   # full reset (wipes all tenant data)
```

Disk usage by table:
```sql
SELECT database, table, formatReadableSize(sum(bytes_on_disk)) AS size
FROM system.parts WHERE active GROUP BY database, table ORDER BY sum(bytes_on_disk) DESC;
```

---

## Migrating data from an old per-repo instance

If a project previously ran its own ClickHouse, carry history over per table with
an `INSERT … SELECT FORMAT Native` pipe:

```bash
# Copy one table from the old CH to this central CH.
clickhouse-client --host <old-host> --port <old-tcp-port> --query \
  "SELECT * FROM app_one.page_visits FORMAT Native" \
| clickhouse-client --host <infra-host> --port 46003 --user "$CLICKHOUSE_ADMIN_USER" --password '…' \
  --query "INSERT INTO app_one.page_visits FORMAT Native"
```

(Or use the HTTP endpoints with `curl` if the native TCP port isn't exposed.)
If history isn't needed, just let the workers create empty tables on first
connect and retire the old instance.
