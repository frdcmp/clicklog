import { useMutation, useQuery } from '@tanstack/react-query'
import { getMe, login } from '../api/auth'
import { getToken } from '../api/client'
import { qk } from './keyFactory'

/** Validate the stored token by calling /me. Only runs when a token exists. */
export function useMe() {
  return useQuery({
    queryKey: qk.me,
    queryFn: getMe,
    enabled: !!getToken(),
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useLogin() {
  return useMutation({
    mutationFn: (vars: { email: string; password: string }) => login(vars.email, vars.password),
  })
}
