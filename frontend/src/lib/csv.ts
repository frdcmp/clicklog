import type { LogEvent } from '../types'

const COLUMNS: (keyof LogEvent)[] = [
  '_tenant',
  'ts',
  'severity',
  'category',
  'event_type',
  'source',
  'message',
  'http_status',
  'duration_ms',
  'route',
  'model',
  'tokens_input',
  'tokens_output',
  'user_id',
  'request_id',
  'error_code',
  'event_id',
  'attributes',
]

function cell(v: unknown): string {
  if (v === undefined || v === null) return ''
  const s = String(v)
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s
}

/** Serialize the loaded events to CSV and trigger a download. */
export function downloadEventsCsv(events: LogEvent[], filename = 'events.csv') {
  const header = COLUMNS.join(',')
  const rows = events.map((e) => COLUMNS.map((c) => cell(e[c])).join(','))
  const blob = new Blob([[header, ...rows].join('\n')], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}
