import { request, type QueryParams } from './client'
import type { StatsResponse } from '../types'

export interface StatsQuery {
  tenant?: string
  group_by: string
  interval?: string
  metric?: string
  from?: string
  to?: string
  severity?: string
  category?: string
}

export const getStats = (q: StatsQuery) =>
  request<StatsResponse>('/stats', { params: q as unknown as QueryParams })
