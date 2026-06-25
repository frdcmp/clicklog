# valkey

**Shared Valkey broker for all frdcmp project stacks.**

A single Valkey (open-source Redis fork) server acting as the central **message
broker** (Redis Streams + Pub/Sub), **cache**, and **lock** store for every
project — `app_one`, `app_two`, and any number of future services.

Each service gets its **own ACL user**, locked to its **own key/channel
prefix**. There is one shared keyspace; isolation is by **naming convention +
ACL**, not by database (Valkey's numbered DBs do *not* isolate — see below).

## Where it runs

Part of the combined stack at the repo root — brought up by the top-level
`docker-compose.yml` alongside `clickhouse`, `prometheus`, and `grafana`. Run all
`docker compose` commands **from the repo root** (one service at a time with
e.g. `docker compose up -d valkey`). What it binds to and which port it
publishes are set in the single root `.env`.

| | |
|--|--|
| **Endpoint** | `<VK_BIND>:<VK_EXT_PORT>` (published port → container `6379`) |
| **Bind** | `VK_BIND` — the interface the published port binds to |
| **Default port** | `46004` (`VK_EXT_PORT`) |

Below, the deployed endpoint is written `<infra-host>:46004` — substitute your
`VK_BIND:VK_EXT_PORT`.

Bind to a **private overlay** IP (these stacks use the private overlay) so the project
stacks reach it over the private network — **never** a public NIC. Auth is
enforced by ACL passwords.

```
  app_one  ─┐                          ┌────────────────────────────────────┐
  app_two ─┤── :46004 (overlay) ────▶ │  valkey                             │
  (future) …   ─┘   AUTH <user> <pw>       │   user app_one → ~app_one:* │
                                           │   user app_two → ~app_two:*│
                                           │   (one shared keyspace, ACL-scoped) │
                                           └────────────────────────────────────┘
```

---

## ⚠️ Why not "one DB per service" (like ClickHouse)?

Valkey/Redis has numbered logical DBs (`SELECT 0–15`), but they are **not**
tenant boundaries:

- **No per-DB auth** — ACL users are global; you can't restrict a user to a DB.
- **Pub/Sub is global** — `SUBSCRIBE` in any DB receives `PUBLISH` from every DB.
  Fatal for a shared broker.
- Legacy feature; unsupported in Cluster mode.

So isolation is done the Valkey-native way: **prefix every key/stream/channel
with the service name**, and lock each service's ACL user to that prefix.

---

# 🔑 Key & channel naming standard (MANDATORY)

> **Every key, stream, and pub/sub channel a service puts in Valkey MUST begin
> with `<service>:`** — where `<service>` is the service's tenant name (its ACL
> username). The ACL *enforces* this: a service literally cannot read or write
> anything outside its own prefix.

### Structure

```
<service>:<type>:<name>[:<sub>...]
```

| Segment | Meaning | Example |
|---------|---------|---------|
| `<service>` | Tenant id = ACL username = prefix | `app_one` |
| `<type>` | Object kind (controlled vocabulary below) | `stream` |
| `<name>` | Logical name (kebab/snake) | `user_interactions` |
| `<sub>` | Optional further qualifier / id | `01J9…` |

### Controlled `<type>` vocabulary

| `<type>` | Use for | Persistence | Example |
|----------|---------|-------------|---------|
| `stream` | Redis Streams — durable event logs & work queues (consumer groups) | durable (AOF) | `app_one:stream:user_interactions` |
| `channel` | Pub/Sub channels — ephemeral fan-out (WebSockets, live notifications) | **none** | `app_one:channel:ws:orders` |
| `queue` | List-based job queues (`LPUSH`/`BRPOP`) | durable | `app_one:queue:email` |
| `cache` | Cached values — **MUST set a TTL** | TTL'd | `app_one:cache:product:01J9…` |
| `lock` | Distributed locks (`SET NX PX`) | TTL'd | `app_one:lock:cron:nightly` |
| `session` | Sessions / presence | TTL'd | `app_one:session:<uid>` |
| `ratelimit` | Rate-limit counters | TTL'd | `app_one:ratelimit:ip:1.2.3.4` |
| `set` / `hash` / `zset` | Generic structures when no better `<type>` fits | as needed | `app_one:zset:leaderboard` |

### Rules

1. **Prefix is non-negotiable** — the ACL rejects anything outside `<service>:*`.
   No bare `streams:foo`, no shared keys between services.
2. **Channels** (Pub/Sub) are scoped by the ACL `&<service>:*` rule — same
   prefix rule applies, and they're **ephemeral** (use `stream` for anything
   that must survive a restart or a disconnected consumer).
3. **Cache keys MUST have a TTL** (`EX`/`PX`). The instance runs
   `maxmemory-policy noeviction`, so untracked growth eventually **blocks
   writes** for everyone rather than evicting.
4. **Bound your streams** — trim with `XADD … MAXLEN ~ N` or have the worker
   `XACK` + `XTRIM`, so a stream can't grow without limit.
5. **Consumer groups** live inside a stream key, so they're auto-scoped; name
   them descriptively (e.g. `group_logs`).
6. New service → add a `name:password` pair to `VK_TENANTS`, and that `name`
   becomes the required prefix.

### app_one migration map (current → standard)

app_one currently uses **unprefixed** keys; moving onto this broker means
renaming them (a code change in the app + workers):

| Current | Standard |
|---------|----------|
| `streams:user_interactions` | `app_one:stream:user_interactions` |
| `streams:page_visits` | `app_one:stream:page_visits` |
| `streams:db_writes` | `app_one:stream:db_writes` |
| `streams:email_events` | `app_one:stream:email_events` |
| `streams:scryfall_sources` | `app_one:stream:scryfall_sources` |
| `email:send` | `app_one:stream:email_send` (or `:queue:email`) |
| WebSocket pub/sub channels | `app_one:channel:…` |
| ban / notification / cache keys | `app_one:cache:…` / `app_one:…` |

---

## Quick start

From the **repo root** (one root `.env` covers every service):

```bash
cp .env.example .env          # set strong passwords + VK_BIND + VK_TENANTS
docker compose up -d valkey
docker compose logs -f valkey
```

On boot, `valkey/entrypoint.sh` builds the ACL file from `VK_TENANTS`. Verify:

```bash
source .env
# admin:
valkey-cli -h 127.0.0.1 -p "$VK_EXT_PORT" --no-auth-warning -a "$VK_ADMIN_PASSWORD" ACL LIST
# a tenant (note username + password):
valkey-cli -h 127.0.0.1 -p "$VK_EXT_PORT" --user app_one --pass '<app_one pw>' --no-auth-warning PING
```

---

## Configuration (`.env`)

| Var | Meaning |
|-----|---------|
| `VK_ADMIN_PASSWORD` | Password for the `default` (admin) ACL user. Healthcheck + ops. |
| `VK_EXT_PORT` | External port (default `46004`) → container `6379`. |
| `VK_BIND` | Interface the published port binds to. `127.0.0.1` default; **the host's private-overlay IP** in production. Never public. |
| `VK_MEM_LIMIT` / `VK_MAXMEMORY` / `VK_CPUS` | Container caps. `VK_MAXMEMORY` (e.g. `768mb`) must stay below `VK_MEM_LIMIT`. |
| `VK_TENANTS` | Space-separated `name:password` pairs — one per service. `name` = ACL user = key/channel prefix. |

### Security model

- Only the Valkey port is published — one port, bound to `VK_BIND`.
- `protected-mode no` inside the container is safe: the sole reachable surface
  is the published port (overlay-bound) and every user needs an ACL password.
- Each tenant user has `+@all -@dangerous`, so no `FLUSHALL`/`FLUSHDB`/`CONFIG`/
  `SHUTDOWN` — important since all tenants share one keyspace.

---

## How a service connects

Use the **tenant** credentials (username + password), and prefix all keys.

**app_one** — its `redis` client URL takes an ACL username:
```dotenv
REDIS_HOST="<infra-host>"          # the VK_BIND IP of wherever this runs
REDIS_PORT="46004"
REDIS_USER="app_one"
REDIS_PASSWORD="<app_one password from VK_TENANTS>"
# URL form: redis://app_one:<pw>@<infra-host>:46004
```
Then rename its keys/streams/channels per the migration map above, and drop the
`redis` service from app_one's own compose once it points here.

> A stack running **on the same host** as this server can use the same
> `<infra-host>:46004` endpoint directly.

---

## Adding a new service (tenant)

1. Add `name:password` to `VK_TENANTS` in `.env`.
2. Recreate so the ACL regenerates: `docker compose up -d --force-recreate valkey`
   *(ACLs are rebuilt from env on every boot; no data is lost — it's in `valkey/valkey_data`)*.
   Or add it live without a restart:
   ```bash
   source .env
   docker compose exec valkey valkey-cli --no-auth-warning -a "$VK_ADMIN_PASSWORD" \
     ACL SETUSER newsvc on '>STRONG_PW' resetchannels '~newsvc:*' '&newsvc:*' +@all -@dangerous
   # (still add it to VK_TENANTS so it survives the next recreate)
   ```
3. That service must prefix everything with `newsvc:`.

---

## Operations

```bash
docker compose ps                              # status (whole stack)
docker compose logs -f valkey
docker compose stop valkey                     # stop just this service (data persists in valkey/valkey_data)
docker compose up -d --force-recreate valkey   # reload after editing VK_TENANTS
docker compose rm -sf valkey && sudo rm -rf valkey/valkey_data   # full wipe

# memory / keyspace:
source .env
valkey-cli -h 127.0.0.1 -p "$VK_EXT_PORT" --no-auth-warning -a "$VK_ADMIN_PASSWORD" INFO memory
valkey-cli -h 127.0.0.1 -p "$VK_EXT_PORT" --no-auth-warning -a "$VK_ADMIN_PASSWORD" --scan --pattern 'app_one:*' | head
```
