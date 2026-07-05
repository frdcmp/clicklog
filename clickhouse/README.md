# clickhouse

**The log store. Internal-only — written and read via the ingest-api gateway.**

A single ClickHouse server holding the log data for every project. Each
project gets its **own database** with an `events` table, created on demand by
the `ingest-api` gateway on the first write for that tenant.

```
ingest-api ──HTTP (clickhouse:8123, internal)──▶ ┌─────────────────────────┐
   (the only client)                             │  clickhouse              │
                                                 │   ├── db `app_one`       │
                                                 │   ├── db `app_two`       │
                                                 │   └── …  (one/tenant)    │
                                                 └─────────────────────────┘
```

| | |
|--|--|
| **Endpoint** | `clickhouse:8123` (HTTP) — internal `clicklog` docker network **only** |
| **Published port** | **none** |
| **Auth** | **none** — network isolation is the access control; only `ingest-api` can reach it |
| **Data** | `./clickhouse_data/` (bind mount) |

Apps **cannot** connect here. They POST to the ingest-api gateway with a
per-project key, and read back via the gateway's `GET /v1/events` / `/v1/stats`.
Tenant isolation is enforced by the gateway (one API key → one database), not by
ClickHouse users.

## Where it runs

Part of the combined stack at the repo root — brought up by the top-level
`docker-compose.yml`. Run all `docker compose` commands **from the repo root**.

## Layout

All paths below are relative to this `clickhouse/` folder (the compose file and
`.env` live one level up at the repo root).

| Path | Purpose |
|------|---------|
| `../docker-compose.yml` | Defines the `clickhouse` service (combined stack). |
| `../.env` / `../.env.example` | Config for the whole stack. `.env` is git-ignored. |
| `clickhouse/config.d/low-resources.xml` | Memory caps (cgroup-aware, 0.8× of `mem_limit`). |
| `clickhouse/config.d/network-and-logging.xml` | `listen_host`, log rotation, capped `system.*_log`. |
| `clickhouse_data/` | Data directory (local volume, git-ignored). |
| `clickhouse_logs/` | Server logs (local, git-ignored). |

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `CLICKHOUSE_MEM_LIMIT` / `CLICKHOUSE_CPUS` | Container resource caps. ClickHouse self-caps memory at 0.8× `MEM_LIMIT`. |

No credentials, no ports — there is nothing else to configure. Tenant databases
are created on demand by the gateway; there's no first-boot provisioning step.

### Security model

- **No published port, no password.** The only reachable surface is the
  internal `clicklog` docker network — in practice only the `ingest-api`
  container. Both HTTP (8123) and native TCP (9000) stay internal.
- All external access (write *and* read) goes through the gateway, which
  authenticates apps with API keys and scopes every query to the key's tenant.

## Operations

```bash
docker compose ps                 # status + health (whole stack)
docker compose logs -f clickhouse # server logs
docker compose stop clickhouse    # stop just this service (data persists in clickhouse/clickhouse_data)
docker compose rm -sf clickhouse && sudo rm -rf clickhouse/clickhouse_data   # full reset (wipes all tenant data)
```

Ad-hoc SQL — from the infra host, through the container:

```bash
docker compose exec clickhouse clickhouse-client -q "SHOW DATABASES"
```

Disk usage by table:
```sql
SELECT database, table, formatReadableSize(sum(bytes_on_disk)) AS size
FROM system.parts WHERE active GROUP BY database, table ORDER BY sum(bytes_on_disk) DESC;
```

## Migrating data from an old per-repo instance

If a project previously ran its own ClickHouse, carry history over per table by
piping `FORMAT Native` through the container:

```bash
# Copy one table from the old CH into this one (run on the infra host).
clickhouse-client --host <old-host> --port <old-tcp-port> --query \
  "SELECT * FROM <tenant>.<table> FORMAT Native" \
| docker compose exec -T clickhouse clickhouse-client \
  --query "INSERT INTO <tenant>.<table> FORMAT Native"
```

If history isn't needed, just start POSTing — the gateway creates empty tables
on the first write and the old instance can be retired.
