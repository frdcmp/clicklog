import { useMemo, useState, type FormEvent } from 'react'
import { PageHeader } from '../components/layout/PageHeader'
import { Card } from '../components/ui/Card'
import { Button } from '../components/ui/Button'
import { Field, Input, Select } from '../components/ui/Field'
import { SeverityBadge, StatusBadge, Badge } from '../components/ui/Badge'
import { Drawer } from '../components/ui/Modal'
import { EmptyState, ErrorNote, Spinner } from '../components/ui/Feedback'
import { cn } from '../lib/cn'
import { RANGE_PRESETS, fmtTs, fmtNumber } from '../lib/time'
import { downloadEventsCsv } from '../lib/csv'
import { useEventsInfinite } from '../query/events'
import { useTenants } from '../query/tenants'
import type { EventsQuery } from '../api/events'
import type { LogEvent, Severity } from '../types'

const SEVERITIES: Severity[] = ['debug', 'info', 'warn', 'error']

interface FormState {
  tenant: string
  from: string // relative token or '' when using custom
  customFrom: string // datetime-local
  customTo: string
  severities: Set<Severity>
  category: string
  event_type: string
  source: string
  model: string
  user_id: string
  request_id: string
  http_status: string
  q: string
  order: 'asc' | 'desc'
  limit: number
}

const initialForm: FormState = {
  tenant: '*',
  from: '-1h',
  customFrom: '',
  customTo: '',
  severities: new Set(),
  category: '',
  event_type: '',
  source: '',
  model: '',
  user_id: '',
  request_id: '',
  http_status: '',
  q: '',
  order: 'desc',
  limit: 100,
}

/** Turn the form into the query the backend understands. */
function buildQuery(f: FormState): EventsQuery {
  const q: EventsQuery = { tenant: f.tenant, order: f.order, limit: f.limit }
  if (f.customFrom) q.from = new Date(f.customFrom).toISOString()
  else q.from = f.from
  if (f.customTo) q.to = new Date(f.customTo).toISOString()
  if (f.severities.size) q.severity = [...f.severities].join(',')
  for (const k of ['category', 'event_type', 'source', 'model', 'user_id', 'request_id', 'q'] as const) {
    if (f[k].trim()) q[k] = f[k].trim()
  }
  if (f.http_status.trim()) q.http_status = f.http_status.trim()
  return q
}

export function LogsPage() {
  const tenants = useTenants()
  const [form, setForm] = useState<FormState>(initialForm)
  const [applied, setApplied] = useState<EventsQuery>(() => buildQuery(initialForm))
  const [selected, setSelected] = useState<LogEvent | null>(null)

  const query = useEventsInfinite(applied)
  const events = useMemo(() => (query.data?.pages ?? []).flatMap((p) => p.events), [query.data])
  const isCrossTenant = applied.tenant === '*' || applied.tenant === 'all'

  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setForm((f) => ({ ...f, [k]: v }))

  const submit = (e: FormEvent) => {
    e.preventDefault()
    setApplied(buildQuery(form))
  }
  const reset = () => {
    setForm(initialForm)
    setApplied(buildQuery(initialForm))
  }

  return (
    <div>
      <PageHeader
        title="Logs"
        description="Search and filter events across tenants."
        actions={
          <Button
            variant="secondary"
            size="sm"
            disabled={events.length === 0}
            onClick={() => downloadEventsCsv(events, 'frdcmp-events.csv')}
          >
            Export CSV
          </Button>
        }
      />

      <Card className="mb-4 p-4">
        <form onSubmit={submit} className="space-y-4">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Field label="Tenant">
              <Select value={form.tenant} onChange={(e) => set('tenant', e.target.value)}>
                <option value="*">All tenants</option>
                {(tenants.data ?? []).map((t) => (
                  <option key={t.tenant} value={t.tenant}>
                    {t.tenant}
                  </option>
                ))}
              </Select>
            </Field>

            <Field label="Time range" className="sm:col-span-2 lg:col-span-2">
              <div className="flex flex-wrap gap-1">
                {RANGE_PRESETS.map((p) => (
                  <button
                    key={p.value}
                    type="button"
                    onClick={() => setForm((f) => ({ ...f, from: p.value, customFrom: '', customTo: '' }))}
                    className={cn(
                      'rounded-md border px-2.5 py-1.5 text-xs font-medium',
                      form.from === p.value && !form.customFrom
                        ? 'border-accent-500 bg-accent-50 text-accent-700'
                        : 'border-zinc-300 text-zinc-600 hover:bg-zinc-50',
                    )}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
            </Field>

            <Field label="Order">
              <Select value={form.order} onChange={(e) => set('order', e.target.value as 'asc' | 'desc')}>
                <option value="desc">Newest first</option>
                <option value="asc">Oldest first</option>
              </Select>
            </Field>
          </div>

          {/* Custom absolute range */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Field label="From (custom)">
              <Input type="datetime-local" value={form.customFrom} onChange={(e) => set('customFrom', e.target.value)} />
            </Field>
            <Field label="To (custom)">
              <Input type="datetime-local" value={form.customTo} onChange={(e) => set('customTo', e.target.value)} />
            </Field>
            <Field label="Limit / page">
              <Select value={String(form.limit)} onChange={(e) => set('limit', Number(e.target.value))}>
                {[50, 100, 250, 500, 1000].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="HTTP status">
              <Input value={form.http_status} onChange={(e) => set('http_status', e.target.value)} placeholder="200,404,500" />
            </Field>
          </div>

          {/* Severity toggles */}
          <div>
            <span className="mb-1 block text-xs font-medium text-zinc-600">Severity</span>
            <div className="flex flex-wrap gap-1.5">
              {SEVERITIES.map((s) => {
                const on = form.severities.has(s)
                return (
                  <button
                    key={s}
                    type="button"
                    onClick={() =>
                      setForm((f) => {
                        const next = new Set(f.severities)
                        next.has(s) ? next.delete(s) : next.add(s)
                        return { ...f, severities: next }
                      })
                    }
                    className={cn(
                      'rounded-md border px-2.5 py-1 text-xs font-medium capitalize',
                      on ? 'border-accent-500 bg-accent-50 text-accent-700' : 'border-zinc-300 text-zinc-500 hover:bg-zinc-50',
                    )}
                  >
                    {s}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Text filters */}
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <Field label="Message contains">
              <Input value={form.q} onChange={(e) => set('q', e.target.value)} placeholder="substring…" />
            </Field>
            <Field label="Category">
              <Input value={form.category} onChange={(e) => set('category', e.target.value)} placeholder="http, llm…" />
            </Field>
            <Field label="Event type">
              <Input value={form.event_type} onChange={(e) => set('event_type', e.target.value)} placeholder="GET, request…" />
            </Field>
            <Field label="Source">
              <Input value={form.source} onChange={(e) => set('source', e.target.value)} />
            </Field>
            <Field label="Model">
              <Input value={form.model} onChange={(e) => set('model', e.target.value)} />
            </Field>
            <Field label="User / request id">
              <Input
                value={form.user_id || form.request_id}
                onChange={(e) => setForm((f) => ({ ...f, user_id: e.target.value, request_id: '' }))}
                placeholder="user_id"
              />
            </Field>
          </div>

          <div className="flex items-center gap-2">
            <Button type="submit" loading={query.isFetching && !query.isFetchingNextPage}>
              Search
            </Button>
            <Button type="button" variant="secondary" onClick={reset}>
              Reset
            </Button>
            {query.isFetching && !query.isFetchingNextPage && <Spinner />}
          </div>
        </form>
      </Card>

      {query.error ? (
        <ErrorNote>Search failed. Check the filters and that the ingest-api is reachable.</ErrorNote>
      ) : query.isLoading ? (
        <div className="flex justify-center py-16">
          <Spinner />
        </div>
      ) : events.length === 0 ? (
        <EmptyState title="No matching events" hint="Try widening the time range or clearing filters." />
      ) : (
        <>
          <div className="mb-2 text-xs text-zinc-400">
            {fmtNumber(events.length)} event{events.length === 1 ? '' : 's'} loaded
          </div>
          <ResultsTable events={events} showTenant={isCrossTenant} onSelect={setSelected} />
          <div className="mt-4 flex justify-center">
            {query.hasNextPage ? (
              <Button variant="secondary" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
                Load more
              </Button>
            ) : (
              <span className="text-xs text-zinc-400">End of results</span>
            )}
          </div>
        </>
      )}

      <EventDrawer event={selected} onClose={() => setSelected(null)} />
    </div>
  )
}

function ResultsTable({
  events,
  showTenant,
  onSelect,
}: {
  events: LogEvent[]
  showTenant: boolean
  onSelect: (e: LogEvent) => void
}) {
  return (
    <Card className="overflow-hidden">
      {/* Desktop table */}
      <div className="hidden overflow-x-auto md:block">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-400">
              <th className="whitespace-nowrap px-3 py-2.5 font-medium">Time</th>
              {showTenant && <th className="px-3 py-2.5 font-medium">Tenant</th>}
              <th className="px-3 py-2.5 font-medium">Sev</th>
              <th className="px-3 py-2.5 font-medium">Category / Type</th>
              <th className="px-3 py-2.5 font-medium">Route</th>
              <th className="px-3 py-2.5 font-medium">Message</th>
              <th className="px-3 py-2.5 font-medium">HTTP</th>
              <th className="px-3 py-2.5 font-medium">ms</th>
            </tr>
          </thead>
          <tbody>
            {events.map((e) => (
              <tr
                key={String(e.event_id)}
                onClick={() => onSelect(e)}
                className="cursor-pointer border-b border-zinc-100 last:border-0 hover:bg-zinc-50"
              >
                <td className="whitespace-nowrap px-3 py-2 text-zinc-500">{fmtTs(e.ts)}</td>
                {showTenant && (
                  <td className="px-3 py-2">
                    <Badge tone="accent">{e._tenant || '—'}</Badge>
                  </td>
                )}
                <td className="px-3 py-2">
                  <SeverityBadge severity={e.severity} />
                </td>
                <td className="whitespace-nowrap px-3 py-2 text-zinc-600">
                  {e.category}
                  <span className="text-zinc-300"> / </span>
                  {e.event_type}
                </td>
                <td className="max-w-[16rem] truncate px-3 py-2 font-mono text-xs text-zinc-600">{e.route || '—'}</td>
                <td className="max-w-md truncate px-3 py-2 text-zinc-700">{e.message || '—'}</td>
                <td className="px-3 py-2">
                  <StatusBadge status={Number(e.http_status)} />
                </td>
                <td className="px-3 py-2 tabular-nums text-zinc-500">{Number(e.duration_ms) || ''}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Mobile cards */}
      <ul className="divide-y divide-zinc-100 md:hidden">
        {events.map((e) => (
          <li key={String(e.event_id)} className="cursor-pointer p-3 active:bg-zinc-50" onClick={() => onSelect(e)}>
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <SeverityBadge severity={e.severity} />
                {showTenant && <Badge tone="accent">{e._tenant}</Badge>}
              </div>
              <span className="text-xs text-zinc-400">{fmtTs(e.ts)}</span>
            </div>
            <div className="mt-1 truncate text-sm text-zinc-700">{e.message || `${e.category} / ${e.event_type}`}</div>
            <div className="mt-0.5 flex items-center gap-2 text-xs text-zinc-400">
              <span>{e.category} / {e.event_type}</span>
              {e.route ? <span className="truncate font-mono">{e.route}</span> : null}
              {Number(e.http_status) > 0 && <StatusBadge status={Number(e.http_status)} />}
            </div>
          </li>
        ))}
      </ul>
    </Card>
  )
}

const DETAIL_FIELDS: (keyof LogEvent)[] = [
  '_tenant', 'ts', 'received_at', 'severity', 'category', 'event_type', 'source', 'message',
  'http_status', 'duration_ms', 'route', 'model', 'tokens_input', 'tokens_output',
  'user_id', 'user_email', 'session_id', 'request_id', 'entity_type', 'entity_id',
  'error_code', 'app_version', 'server', 'ip', 'user_agent', 'event_id',
]

function EventDrawer({ event, onClose }: { event: LogEvent | null; onClose: () => void }) {
  let attrs: string | null = null
  if (event?.attributes) {
    try {
      attrs = JSON.stringify(JSON.parse(event.attributes), null, 2)
    } catch {
      attrs = event.attributes
    }
  }
  return (
    <Drawer open={!!event} onClose={onClose} title={event ? `${event.category} / ${event.event_type}` : ''}>
      {event && (
        <div className="space-y-4">
          <dl className="grid grid-cols-1 gap-x-4 gap-y-2 text-sm sm:grid-cols-2">
            {DETAIL_FIELDS.filter((f) => {
              const v = event[f]
              return v !== undefined && v !== null && v !== '' && v !== 0
            }).map((f) => (
              <div key={String(f)} className="min-w-0">
                <dt className="text-xs font-medium uppercase tracking-wide text-zinc-400">{String(f)}</dt>
                <dd className="break-words text-zinc-800">
                  {f === 'severity' ? <SeverityBadge severity={String(event[f])} /> : String(event[f])}
                </dd>
              </div>
            ))}
          </dl>
          {attrs && (
            <div>
              <div className="mb-1 text-xs font-medium uppercase tracking-wide text-zinc-400">attributes</div>
              <pre className="overflow-x-auto rounded-md bg-zinc-900 p-3 text-xs leading-relaxed text-zinc-100">
                {attrs}
              </pre>
            </div>
          )}
        </div>
      )}
    </Drawer>
  )
}
