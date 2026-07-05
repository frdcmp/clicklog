import { request, type QueryParams } from './client'
import type { EventsResponse, LogEvent } from '../types'

/** Filter/search params for GET /v1/admin/events. `tenant` = name or "*"/"all". */
export interface EventsQuery {
  tenant?: string
  from?: string
  to?: string
  category?: string
  event_type?: string
  severity?: string
  source?: string
  model?: string
  route?: string
  user_id?: string
  request_id?: string
  http_status?: string
  q?: string
  order?: 'asc' | 'desc'
  limit?: number
  cursor?: string
}

export const listEvents = (q: EventsQuery) =>
  request<EventsResponse>('/events', { params: q as QueryParams })

export const getEvent = (tenant: string, id: string) =>
  request<LogEvent>(`/events/${encodeURIComponent(id)}`, { params: { tenant } })
