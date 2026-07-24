# clicklog-mcp — telemetry read tools for MCP clients

An [MCP](https://modelcontextprotocol.io) server that wraps the clicklog
**read API**, so AI clients can query your telemetry with typed tools. It only
ever calls the three read endpoints; it exposes no ingest/write tool. Like the
raw API, the API key scopes every query to **one tenant**.

Coding agents that can run curl don't strictly need this — hand them the
dashboard's `/llms-read.txt` guide instead. MCP is for clients that can't make
raw HTTP calls (Claude Desktop) or when you want typed, discoverable tools.

## Tools

| Tool | Wraps | Purpose |
|------|-------|---------|
| `stats` | `GET /v1/stats` | Counts / sum·avg·min·max aggregates by dimension, optional time buckets — the starting point for any "what's happening?" question |
| `search_events` | `GET /v1/events` | Filtered event search with keyset pagination |
| `get_event` | `GET /v1/events/{id}` | One event by UUID |

## Two ways to run it

**clicklog itself always stays on its own host** — the only question is where
the small MCP bridge process runs.

### 1. Remote endpoint (recommended) — `--profile mcp` in compose

Runs next to the stack and serves streamable-HTTP MCP. **No key is stored
server-side**: every client sends its own clicklog API key on each request,
so one endpoint serves all tenants.

```bash
# on the clicklog host
docker compose --profile mcp up -d          # add --profile dashboard as usual
```

It's reachable two ways (bind/port via `MCP_BIND` / `MCP_EXT_PORT` in `.env`):

- through the dashboard, same origin: `http://<host>:46006/mcp`
- its own published port: `http://<host>:46007/mcp`

Connect from anywhere that can reach it:

```bash
# Claude Code
claude mcp add --transport http clicklog http://<host>:46006/mcp \
  --header "x-api-key: ik_…"
```

For Claude Desktop (which speaks stdio to local processes), bridge with
`mcp-remote`:

```json
{
  "mcpServers": {
    "clicklog": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "http://<host>:46006/mcp",
               "--header", "x-api-key: ik_…"]
    }
  }
}
```

> **Exposure:** the endpoint speaks plain HTTP and auth is the API key header.
> Keep it on the private overlay like the rest of the stack. To use it from
> the actual internet (e.g. claude.ai custom connectors, which require public
> HTTPS), front it with a TLS tunnel (cloudflared/Caddy) — never bind a public
> NIC directly.

### 2. Local stdio (no server-side changes at all)

The MCP client spawns the process locally; it talks HTTP to the remote
clicklog. Needs Node ≥ 18 and this folder on the client machine
(`npm install` once).

```bash
claude mcp add clicklog \
  -e CLICKLOG_BASE=http://<host>:46005 \
  -e CLICKLOG_API_KEY=ik_… \
  -- node /path/to/clicklog/mcp/index.js
```

`CLICKLOG_BASE` may also point at the dashboard (`http://<host>:46006`) —
nginx proxies `/v1/` to the gateway. The `TELEMETRY_*` env names used by app
integrations are accepted too, and a base ending in `/v1/events` is trimmed.

## Smoke test (HTTP mode)

```bash
curl -s http://<host>:46007/healthz    # → ok
curl -s -X POST http://<host>:46006/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'x-api-key: ik_…' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stats","arguments":{"group_by":"severity","from":"-24h"}}}'
```

The response should carry `{"stats": [...]}` for your tenant. Without the
`x-api-key` header the endpoint answers `401`.
