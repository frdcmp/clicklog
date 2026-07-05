import type { ReactNode } from 'react'
import { PageHeader } from '../components/layout/PageHeader'
import { Card } from '../components/ui/Card'
import { Badge } from '../components/ui/Badge'

// Curated documentation for the ingest gateway, derived from the repo READMEs +
// the event standard in ingest-api/src/schema.rs. Static content, no data fetch.

const SECTIONS = [
  { id: 'overview', label: 'Overview' },
  { id: 'event-standard', label: 'Event standard' },
  { id: 'ingest', label: 'Ingest' },
  { id: 'read', label: 'Read & stats' },
  { id: 'admin', label: 'Admin API' },
  { id: 'onboarding', label: 'Onboard a service' },
]

export function DocsPage() {
  return (
    <div>
      <PageHeader title="Documentation" description="How the ingest gateway works and how services connect to it." />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[180px_1fr]">
        <nav className="hidden lg:block">
          <div className="sticky top-6 space-y-1">
            {SECTIONS.map((s) => (
              <a
                key={s.id}
                href={`#${s.id}`}
                className="block rounded px-2 py-1 text-sm text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800"
              >
                {s.label}
              </a>
            ))}
          </div>
        </nav>

        <div className="min-w-0 space-y-6">
          <Section id="overview" title="Overview">
            <P>
              Apps POST batches of <b>events</b> to the gateway with an API key. Events are validated against the
              standard, buffered on a Valkey stream, and drained into per-tenant ClickHouse <Code>events</Code> tables.
              Storage credentials never leave the infra host — an app holds only a URL and a key.
            </P>
            <Pre>{`app ──POST /v1/events (Bearer <key>)──▶ ingest-api ──▶ Valkey ingest:events
                                                     │ (drain)
                                                     ▼
                                       ClickHouse  <tenant>.events`}</Pre>
            <P>
              One key maps to one <b>tenant</b> (its own database). The same key both writes and reads that tenant's
              events. Logging is <b>gateway-only</b>: ClickHouse is not otherwise reachable.
            </P>
          </Section>

          <Section id="event-standard" title="Event standard">
            <P>
              Every event is validated <b>before</b> buffering. Any violation rejects the <b>whole batch</b> with{' '}
              <Code>400</Code> and a per-event error list. Unknown top-level fields are rejected — put custom data inside{' '}
              <Code>attributes</Code> (a JSON string). Body may be a JSON object, an array, or NDJSON (≤ 1000
              events/batch).
            </P>
            <FieldTable />
            <P className="mt-3">Example valid event:</P>
            <Pre>{`{
  "category": "http",
  "event_type": "GET",
  "severity": "info",
  "route": "/widgets/summary",
  "http_status": 200,
  "duration_ms": 12,
  "attributes": "{\\"region\\":\\"eu\\"}"
}`}</Pre>
          </Section>

          <Section id="ingest" title="Ingest">
            <Endpoint method="POST" path="/v1/events" note="Auth: Bearer <key> or x-api-key" />
            <P>Send one or many events. Returns <Code>{`{ "accepted": N }`}</Code> on success.</P>
            <Pre>{`curl -X POST "$URL/v1/events" \\
  -H "Authorization: Bearer $KEY" \\
  -H 'content-type: application/json' \\
  -d '[{"category":"test","event_type":"smoke","severity":"info","message":"hello"}]'`}</Pre>
            <Callout>
              Do <b>not</b> add your own Redis queue for logs — the durable buffer lives in the gateway. Fire-and-forget,
              or use a small in-memory buffer that flushes ~every second.
            </Callout>
          </Section>

          <Section id="read" title="Read & stats">
            <Endpoint method="GET" path="/v1/events" note="search / list" />
            <P>
              Params: <Code>from</Code>/<Code>to</Code> (<Code>-1h</Code>, <Code>-7d</Code>, RFC3339; default last 1h),{' '}
              <Code>category</Code>, <Code>event_type</Code>, <Code>severity</Code>, <Code>source</Code>,{' '}
              <Code>model</Code>, <Code>user_id</Code>, <Code>http_status</Code> (comma = OR), <Code>q</Code> (message
              substring), <Code>order</Code>, <Code>limit</Code> (≤1000), <Code>cursor</Code>. Returns{' '}
              <Code>{`{ events, next_cursor }`}</Code>.
            </P>
            <Endpoint method="GET" path="/v1/events/{event_id}" note="fetch one" />
            <Endpoint method="GET" path="/v1/stats" note="aggregates / timeseries" />
            <P>
              Params: <Code>group_by</Code> (category/event_type/severity/source/model/http_status/…),{' '}
              <Code>interval</Code> (<Code>1m</Code>…<Code>1d</Code> → timeseries; omit → totals),{' '}
              <Code>metric</Code> (<Code>count</Code>, <Code>sum:tokens_input</Code>, <Code>avg:duration_ms</Code>, …)
              plus the same filters.
            </P>
          </Section>

          <Section id="admin" title="Admin API">
            <P>
              The endpoints this dashboard uses, under <Code>/v1/admin/*</Code>. Auth is a JWT from{' '}
              <Code>POST /v1/admin/login</Code>.
            </P>
            <Endpoint method="POST" path="/v1/admin/login" note="{ email, password } → { token }" />
            <Endpoint method="GET" path="/v1/admin/me" />
            <Endpoint method="GET" path="/v1/admin/keys" />
            <Endpoint method="POST" path="/v1/admin/keys" note="{ tenant, label } → key (shown once)" />
            <Endpoint method="DELETE" path="/v1/admin/keys/{id}" />
            <Endpoint method="GET" path="/v1/admin/tenants" />
            <Endpoint method="GET" path="/v1/admin/events" note="+ tenant (name or * for all)" />
            <Endpoint method="GET" path="/v1/admin/stats" note="+ tenant; group_by=tenant when tenant=*" />
          </Section>

          <Section id="onboarding" title="Onboard a service">
            <Ol>
              <li>
                Go to <b>API Keys → Mint key</b>, enter the service name as the tenant (lowercase slug) and a label. Copy
                the <Code>ik_…</Code> key — it is shown once.
              </li>
              <li>
                In the app, set two env vars and POST event batches:
                <Pre>{`TELEMETRY_INGEST_URL="http://<infra-host>:46005/v1/events"
TELEMETRY_API_KEY="ik_…"`}</Pre>
              </li>
              <li>The tenant's ClickHouse database + <Code>events</Code> table are created on the first write.</li>
              <li>Search them here under <b>Logs</b> (pick the tenant, or “All”).</li>
            </Ol>
          </Section>
        </div>
      </div>
    </div>
  )
}

// ── field reference (mirrors ingest-api/src/schema.rs) ────────────────────────

interface FieldRow {
  name: string
  type: string
  notes: string
  req?: boolean
}
const FIELDS: FieldRow[] = [
  { name: 'category', type: 'string', notes: 'Coarse class, e.g. http / llm / auth.', req: true },
  { name: 'event_type', type: 'string', notes: 'Specific type, e.g. GET / request.', req: true },
  { name: 'severity', type: 'enum', notes: 'debug | info | warn | error (default info).' },
  { name: 'source', type: 'string', notes: 'Emitting component.' },
  { name: 'message', type: 'string', notes: 'Human-readable text (searchable via q).' },
  { name: 'event_id', type: 'uuid', notes: 'Optional; must be a UUID if supplied.' },
  { name: 'ts / received_at', type: 'datetime', notes: 'RFC3339 string or epoch number.' },
  { name: 'user_id / user_email', type: 'string', notes: 'Actor identity.' },
  { name: 'session_id / request_id', type: 'string', notes: 'Correlation ids.' },
  { name: 'entity_type / entity_id', type: 'string', notes: 'Domain object touched.' },
  { name: 'error_code', type: 'string', notes: 'Machine error code.' },
  { name: 'model', type: 'string', notes: 'LLM/model name.' },
  { name: 'route / app_version', type: 'string', notes: 'HTTP route, app version.' },
  { name: 'server / ip / user_agent', type: 'string', notes: 'Origin metadata.' },
  { name: 'tokens_input / tokens_output', type: 'uint32', notes: 'Token counts (≥ 0).' },
  { name: 'duration_ms', type: 'uint32', notes: 'Elapsed ms.' },
  { name: 'http_status', type: 'uint16', notes: 'HTTP status code.' },
  { name: 'attributes', type: 'string', notes: 'JSON string for any custom fields.' },
]

function FieldTable() {
  return (
    <div className="overflow-x-auto rounded-lg border border-zinc-200">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-zinc-200 bg-zinc-50 text-left text-xs uppercase tracking-wide text-zinc-400">
            <th className="px-3 py-2 font-medium">Field</th>
            <th className="px-3 py-2 font-medium">Type</th>
            <th className="px-3 py-2 font-medium">Notes</th>
          </tr>
        </thead>
        <tbody>
          {FIELDS.map((f) => (
            <tr key={f.name} className="border-b border-zinc-100 last:border-0">
              <td className="whitespace-nowrap px-3 py-2 font-mono text-xs text-zinc-800">
                {f.name} {f.req && <Badge tone="accent">required</Badge>}
              </td>
              <td className="whitespace-nowrap px-3 py-2 text-zinc-500">{f.type}</td>
              <td className="px-3 py-2 text-zinc-600">{f.notes}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ── small doc primitives ──────────────────────────────────────────────────────

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <Card className="scroll-mt-6 p-5" >
      <h2 id={id} className="mb-3 text-base font-semibold text-zinc-900">
        {title}
      </h2>
      <div className="space-y-3">{children}</div>
    </Card>
  )
}
function P({ children, className }: { children: ReactNode; className?: string }) {
  return <p className={`text-sm leading-relaxed text-zinc-600 ${className ?? ''}`}>{children}</p>
}
function Code({ children }: { children: ReactNode }) {
  return <code className="rounded bg-zinc-100 px-1 py-0.5 font-mono text-[0.8em] text-zinc-800">{children}</code>
}
function Pre({ children }: { children: string }) {
  return (
    <pre className="overflow-x-auto rounded-md bg-zinc-900 p-3 text-xs leading-relaxed text-zinc-100">{children}</pre>
  )
}
function Ol({ children }: { children: ReactNode }) {
  return <ol className="list-decimal space-y-2 pl-5 text-sm leading-relaxed text-zinc-600">{children}</ol>
}
function Callout({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800">{children}</div>
  )
}
function Endpoint({ method, path, note }: { method: string; path: string; note?: string }) {
  const tone =
    method === 'GET' ? 'bg-sky-100 text-sky-700'
    : method === 'POST' ? 'bg-emerald-100 text-emerald-700'
    : method === 'DELETE' ? 'bg-red-100 text-red-700'
    : 'bg-zinc-100 text-zinc-700'
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className={`inline-flex rounded px-1.5 py-0.5 font-mono text-xs font-semibold ${tone}`}>{method}</span>
      <code className="font-mono text-sm text-zinc-800">{path}</code>
      {note && <span className="text-xs text-zinc-400">{note}</span>}
    </div>
  )
}
