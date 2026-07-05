import { QueryClient } from '@tanstack/react-query'
import { ApiError } from '../api/client'

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
      retry: (failureCount, error) => {
        // Never retry auth failures — let the 401 handler log out instead.
        if (error instanceof ApiError && (error.status === 401 || error.status === 400)) return false
        return failureCount < 2
      },
    },
  },
})
