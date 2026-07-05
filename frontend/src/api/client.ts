// Low-level fetch client for the admin API. No react-query here — just the raw
// request primitive, token handling, and error shaping. All admin endpoints live
// under /v1/admin (proxied to the ingest-api by the Vite dev server).

const BASE = '/v1/admin'
const TOKEN_KEY = 'frdcmp_token'

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}
export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token)
}
export function clearToken() {
  localStorage.removeItem(TOKEN_KEY)
}

// The AuthProvider registers a handler so a 401 anywhere logs the user out.
let onUnauthorized: (() => void) | null = null
export function setUnauthorizedHandler(fn: (() => void) | null) {
  onUnauthorized = fn
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
    this.name = 'ApiError'
  }
}

export type QueryParams = Record<string, string | number | boolean | undefined | null>

function buildQuery(params?: QueryParams): string {
  if (!params) return ''
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') sp.set(k, String(v))
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}

interface RequestOpts {
  method?: string
  body?: unknown
  params?: QueryParams
  /** Attach the bearer token. Default true. */
  auth?: boolean
}

export async function request<T>(path: string, opts: RequestOpts = {}): Promise<T> {
  const { method = 'GET', body, params, auth = true } = opts
  const headers: Record<string, string> = {}
  if (body !== undefined) headers['content-type'] = 'application/json'
  if (auth) {
    const token = getToken()
    if (token) headers['authorization'] = `Bearer ${token}`
  }

  const res = await fetch(`${BASE}${path}${buildQuery(params)}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })

  if (res.status === 401 && auth) {
    onUnauthorized?.()
  }

  const text = await res.text()
  const data = text ? safeParse(text) : null
  if (!res.ok) {
    let msg = res.statusText
    if (data && typeof data === 'object' && 'error' in data) {
      msg = String((data as { error: unknown }).error)
    }
    throw new ApiError(res.status, msg)
  }
  return data as T
}

function safeParse(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return { error: text }
  }
}
