// Shared shapes mirroring the ingest-api JSON responses.

export interface ApiKey {
  id: string
  tenant: string
  label: string
  scopes: string
  active: number // 1 = active, 0 = revoked
  created_at: string
  revoked_at: string
}

export interface Tenant {
  tenant: string
  keys: number
  active_keys: number
  has_events: boolean
}

/** A stored log event. Columns mirror the ClickHouse `events` table. */
export interface LogEvent {
  event_id: string
  ts: string
  received_at: string
  source: string
  category: string
  event_type: string
  severity: string
  user_id: string
  user_email: string
  session_id: string
  request_id: string
  entity_type: string
  entity_id: string
  message: string
  error_code: string
  model: string
  tokens_input: number
  tokens_output: number
  duration_ms: number
  http_status: number
  route: string
  app_version: string
  server: string
  ip: string
  user_agent: string
  attributes: string
  /** Present only on cross-tenant ("All") searches. */
  _tenant?: string
  [k: string]: unknown
}

export interface EventsResponse {
  events: LogEvent[]
  next_cursor: string | null
}

export interface StatRow {
  group_value: string | number
  bucket?: string
  value: number
}

export interface StatsResponse {
  stats: StatRow[]
}

export interface LoginResponse {
  token: string
  email: string
  expires_in: number
}

export type Severity = 'debug' | 'info' | 'warn' | 'error'
