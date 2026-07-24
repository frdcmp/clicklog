#!/usr/bin/env node
// clicklog MCP server — read-only tools over the clicklog telemetry read API.
//
// Two transports:
//   • stdio (default)      — launched locally by an MCP client (Claude Code /
//     Claude Desktop). The API key comes from env and scopes every query to
//     one tenant, same as the raw HTTP API.
//   • --http               — a remote streamable-HTTP endpoint (the compose
//     `mcp` service). One shared endpoint serves all tenants: each client
//     sends ITS OWN clicklog API key on every request (`x-api-key` or
//     `Authorization: Bearer`), and the server forwards it per-request. No
//     key is stored server-side.
//
// Env:
//   CLICKLOG_BASE     e.g. http://infra-host:46005 (stdio) or
//                     http://ingest-api:8080 (compose). A full ingest URL
//                     ending in /v1/events is accepted and trimmed.
//   CLICKLOG_API_KEY  ik_… key — stdio mode only.
//   MCP_PORT          http mode listen port (default 3000).

import http from 'node:http'

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'
import { z } from 'zod'

const rawBase = process.env.CLICKLOG_BASE || process.env.CLICKLOG_URL || process.env.TELEMETRY_INGEST_URL || ''
const BASE = rawBase.replace(/\/v1\/events\/?$/, '').replace(/\/+$/, '')
const HTTP_MODE = process.argv.includes('--http') || process.env.MCP_TRANSPORT === 'http'

if (!BASE) {
  console.error('clicklog-mcp: set CLICKLOG_BASE (e.g. http://host:46005)')
  process.exit(1)
}

// Exact-match filters accepted by /v1/events and /v1/stats. A value may be a
// comma-separated list (matches any of them).
const filterShape = {
  from: z
    .string()
    .optional()
    .describe("Start of time range: relative '-15m'/'-6h'/'-7d' or RFC3339. Default: -1h"),
  to: z.string().optional().describe("End of time range: 'now' (default), relative, or RFC3339"),
  category: z.string().optional().describe("Event category, e.g. 'http', 'llm', 'auth'. Comma-separated for OR"),
  event_type: z.string().optional().describe("Specific action, e.g. 'GET', 'login_failed'. Comma-separated for OR"),
  severity: z.string().optional().describe("'debug', 'info', 'warn' or 'error'. Comma-separated for OR"),
  source: z.string().optional().describe("Emitting process, e.g. 'backend', 'frontend'"),
  model: z.string().optional().describe('LLM model name (for llm events)'),
  user_id: z.string().optional(),
  session_id: z.string().optional(),
  request_id: z.string().optional(),
  entity_type: z.string().optional(),
  entity_id: z.string().optional(),
  error_code: z.string().optional(),
  route: z.string().optional().describe("HTTP path or logical op name, e.g. '/api/orders', 'worker:email_send'"),
  server: z.string().optional().describe("Host/deployment, e.g. 'prod-1'"),
  app_version: z.string().optional(),
  ip: z.string().optional(),
  http_status: z.string().optional().describe("HTTP status code(s), e.g. '500' or '500,502,503'"),
  q: z.string().optional().describe('Case-insensitive substring match on the message field'),
}

async function get(apiKey, path, params = {}) {
  const url = new URL(BASE + path)
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') url.searchParams.set(k, String(v))
  }
  let resp
  try {
    resp = await fetch(url, { headers: { 'x-api-key': apiKey } })
  } catch (e) {
    const why = e.cause?.code || e.cause?.message || e.message
    return { isError: true, content: [{ type: 'text', text: `cannot reach clicklog at ${BASE}: ${why}` }] }
  }
  const body = await resp.text()
  if (!resp.ok) {
    return { isError: true, content: [{ type: 'text', text: `clicklog API ${resp.status}: ${body}` }] }
  }
  return { content: [{ type: 'text', text: body }] }
}

// The tenant API key is bound per server instance: from env in stdio mode,
// from the request headers in http mode (a fresh instance per request).
function buildServer(apiKey) {
  const server = new McpServer({ name: 'clicklog', version: '0.1.0' })

  server.tool(
    'search_events',
    'Search telemetry events (newest first by default). Returns {events, next_cursor}. ' +
      'Prefer the stats tool for counts/aggregates and use this to inspect specific events. ' +
      'Numeric JSON values may be serialised as strings — coerce before doing math. ' +
      "The attributes field is stringified JSON — parse it for custom data. Timestamps are UTC.",
    {
      ...filterShape,
      limit: z.number().int().min(1).max(1000).optional().describe('Rows per page, default 100, max 1000'),
      order: z.enum(['desc', 'asc']).optional().describe("'desc' (default) or 'asc' to replay chronologically"),
      cursor: z.string().optional().describe("Pagination token: pass back the previous response's next_cursor verbatim"),
    },
    async (args) => get(apiKey, '/v1/events', args),
  )

  server.tool(
    'get_event',
    'Fetch a single telemetry event by its UUID event_id.',
    { event_id: z.string().uuid().describe('The event UUID') },
    async ({ event_id }) => get(apiKey, `/v1/events/${event_id}`),
  )

  server.tool(
    'stats',
    'Aggregate telemetry events: counts or sum/avg/min/max of a numeric field, grouped by a dimension, ' +
      'optionally bucketed over time. Returns {stats: [{group_value, value, bucket?}]} sorted by value ' +
      '(by bucket first when bucketed), capped at 5000 rows. Start here to find hot routes / error codes / ' +
      'trends, then drill into search_events. All search filters apply.',
    {
      group_by: z
        .enum([
          'category',
          'event_type',
          'severity',
          'source',
          'model',
          'entity_type',
          'http_status',
          'route',
          'server',
          'app_version',
        ])
        .describe('Dimension to group by'),
      metric: z
        .string()
        .optional()
        .describe(
          "'count' (default) or '<agg>:<field>' with agg sum|avg|min|max and field " +
            'tokens_input|tokens_output|duration_ms|http_status, e.g. avg:duration_ms',
        ),
      interval: z
        .enum(['1m', '5m', '15m', '30m', '1h', '6h', '12h', '1d'])
        .optional()
        .describe('Optional time bucket — adds a bucket timestamp to each row'),
      ...filterShape,
    },
    async (args) => get(apiKey, '/v1/stats', args),
  )

  return server
}

// ── stdio mode ───────────────────────────────────────────────────────────────

async function runStdio() {
  const apiKey = process.env.CLICKLOG_API_KEY || process.env.TELEMETRY_API_KEY || ''
  if (!apiKey) {
    console.error('clicklog-mcp: set CLICKLOG_API_KEY (stdio mode binds one key from env)')
    process.exit(1)
  }
  const server = buildServer(apiKey)
  await server.connect(new StdioServerTransport())
  console.error(`clicklog-mcp: connected over stdio (base ${BASE})`)
}

// ── http mode (remote endpoint) ──────────────────────────────────────────────
// Stateless: each POST is handled by a fresh server+transport pair bound to the
// caller's API key, so one endpoint serves many tenants with no session state.

function keyFrom(req) {
  const direct = req.headers['x-api-key']
  if (direct) return String(direct)
  const auth = String(req.headers.authorization || '')
  return auth.toLowerCase().startsWith('bearer ') ? auth.slice(7).trim() : ''
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = ''
    req.on('data', (c) => {
      data += c
      if (data.length > 4 * 1024 * 1024) reject(new Error('body too large'))
    })
    req.on('end', () => resolve(data))
    req.on('error', reject)
  })
}

function deny(res, status, message) {
  res.writeHead(status, { 'content-type': 'application/json' })
  res.end(JSON.stringify({ jsonrpc: '2.0', error: { code: -32001, message }, id: null }))
}

async function runHttp() {
  const port = Number(process.env.MCP_PORT || 3000)
  const httpServer = http.createServer(async (req, res) => {
    const path = (req.url || '').split('?')[0]
    if (path === '/healthz') {
      res.writeHead(200, { 'content-type': 'text/plain' })
      return res.end('ok')
    }
    // Accept the MCP endpoint at /mcp (nginx route) and / (direct port).
    if (path !== '/mcp' && path !== '/') {
      return deny(res, 404, 'not found — MCP endpoint is /mcp')
    }
    const apiKey = keyFrom(req)
    if (!apiKey) {
      return deny(res, 401, 'missing clicklog API key (x-api-key or Authorization: Bearer header)')
    }
    try {
      let parsed
      if (req.method === 'POST') {
        const raw = await readBody(req)
        parsed = raw ? JSON.parse(raw) : undefined
      }
      const server = buildServer(apiKey)
      const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined })
      res.on('close', () => {
        transport.close()
        server.close()
      })
      await server.connect(transport)
      await transport.handleRequest(req, res, parsed)
    } catch (e) {
      console.error('clicklog-mcp: request failed:', e.message)
      if (!res.headersSent) deny(res, 400, `bad request: ${e.message}`)
    }
  })
  httpServer.listen(port, '0.0.0.0', () => {
    console.error(`clicklog-mcp: streamable-http listening on :${port} (base ${BASE})`)
  })
}

if (HTTP_MODE) {
  await runHttp()
} else {
  await runStdio()
}
