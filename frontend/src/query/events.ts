import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { getEvent, listEvents, type EventsQuery } from '../api/events'
import { qk } from './keyFactory'

/**
 * Cursor-paginated event search. `filters` should NOT include `cursor` — the
 * hook threads it. Pass `enabled` to gate the query until the user searches.
 */
export function useEventsInfinite(filters: EventsQuery, enabled = true) {
  return useInfiniteQuery({
    queryKey: qk.events(filters),
    queryFn: ({ pageParam }) =>
      listEvents({ ...filters, cursor: (pageParam as string | undefined) || undefined }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor || undefined,
    enabled,
    staleTime: 0,
  })
}

export function useEvent(tenant: string | undefined, id: string | undefined) {
  return useQuery({
    queryKey: qk.event(tenant || '', id || ''),
    queryFn: () => getEvent(tenant!, id!),
    enabled: !!tenant && !!id,
  })
}
