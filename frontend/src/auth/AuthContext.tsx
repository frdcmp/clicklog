import { createContext, useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { clearToken, getToken, setToken, setUnauthorizedHandler } from '../api/client'
import { queryClient } from '../query/queryClient'

interface AuthState {
  token: string | null
  email: string | null
  isAuthed: boolean
  login: (token: string, email: string) => void
  logout: () => void
}

// eslint-disable-next-line react-refresh/only-export-components
export const AuthContext = createContext<AuthState | null>(null)

const EMAIL_KEY = 'frdcmp_email'

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setTok] = useState<string | null>(() => getToken())
  const [email, setEmail] = useState<string | null>(() => localStorage.getItem(EMAIL_KEY))

  const logout = useCallback(() => {
    clearToken()
    localStorage.removeItem(EMAIL_KEY)
    setTok(null)
    setEmail(null)
    queryClient.clear()
  }, [])

  const login = useCallback((newToken: string, newEmail: string) => {
    setToken(newToken)
    localStorage.setItem(EMAIL_KEY, newEmail)
    setTok(newToken)
    setEmail(newEmail)
  }, [])

  // Any 401 from the API client forces a logout.
  useEffect(() => {
    setUnauthorizedHandler(logout)
    return () => setUnauthorizedHandler(null)
  }, [logout])

  const value = useMemo<AuthState>(
    () => ({ token, email, isAuthed: !!token, login, logout }),
    [token, email, login, logout],
  )
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}
