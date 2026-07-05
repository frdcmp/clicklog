import { request } from './client'
import type { LoginResponse } from '../types'

export const login = (email: string, password: string) =>
  request<LoginResponse>('/login', { method: 'POST', body: { email, password }, auth: false })

export const getMe = () => request<{ email: string }>('/me')
