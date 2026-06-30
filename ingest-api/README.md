# ingest-api — telemetry ingest gateway

A small Rust/Actix service that accepts batches of **events** over HTTP, buffers
them on Valkey, and drains them into per-tenant **ClickHouse** `events` tables.

Apps that want logging hold **only a URL + an API key** — no ClickHouse or
Valkey client, no credentials, no worker. One API key maps to one **tenant**
(its own ClickHouse database). This keeps app repos clean and publishable; all
the storage, batching, retries and retention live here.

```
your app ──POST /v1/events (Bearer <key>)──▶ ingest-api ──XADD──▶ Valkey ingest:events
                                                                      │  (drain task)
                                                                      ▼
                                                       ClickHouse  <tenant>.events
        └──────────────────────── all inside frdcmp-infra (<infra-host>) ────────────┘
```

- **Endpoint (prod):** `http://172.25.125.233:46005` over the the private overlay overlay.
  On the same host, use the internal name `http://ingest-api:8080`.
- The hop is plain HTTP, but the private overlay encrypts the overlay, and the port binds
  to the overlay IP only (never a public NIC). The API key is the second layer.

---

## 1. Connect an app (TL;DR)

1. **Mint a key** for your tenant (see §4). You get an `ik_…` string, shown once.
2. **Set two env vars** in the app:
   ```dotenv
   TELEMETRY_INGEST_URL="http://172.25.125.233:46005/v1/events"
   TELEMETRY_API_KEY="ik_…"
   ```
   On the same host as the infra you may use `http://ingest-api:8080/v1/events`.
3. **POST event batches** to that URL with `Authorization: Bearer $TELEMETRY_API_KEY`.

That's it. The tenant's `events` table is created automatically on first insert.
If `TELEMETRY_INGEST_URL` is unset, an app should simply not send anything
(telemetry off) — the gateway is never a hard dependency.

Quick smoke test:
```bash
curl -s -X POST http://172.25.125.233:46005/v1/events \
  -H "Authorization: Bearer $TELEMETRY_API_KEY" \
  -H 'content-type: application/json' \
  -d '[{"category":"test","event_type":"smoke","severity":"info","message":"hello"}]'
# → {"accepted":1}
```

---

## 2. HTTP API

### `GET /health`
Liveness probe. → `200 {"status":"ok"}`.

### `POST /v1/events`
Ingest a batch of events.

- **Auth:** `Authorization: Bearer <key>` **or** `x-api-key: <key>`. Invalid/unknown/
  revoked key → `401`.
- **Body** (any of):
  - a JSON **array** of event objects: `[{...}, {...}]`
  - a single JSON **object**: `{...}`
  - **NDJSON**: one JSON object per line.
- **Limits:** ≤ **1000** events per request (else `413`); body ≤ 8 MiB. Batch
  client-side and send ~1×/sec.
- **Responses:** `202 {"accepted": N}` · `400` (no/!utf8/empty body) ·
  `401` (bad key) · `413` (too many) · `503` (queue unavailable).

Only the fields you set are sent; everything else falls back to a column
default. Unknown fields are ignored (the insert uses
`input_format_skip_unknown_fields=1`), so you can add fields before the schema
catches up.

### Admin — key management
All guarded by the `INGEST_ADMIN_TOKEN` (header `x-admin-token: <token>` or
`Authorization: Bearer <token>`). Disabled if the token is unset.

| Method | Path | Body | Returns |
|--------|------|------|---------|
| `POST` | `/v1/admin/keys` | `{"tenant":"...","label":"..."}` | `{id, tenant, key, note}` — `key` shown once |
| `GET` | `/v1/admin/keys` | — | array of key metadata (no plaintext) |
| `DELETE` | `/v1/admin/keys/{id}` | — | `{"revoked": true\|false}` |

Revocation takes effect within ~**60s** (the in-process key-lookup cache TTL).

---

## 3. Event schema

Events land in `<tenant>.events` (ClickHouse `MergeTree`, partitioned by month).
Send JSON objects with any subset of these fields — names match the columns:

| field | type | meaning |
|-------|------|---------|
| `ts` | string | event time. Best as `YYYY-MM-DD HH:MM:SS.mmm` (UTC); RFC3339 also parses. Defaults to ingest time if omitted. |
| `source` | string | `backend` / `frontend` / worker name |
| `category` | string | top-level grouping — e.g. `http`, `auth`, `note`, `chat`, `log` |
| `event_type` | string | action within the category — e.g. `created`, `login_failed`, `GET` |
| `severity` | string | `debug` / `info` / `warn` / `error` |
| `user_id`, `user_email` | string | acting user |
| `session_id`, `request_id` | string | correlation ids |
| `entity_type`, `entity_id` | string | the affected resource — e.g. `("note", "<uuid>")` |
| `message` | string | free-text |
| `error_code` | string | machine-readable failure tag |
| `model` | string | LLM model (for chat/AI events) |
| `tokens_input`, `tokens_output` | uint | token counts |
| `duration_ms` | uint | operation/request duration |
| `http_status` | uint | response status (for http events) |
| `route` | string | request path / route |
| `app_version`, `server` | string | build + host identity (set `server` to tell hosts apart) |
| `ip`, `user_agent` | string | client info |
| `attributes` | string | free-form JSON blob — query later via `JSONExtract(attributes, ...)` |

> Tip: set `server` per host (e.g. `<node>-app_three` vs a dev box) so you can
> filter by origin. The app decides this value; nothing is inferred.

---

## 4. Onboard a new tenant

The tenant id is the project name, used verbatim (matches the `*_TENANTS`
convention used elsewhere in this repo).

1. **Mint a key** — run on the infra host (admin token from `.env`):
   ```bash
   cd ~/docker/frdcmp-infra && source .env
   curl -s -X POST http://172.25.125.233:46005/v1/admin/keys \
     -H "x-admin-token: $INGEST_ADMIN_TOKEN" \
     -H 'content-type: application/json' \
     -d '{"tenant":"<name>","label":"<env / description>"}'
   # → {"id":"…","tenant":"<name>","key":"ik_…"}   ← store the key now, it is not recoverable
   ```
   The `<name>.events` table is created automatically on the first insert — no
   manual DDL needed. That's the only step — the gateway writes as admin, so the
   tenant needs no ClickHouse user of its own.

No Valkey tenant is needed — the gateway is the only Valkey writer.

**Per environment:** mint a **separate key per deployment** (e.g. one for prod,
one for dev) with distinct labels, so you can revoke one without the other.
Events from both land in the same `<tenant>.events` table; tell them apart with
the `server` field.

---

## 5. Retention (TTL)

One `events` table per tenant with tiered, expression-based TTL:

- `category = 'http'` (noisy access logs) → **30 days**
- everything else (domain events, errors/warns) → **90 days**

```sql
TTL toDateTime(ts) + toIntervalDay(if(category = 'http', 30, 90)) DELETE
```

New tenants get this automatically. To change retention on an existing tenant:
```sql
ALTER TABLE <tenant>.events
  MODIFY TTL toDateTime(ts) + toIntervalDay(if(category = 'http', 30, 90)) DELETE;
```

---

## 6. Query the logs

Direct ClickHouse (admin), or wire a Grafana ClickHouse datasource at
`<tenant>.events`:
```bash
cd ~/docker/frdcmp-infra && source .env
curl -s "http://172.25.125.233:46003/?database=<tenant>" \
  -u "$CLICKHOUSE_ADMIN_USER:$CLICKHOUSE_ADMIN_PASSWORD" \
  --data-binary "SELECT category, event_type, count() n, max(ts) latest
                 FROM events WHERE ts > now() - INTERVAL 1 HOUR
                 GROUP BY category, event_type ORDER BY latest DESC FORMAT PrettyCompact"
```

---

## 7. Reference integration (the thin client pattern)

`app-three` is the canonical example (`rust/src/apps/telemetry/`). The shape,
in any language:

1. An `Event` builder whose fields mirror §3.
2. A bounded in-memory queue + a background task that batches (~500 events or
   ~1s) and POSTs to `TELEMETRY_INGEST_URL` with the bearer key.
3. **Fire-and-forget:** emitting never blocks a request and drops on a full
   queue or a failed POST — logging must never take down the app.
4. Disabled entirely when `TELEMETRY_INGEST_URL` is empty.

Plus, in app-three: an HTTP middleware emitting one `http` event per request,
and a `log` tee that forwards `warn!`/`error!` records as events.

---

## 8. Operations

Runs as the `ingest-api` service in the root `docker-compose.yml`.

```bash
cd ~/docker/frdcmp-infra
docker compose up -d --build ingest-api     # build + (re)start
docker compose logs -f ingest-api           # tail
docker compose ps ingest-api                # status/health
```

**Server env** (set in the root `.env`):

| var | purpose |
|-----|---------|
| `INGEST_ADMIN_TOKEN` | bearer token for `/v1/admin/keys` (empty = admin disabled) |
| `INGEST_BIND` | published bind IP (overlay IP in prod; default `127.0.0.1`) |
| `INGEST_EXT_PORT` | published port (default `46005`) |
| `INGEST_RUST_LOG` | log level (default `info`) |
| `CLICKHOUSE_URL` | `http://clickhouse:8123` (internal) |
| `CLICKHOUSE_ADMIN_USER` / `CLICKHOUSE_ADMIN_PASSWORD` | admin creds (provision + insert) |
| `REDIS_URL` | `redis://default:<VK_ADMIN_PASSWORD>@valkey:6379` (must be URL-safe) |

**Internals:** stream `ingest:events` capped at ~5M (oldest dropped on overflow);
drain reads in batches of 5000 / 2s block; a ClickHouse error leaves the batch
unacked for redelivery (at-least-once) and backs off — a CH outage buffers in
Valkey rather than losing events.

---

## 9. Troubleshooting

| symptom | cause / fix |
|---------|-------------|
| `401` on POST | key wrong/revoked, or revoke not yet past the 60s cache. Re-check the key; mint a new one. |
| `401` on admin | `INGEST_ADMIN_TOKEN` mismatch or unset on the server. |
| `accepted` > 0 but nothing in ClickHouse | check `docker compose logs ingest-api` for `insert … failed` (CH down) — events stay buffered in Valkey and flush when CH recovers. |
| app can't reach the URL | not on the the private overlay overlay, or `INGEST_BIND` is loopback. Confirm `curl http://172.25.125.233:46005/health`. |
| `server` column blank | the app isn't sending `server` — set it (and in app-three, pass `SERVER_NAME` to the backend container). |
| events queued but app restarted | the in-app buffer is in-memory; a small number in flight can be lost on restart. Durable buffering starts at the gateway's Valkey. |
