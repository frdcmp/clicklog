import { useQuery } from '@tanstack/react-query'
import { listTenants } from '../api/tenants'
import { qk } from './keyFactory'

export function useTenants() {
  return useQuery({
    queryKey: qk.tenants,
    queryFn: listTenants,
    select: (d) => d.tenants,
  })
}
