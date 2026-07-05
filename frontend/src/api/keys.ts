import { request } from './client'
import type { ApiKey } from '../types'

export const listKeys = () => request<ApiKey[]>('/keys')

export interface MintResult {
  id: string
  tenant: string
  key: string
  note?: string
}
export const mintKey = (tenant: string, label: string) =>
  request<MintResult>('/keys', { method: 'POST', body: { tenant, label } })

export const revokeKey = (id: string) =>
  request<{ revoked: boolean }>(`/keys/${encodeURIComponent(id)}`, { method: 'DELETE' })
