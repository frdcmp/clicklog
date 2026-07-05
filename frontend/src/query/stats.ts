import { useQuery } from '@tanstack/react-query'
import { getStats, type StatsQuery } from '../api/stats'
import { qk } from './keyFactory'

export function useStats(q: StatsQuery, enabled = true) {
  return useQuery({
    queryKey: qk.stats(q),
    queryFn: () => getStats(q),
    enabled,
    select: (d) => d.stats,
  })
}
