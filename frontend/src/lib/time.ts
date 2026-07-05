// ── dateTime utils ────────────────────────────────────────────────────────────
// The backend is ALWAYS UTC: ClickHouse returns naive `YYYY-MM-DD HH:MM:SS[.sss]`
// strings (no offset), the API sometimes RFC3339 or epoch numbers. Parse all of
// them AS UTC, and display them ALWAYS in the browser's local timezone, as
// dd/mm/yyyy + am/pm time. Every date/time shown in the UI must go through
// these helpers — never call `new Date(ts)` / `toLocaleString` on a backend
// value directly (a naive string would silently parse as LOCAL time).

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

/**
 * Parse a backend timestamp into a Date. Accepts ClickHouse naive UTC
 * (`YYYY-MM-DD HH:MM:SS[.sss]`), RFC3339 (with or without offset — offsetless
 * is treated as UTC), and epoch seconds/millis. Returns null if unparseable.
 */
export function parseUtc(ts: string | number | undefined | null): Date | null {
  if (ts === undefined || ts === null || ts === '') return null
  if (typeof ts === 'number') {
    const ms = ts < 1e12 ? ts * 1000 : ts // epoch seconds vs millis
    const d = new Date(ms)
    return Number.isNaN(d.getTime()) ? null : d
  }
  let s = ts.trim()
  if (/^\d{10,13}$/.test(s)) return parseUtc(Number(s))
  if (!s.includes('T')) s = s.replace(' ', 'T')
  // No explicit offset → naive UTC: make it explicit so Date doesn't assume local.
  if (!/([zZ]|[+-]\d{2}:?\d{2})$/.test(s)) s += 'Z'
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? null : d
}

// en-GB pins dd/mm/yyyy ordering; hour12 pins am/pm. Timezone is left to the
// browser default, so values render in the viewer's local time.
const DATE_OPTS: Intl.DateTimeFormatOptions = { day: '2-digit', month: '2-digit', year: 'numeric' }
const TIME_OPTS: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: true }

/** `06/07/2026` (browser tz). */
export function fmtDate(ts: string | number | undefined | null): string {
  const d = parseUtc(ts)
  return d ? d.toLocaleDateString('en-GB', DATE_OPTS) : '—'
}

/** `12:31:03 am` (browser tz). */
export function fmtTime(ts: string | number | undefined | null): string {
  const d = parseUtc(ts)
  return d ? d.toLocaleTimeString('en-GB', TIME_OPTS) : '—'
}

/** `06/07/2026, 12:31:03 am` (browser tz) — the default everywhere. */
export function fmtDateTime(ts: string | number | undefined | null): string {
  const d = parseUtc(ts)
  return d ? d.toLocaleString('en-GB', { ...DATE_OPTS, ...TIME_OPTS }) : '—'
}

/** Short chart-axis label: `06/07, 12:31 am` (browser tz). */
export function fmtBucket(ts: string | number | undefined | null): string {
  const d = parseUtc(ts)
  if (!d) return ''
  return d.toLocaleString('en-GB', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit', hour12: true })
}

export function fmtNumber(n: number): string {
  return new Intl.NumberFormat().format(n)
}
