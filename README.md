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

| Service | Role | Container port | Compose host port | Docs |
|---------|------|----------------|-------------------|------|
| `ingest-api` | **The only entry point** — validates events against the standard, queues them, drains them to ClickHouse | `8080` | `46005` | [README](ingest-api/README.md) |
| `valkey` | **Internal queue** — buffers accepted events between ingest-api and ClickHouse. Used exclusively by ingest-api | `6379` | **none (internal-only)** | [README](valkey/README.md) |
| `clickhouse` | **The log store** — one database per project | `8123` (HTTP) | **none (internal-only)** | [README](clickhouse/README.md) |
| `mcp` | **Optional MCP endpoint** (profile `mcp`) — read-only telemetry tools for AI clients; each client sends its own API key | `3000` | `46007` | [README](mcp/README.md) |

> **Everything is gateway-only.** Neither ClickHouse nor Valkey publishes a host
> port — both are reachable only on the internal `clicklog` network, in practice
> only by `ingest-api`. Apps **cannot** write to ClickHouse or touch the queue
> directly; all event data must go through `POST /v1/events`, which enforces the
> event standard and rejects anything off-spec. There is no fallback.

There are two ways to run the stack, and they share the same services,
config keys and tenant model:

- **`k8s/` — the production deployment**, a k3s namespace. The gateway is
  addressed by a name on the private overlay that belongs to the *service*, so
  it does not matter which node runs the pod.
- **`docker-compose.yml` + `.env` — local and development**, all services on one
  host sharing a `clicklog` bridge network, up and down together.

Each service keeps its own config/data subfolder and README.

---

## Deployment topology

### Production — k3s (`k8s/`)

Two services are reachable, each as its own device on the private overlay
(Tailscale), published by the operator's `loadBalancerClass: tailscale`. The
address belongs to the Service, not to a node, so it follows the pod when it
reschedules and every caller uses one name:

| Service | Endpoint | Exposure |
|---------|----------|----------|
| ingest-api | `http://clicklog-ingest.<tailnet>.ts.net:8080` | overlay device |
| frontend (dashboard) | `http://clicklog.<tailnet>.ts.net` | overlay device |
| mcp (AI read tools) | `http://clicklog.<tailnet>.ts.net/mcp` | via the dashboard's nginx |
| clickhouse | internal-only (`clickhouse:8123`) | NetworkPolicy: gateway only |
| valkey | internal-only (`valkey:6379`) | NetworkPolicy: gateway only |

Cluster pods resolve those names because CoreDNS forwards the overlay's DNS zone
to the overlay resolver (`k8s/05-coredns-ts-net.yaml`); machines on the overlay
resolve them natively. Apps therefore carry a name, and a service that is
assigned a new overlay address needs no config change anywhere.

**The manifests in `k8s/` are templates.** They are committed with
`__REGISTRY__`, `__TAG__` and `__TAILNET__` placeholders, so this repo carries no
one installation's addresses and nothing real is ever pushed. Rendering them is
one command:

```bash
cp .env.k8s.example .env.k8s     # then edit: REGISTRY_HOST, TAILNET, secrets
./k8s/render-config.sh           # -> k8s/.rendered/  (gitignored)
kubectl apply -f k8s/.rendered/
```

`render-config.sh` writes `k8s/.rendered/` and nothing else: `00-config.yaml`
(the Namespace, ConfigMap and Secret built from `.env.k8s`) plus every template
with its placeholders substituted. That directory is `0700`, the Secret `0600`,
and it is gitignored — it is the only place real values land on disk.

`TAG` selects the image tag and defaults to the current git short SHA. **Applying
the rendered manifests sets the image**, so when you mean to change only other
fields, render with the tag that is already running:

```bash
TAG=$(kubectl -n clicklog get deploy ingest-api \
        -o jsonpath='{.spec.template.spec.containers[0].image}' | sed 's/.*://') \
  ./k8s/render-config.sh
```

Building and pushing the three images is the other half:

```bash
TAG=$(git rev-parse --short HEAD); REG=<your-registry-host>
for c in ingest-api frontend mcp; do
  docker build -t "$REG/clicklog-$c:$TAG" "./$c" && docker push "$REG/clicklog-$c:$TAG"
  kubectl -n clicklog set image "deploy/$c" "$c=$REG/clicklog-$c:$TAG"
done
```

Two notes on applying:

- `00-config.yaml` always reports one change. Its Secret is written as
  `stringData`, which the server stores as `data`, so the applied annotation
  never matches — it is not a real diff.
- A ConfigMap change needs `ingest-api` and `valkey` restarted. The gateway
  reads it through `envFrom` and valkey pulls `VK_MAXMEMORY` into its launch
  arguments, and neither re-reads a ConfigMap while running. Valkey persists the
  event stream with AOF, so the queue survives the restart.

`k8s/05-coredns-ts-net.yaml` is **cluster-wide DNS**, not a clicklog resource. If
you change it, CoreDNS needs a restart:

```bash
kubectl -n kube-system rollout restart deploy/coredns
```

### Local / development — Compose

All services on one host. What interface each published port binds to and which
port it uses are set in the root `.env`, not baked into the repo:

| Service | Bind interface | Endpoint | `.env` knobs |
|---------|----------------|----------|--------------|
| ingest-api | _overlay IP_ | `http://<ip>:46005` | `INGEST_BIND`, `INGEST_EXT_PORT` |
| frontend (dashboard, optional) | _overlay IP_ | `http://<ip>:46006` | `FRONTEND_BIND`, `FRONTEND_EXT_PORT` |
| mcp (AI read tools, optional) | _overlay IP_ | `http://<ip>:46007/mcp` (or `:46006/mcp` via dashboard) | `MCP_BIND`, `MCP_EXT_PORT` |
| clickhouse | — | internal-only (`clickhouse:8123`) | — |
| valkey | — | internal-only (`valkey:6379`) | — |

**Networking & security model** (both deployments):

- Only `ingest-api` (and the optional dashboard) are reachable. Keep them on a
  **private overlay** and **never** on a public NIC. Under Compose that is the
  `*_BIND` setting; under k3s it is the overlay device the operator creates.
- ClickHouse and Valkey are reachable only by `ingest-api` — on the internal
  `clicklog` bridge under Compose, and by NetworkPolicy under k3s. Neither has a
  password: unreachability *is* the access control, so the two must stay
  equivalent.
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
TELEMETRY_INGEST_URL="http://clicklog-ingest.<tailnet>.ts.net:8080/v1/events"
#                      ... or http://ingest-api:8080/v1/events from inside the stack
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

### Let an LLM do the integration

There is a single self-contained prompt page with everything an LLM needs to
implement a clicklog client — schema, rules, instrumentation guidance, smoke
tests. Tell your coding agent:

> Fetch `http://<dashboard-host>:46006/llms.txt` and implement telemetry in
> this app as it describes. Here are the two env values: …

The dashboard serves it at **`/llms.txt`**; if the agent can't reach the
overlay, paste the contents of
[`frontend/public/llms.txt`](frontend/public/llms.txt) into the chat instead.
Either way, hand the agent the two env vars (`TELEMETRY_INGEST_URL`,
`TELEMETRY_API_KEY`) and it can take the integration from zero to verified.

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

### Let an LLM read the telemetry

Two ways to point an AI agent at your logs, mirroring the integration guide:

- **Prompt page** — the dashboard serves **`/llms-read.txt`**
  ([`frontend/public/llms-read.txt`](frontend/public/llms-read.txt)): a
  self-contained guide to the read API (search, stats, pagination, recipes).
  Any agent that can make HTTP calls (Claude Code, a script) just needs that
  link plus the base URL + API key.
- **MCP server** — [`mcp/`](mcp/README.md) exposes `stats`, `search_events`,
  and `get_event` as typed MCP tools over the same API. Run it as a **remote
  endpoint** next to the stack (`docker compose --profile mcp up -d`, reachable
  at `http://<host>:46006/mcp` via the dashboard or on its own port `46007` —
  clients authenticate per request with their tenant API key), or spawn it
  locally over stdio from any MCP client. clicklog itself never moves.

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
├── k8s/                 ← the production deployment (k3s namespace `clicklog`)
│   ├── *.yaml           ← templates: __REGISTRY__ / __TAG__ / __TAILNET__
│   ├── render-config.sh ← renders them + the Secret into k8s/.rendered/
│   └── .rendered/       ← gitignored output; what you actually apply
├── docker-compose.yml   ← the local/dev stack (all services on one host)
├── .env / .env.example  ← single env for the whole stack
├── clickhouse/          ← log store: config.d/, init/, README, data dirs
├── valkey/              ← internal log queue: valkey.conf, README, data dir
├── ingest-api/          ← telemetry gateway (Rust): src/, Dockerfile, README
├── frontend/            ← admin dashboard (React+nginx, profile: dashboard), README
└── mcp/                 ← stdio MCP server exposing the read API as tools (Node), README
```

> **Admin dashboard (optional):** a React UI for API-key CRUD, cross-tenant log
> search, and docs, served by nginx and gated behind the `dashboard` compose
> profile. Start the stack with it via `docker compose --profile dashboard up -d`
> (default `127.0.0.1:46006`). Login is a single seeded admin (JWT) — set
> `JWT_SECRET` + `ADMIN_PASSWORD` in `.env`. See [frontend/README.md](frontend/README.md).

Secrets (`.env`) and data volumes (`*_data/`, `*_logs/`) are git-ignored — only
`.env.example` and config are tracked.
