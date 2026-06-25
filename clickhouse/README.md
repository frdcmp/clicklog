# decalitro-clickhouse

**Centralised logging ClickHouse for the decalitro project stacks.**

A single ClickHouse server that holds the log data for every project on
decalitro (`app_one`, `app_two`, and future stacks). Each project gets
its **own database** and its **own restricted user**; the per-project
`log_worker` processes create their tables and write into their own database
over HTTP.

This consolidates what used to be one ClickHouse instance per repo (e.g.
app_one ran its own ClickHouse on <infra-host>) into a single instance on one
host.

## Where it runs — `decalitro`

This is the **central logging host**. Everything connects in here.

| | |
|--|--|
| **Host** | `decalitro` |
| **the private overlay IP** | `172.25.3.220` (interface `ztbtowjpcj`) |
| **HTTP endpoint** | `http://172.25.3.220:46003` (published port `46003` → container `8123`) |
| **Repo path on host** | `~/docker/decalitro-clickhouse` |
| **SSH** | `ssh decalitro` |

The published port is bound to the the private overlay IP (`CLICKHOUSE_BIND=172.25.3.220`
in `.env`), so any stack on the the private overlay overlay can reach it; it is **not**
exposed on decalitro's public NIC (`176.123.2.138`).

```bash
# Operate it (from your machine):
ssh decalitro 'cd ~/docker/decalitro-clickhouse && docker compose ps'
ssh decalitro 'cd ~/docker/decalitro-clickhouse && docker compose logs -f clickhouse'

# Reach it over the private overlay from anywhere on the overlay:
curl -s "http://172.25.3.220:46003/?query=SHOW%20DATABASES" -u "admin:<admin-pw>"
```

```
  app_one log_worker ─┐
  app_two log_worker ─┤── HTTP :46003 ──▶  ┌──────────────────────────────────┐
  (future) …              ─┘   (the private overlay)        │  decalitro-clickhouse             │
                                                 │   ├── db `app_one`  (user …)  │
                                                 │   ├── db `app_two` (user …)  │
                                                 │   └── …                           │
                                                 └──────────────────────────────────┘
```

---

## Layout

| Path | Purpose |
|------|---------|
| `docker-compose.yml` | The single ClickHouse service. |
| `.env` / `.env.example` | Secrets + config (port, bind IP, tenants). `.env` is git-ignored. |
| `clickhouse/config.d/low-resources.xml` | Memory caps (cgroup-aware, 0.8× of `mem_limit`). |
| `clickhouse/config.d/network-and-logging.xml` | `listen_host`, log rotation, capped `system.*_log`. |
| `clickhouse/init/01-init-tenants.sh` | First-boot provisioning of tenant DBs + users from `CH_TENANTS`. |
| `clickhouse_data/` | Data directory (local volume under `./`, git-ignored). |
| `clickhouse_logs/` | Server logs (local, git-ignored). |

---

## Quick start

```bash
cp .env.example .env          # then edit: set strong passwords + CLICKHOUSE_BIND
docker compose up -d
docker compose logs -f clickhouse
```

First boot runs `01-init-tenants.sh`, which creates one database + user per
entry in `CH_TENANTS`. Verify:

```bash
source .env
curl -s "http://127.0.0.1:${CLICKHOUSE_EXT_PORT}/?query=SHOW%20DATABASES" \
  -u "${CLICKHOUSE_ADMIN_USER}:${CLICKHOUSE_ADMIN_PASSWORD}"
```

---

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `CLICKHOUSE_ADMIN_USER` / `CLICKHOUSE_ADMIN_PASSWORD` | Bootstrap admin (access management). Used by the init script and healthcheck. |
| `CLICKHOUSE_EXT_PORT` | External HTTP port (a single random high port; default `46003`). Maps to container `8123`. |
| `CLICKHOUSE_BIND` | Interface the published port binds to. `127.0.0.1` by default. **On decalitro set the the private overlay IP** `172.25.3.220`. Never a public interface. |
| `CLICKHOUSE_MEM_LIMIT` / `CLICKHOUSE_CPUS` | Container resource caps. ClickHouse self-caps memory at 0.8× `MEM_LIMIT`. |
| `CH_TENANTS` | Space-separated `db:user:password` triples — one per project. |

### Networking / security model

- Only the **HTTP** port (8123) is published — on **one** port, bound to
  `CLICKHOUSE_BIND`. The native TCP protocol (9000) stays internal to the
  container network.
- Inside the container ClickHouse listens on `0.0.0.0` so Docker can forward to
  it; the only reachable surface is the single published port.
- In production, `CLICKHOUSE_BIND` is decalitro's **the private overlay** IP so the project
  stacks reach it over the private overlay — never the public NIC.

---

## How the per-project stacks connect

Point each stack's existing ClickHouse env at this server. Use the **tenant**
credentials, not the admin ones, and **disable that repo's own `clickhouse`
service** once it points here.

**app_one** — note the two binaries read **different** port vars: the main
API uses `CLICKHOUSE_PORT`, the `log_worker` uses `CLICKHOUSE_HTTP_PORT` (so set
both). In app_one's `.env`:
```dotenv
CLICKHOUSE_HOST="172.25.3.220"     # decalitro the private overlay
CLICKHOUSE_PORT="46003"            # main API (admin log queries)
CLICKHOUSE_HTTP_PORT="46003"       # log_worker
CLICKHOUSE_DB="app_one"
CLICKHOUSE_USER="app_one"
CLICKHOUSE_PASSWORD="<app_one password from CH_TENANTS>"
```
Then drop the `clickhouse` service from app_one's `docker-compose.yml` (or
stop it) — it no longer runs its own ClickHouse on <infra-host>.

**app_two** — same idea, with its own tenant:
```dotenv
CLICKHOUSE_HOST="172.25.3.220"
CLICKHOUSE_PORT="46003"
CLICKHOUSE_HTTP_PORT="46003"
CLICKHOUSE_DB="app_two"
CLICKHOUSE_USER="app_two"
CLICKHOUSE_PASSWORD="<app_two password from CH_TENANTS>"
```

Each worker issues `CREATE DATABASE IF NOT EXISTS` + `CREATE TABLE …` on boot;
the tenant user is granted exactly that on its own database.

> If a stack runs **on decalitro itself**, `172.25.3.220:46003` still works
> locally — no need for a separate localhost wiring.

---

## Adding a new tenant

The init script only runs on a **fresh** data dir. Two cases:

**A) Before first boot** — just add a triple to `CH_TENANTS` in `.env` and
`docker compose up -d`.

**B) Already running** — add the triple to `CH_TENANTS` (so it survives a future
re-init) *and* create it live:

```bash
source .env
docker compose exec clickhouse clickhouse-client \
  --user "$CLICKHOUSE_ADMIN_USER" --password "$CLICKHOUSE_ADMIN_PASSWORD" -n -q "
    CREATE DATABASE IF NOT EXISTS \`newsys\`;
    CREATE USER IF NOT EXISTS \`newsys\` IDENTIFIED WITH sha256_password BY 'STRONG_PW' DEFAULT DATABASE \`newsys\`;
    GRANT ALL ON \`newsys\`.* TO \`newsys\`;
    GRANT CREATE DATABASE ON \`newsys\`.* TO \`newsys\`;
"
```

---

## Operations

```bash
docker compose ps                 # status + health
docker compose logs -f clickhouse # server logs
docker compose down               # stop (data persists in ./clickhouse_data)
docker compose down && sudo rm -rf clickhouse_data   # full reset (re-runs init)
```

Disk usage by table:
```sql
SELECT database, table, formatReadableSize(sum(bytes_on_disk)) AS size
FROM system.parts WHERE active GROUP BY database, table ORDER BY sum(bytes_on_disk) DESC;
```

---

## Migrating data from the old per-repo instances

app_one's existing logs live in its own ClickHouse on <infra-host>
(`192.168.1.62:8169`, db `app_one`). To carry history over into the central
instance, copy per table with an `INSERT … SELECT FORMAT Native` pipe:

```bash
# Example: copy one table from the old <infra-host> CH to the central decalitro CH.
clickhouse-client --host 192.168.1.62 --port OLD_TCP_PORT --query \
  "SELECT * FROM app_one.page_visits FORMAT Native" \
| clickhouse-client --host 172.25.3.220 --port 46003 --user app_one --password '…' \
  --query "INSERT INTO app_one.page_visits FORMAT Native"
```

(Or use the HTTP endpoints with `curl` if the native TCP port isn't exposed.)
If history isn't needed, just let the workers create empty tables on first
connect and retire the old instances.
