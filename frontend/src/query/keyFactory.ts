import type { EventsQuery } from '../api/events'
import type { StatsQuery } from '../api/stats'

// Central query-key factory so invalidations stay consistent.
export const qk = {
  me: ['me'] as const,
  keys: ['keys'] as const,
  tenants: ['tenants'] as const,
  events: (q: EventsQuery) => ['events', q] as const,
  event: (tenant: string, id: string) => ['event', tenant, id] as const,
  stats: (q: StatsQuery) => ['stats', q] as const,
}
