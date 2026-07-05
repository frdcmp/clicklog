import { Navigate, Outlet } from 'react-router-dom'
import { useAuth } from './useAuth'

export function ProtectedRoute() {
  const { isAuthed } = useAuth()
  if (!isAuthed) return <Navigate to="/login" replace />
  return <Outlet />
}
