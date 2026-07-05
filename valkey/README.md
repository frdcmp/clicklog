# valkey

**Internal log queue for the ingest-api. Not a shared broker.**

A single Valkey (open-source Redis fork) server whose only job is to buffer
accepted events between the `ingest-api` gateway and ClickHouse:

```
ingest-api ──XADD──▶ valkey  ingest:events  ──XREADGROUP (drain)──▶ ClickHouse
```

The gateway is the **only client**. There are no tenants, no ACL users, no
published host port — nothing else connects here, by design. Apps never talk to
Valkey; they `POST /v1/events` to the gateway (see
[ingest-api/README.md](../ingest-api/README.md)).

## Where it runs

Part of the combined stack at the repo root — brought up by the top-level
`docker-compose.yml`. Run all `docker compose` commands **from the repo root**.

| | |
|--|--|
| **Endpoint** | `valkey:6379` — internal `clicklog` docker network **only** |
| **Published port** | **none** |
| **Auth** | **none** — network isolation is the access control |
| **Data** | `./valkey_data/` (bind mount; AOF + RDB snapshots) |

## What lives in it

| Key | Type | Purpose |
|-----|------|---------|
| `ingest:events` | stream | Durable buffer of accepted event batches, capped by the gateway (`XADD … MAXLEN`). Survives a ClickHouse outage; the drain task flushes it when ClickHouse recovers. |

That's the entire keyspace. If you see anything else in here, something is
wrong.

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `VK_MEM_LIMIT` / `VK_MAXMEMORY` / `VK_CPUS` | Container caps. `VK_MAXMEMORY` (e.g. `768mb`) must stay below `VK_MEM_LIMIT`. |

No credentials — there is nothing else to configure.

### Security model

- **No published port, no password.** The only reachable surface is the
  internal `clicklog` docker network — in practice only the `ingest-api`
  container. Network isolation is the access control; adding a password would
  only guard against the stack's own containers.
- `maxmemory-policy noeviction`: a full instance errors on write instead of
  silently dropping queued events; the capped stream keeps that from happening.

## Operations

```bash
docker compose ps                              # status (whole stack)
docker compose logs -f valkey
docker compose stop valkey                     # data persists in valkey/valkey_data
docker compose rm -sf valkey && sudo rm -rf valkey/valkey_data   # full wipe (queue only — logs already drained to ClickHouse are safe)

# inspect the queue:
docker compose exec valkey valkey-cli XLEN ingest:events
docker compose exec valkey valkey-cli INFO memory
```
