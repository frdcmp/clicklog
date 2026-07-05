import { useMemo } from 'react'
import { Link } from 'react-router-dom'
import { PageHeader } from '../components/layout/PageHeader'
import { Card, StatCard } from '../components/ui/Card'
import { Spinner } from '../components/ui/Feedback'
import { SeverityBadge } from '../components/ui/Badge'
import { useTenants } from '../query/tenants'
import { useKeys } from '../query/keys'
import { useStats } from '../query/stats'
import { fmtBucket, fmtNumber } from '../lib/time'
import type { StatRow } from '../types'

const SEVERITY_ORDER = ['error', 'warn', 'info', 'debug']

export function DashboardPage() {
  const tenants = useTenants()
  const keys = useKeys()
  const bySeverity = useStats({ tenant: '*', group_by: 'severity', from: '-24h', metric: 'count' })
  const timeline = useStats({ tenant: '*', group_by: 'severity', interval: '1h', from: '-24h', metric: 'count' })

  const totalEvents = useMemo(
    () => (bySeverity.data ?? []).reduce((s, r) => s + Number(r.value || 0), 0),
    [bySeverity.data],
  )
  const activeKeys = useMemo(() => (keys.data ?? []).filter((k) => k.active === 1).length, [keys.data])

  const severityRows = useMemo(() => {
    const map = new Map<string, number>()
    for (const r of bySeverity.data ?? []) map.set(String(r.group_value), Number(r.value || 0))
    return SEVERITY_ORDER.filter((s) => map.has(s)).map((s) => ({ severity: s, count: map.get(s)! }))
  }, [bySeverity.data])

  const buckets = useMemo(() => aggregateBuckets(timeline.data ?? []), [timeline.data])

  return (
    <div>
      <PageHeader title="Dashboard" description="Overview of the ingest gateway across all tenants (last 24h)." />

      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
        <StatCard label="Events · 24h" value={bySeverity.isLoading ? '…' : fmtNumber(totalEvents)} />
        <StatCard
          label="Tenants"
          value={tenants.isLoading ? '…' : fmtNumber(tenants.data?.length ?? 0)}
          sub={`${tenants.data?.filter((t) => t.has_events).length ?? 0} with events`}
        />
        <StatCard label="Active keys" value={keys.isLoading ? '…' : fmtNumber(activeKeys)} />
        <StatCard label="Errors · 24h" value={fmtNumber(severityRows.find((r) => r.severity === 'error')?.count ?? 0)} />
      </div>

      <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
        <Card className="p-4 lg:col-span-2">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-sm font-semibold text-zinc-800">Events per hour</h2>
            <span className="text-xs text-zinc-400">last 24h · all tenants</span>
          </div>
          {timeline.isLoading ? (
            <div className="flex h-40 items-center justify-center">
              <Spinner />
            </div>
          ) : buckets.length === 0 ? (
            <p className="py-12 text-center text-sm text-zinc-400">No events in the last 24h.</p>
          ) : (
            <BarChart buckets={buckets} />
          )}
        </Card>

        <Card className="p-4">
          <h2 className="mb-3 text-sm font-semibold text-zinc-800">By severity</h2>
          {severityRows.length === 0 ? (
            <p className="py-8 text-center text-sm text-zinc-400">No data.</p>
          ) : (
            <ul className="space-y-2">
              {severityRows.map((r) => (
                <li key={r.severity} className="flex items-center justify-between">
                  <SeverityBadge severity={r.severity} />
                  <span className="text-sm font-medium tabular-nums text-zinc-700">{fmtNumber(r.count)}</span>
                </li>
              ))}
            </ul>
          )}
          <Link to="/logs" className="mt-4 inline-block text-xs font-medium text-accent-600 hover:underline">
            Search logs →
          </Link>
        </Card>
      </div>
    </div>
  )
}

interface Bucket {
  bucket: string
  total: number
}

function aggregateBuckets(rows: StatRow[]): Bucket[] {
  const map = new Map<string, number>()
  for (const r of rows) {
    if (!r.bucket) continue
    map.set(r.bucket, (map.get(r.bucket) ?? 0) + Number(r.value || 0))
  }
  return [...map.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([bucket, total]) => ({ bucket, total }))
}

function BarChart({ buckets }: { buckets: Bucket[] }) {
  const max = Math.max(1, ...buckets.map((b) => b.total))
  return (
    <div className="flex h-40 items-end gap-0.5 overflow-x-auto">
      {buckets.map((b) => (
        <div key={b.bucket} className="group flex h-full min-w-[8px] flex-1 flex-col items-center justify-end">
          <div
            className="w-full rounded-t bg-accent-500/80 transition-colors group-hover:bg-accent-600"
            style={{ height: `${Math.max(2, (b.total / max) * 100)}%` }}
            title={`${fmtBucket(b.bucket)} · ${fmtNumber(b.total)}`}
          />
        </div>
      ))}
    </div>
  )
}
