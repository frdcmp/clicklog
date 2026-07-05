// Time helpers for the log UI: relative-range presets + display formatting.

export interface RangePreset {
  label: string
  value: string // passed as `from` (to defaults to now)
}

export const RANGE_PRESETS: RangePreset[] = [
  { label: '15m', value: '-15m' },
  { label: '1h', value: '-1h' },
  { label: '6h', value: '-6h' },
  { label: '24h', value: '-24h' },
  { label: '7d', value: '-7d' },
  { label: '30d', value: '-30d' },
]

/** Format a ClickHouse timestamp (`YYYY-MM-DD HH:MM:SS.sss`, UTC) for display. */
export function fmtTs(ts: string | undefined): string {
  if (!ts) return '—'
  // ClickHouse returns naive UTC; make it explicit so Date parses correctly.
  const iso = ts.includes('T') ? ts : ts.replace(' ', 'T') + 'Z'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return ts
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

/** Short bucket label for chart axes. */
export function fmtBucket(ts: string | undefined): string {
  if (!ts) return ''
  const iso = ts.includes('T') ? ts : ts.replace(' ', 'T') + 'Z'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return ts
  return d.toLocaleString(undefined, { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' })
}

export function fmtNumber(n: number): string {
  return new Intl.NumberFormat().format(n)
}
