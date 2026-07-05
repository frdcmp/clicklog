import { request } from './client'
import type { Tenant } from '../types'

export const listTenants = () => request<{ tenants: Tenant[] }>('/tenants')
