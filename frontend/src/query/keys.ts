import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { listKeys, mintKey, revokeKey } from '../api/keys'
import { qk } from './keyFactory'

export function useKeys() {
  return useQuery({ queryKey: qk.keys, queryFn: listKeys })
}

export function useMintKey() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (vars: { tenant: string; label: string }) => mintKey(vars.tenant, vars.label),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.keys })
      qc.invalidateQueries({ queryKey: qk.tenants })
    },
  })
}

export function useRevokeKey() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => revokeKey(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.keys })
      qc.invalidateQueries({ queryKey: qk.tenants })
    },
  })
}
