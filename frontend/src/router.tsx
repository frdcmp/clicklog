import { createBrowserRouter } from 'react-router-dom'
import { AppShell } from './components/layout/AppShell'
import { ProtectedRoute } from './auth/ProtectedRoute'
import { LoginPage } from './pages/LoginPage'
import { DashboardPage } from './pages/DashboardPage'
import { KeysPage } from './pages/KeysPage'
import { LogsPage } from './pages/LogsPage'
import { DocsPage } from './pages/DocsPage'
import { NotFoundPage } from './pages/NotFoundPage'

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  {
    element: <ProtectedRoute />,
    children: [
      {
        element: <AppShell />,
        children: [
          { index: true, element: <DashboardPage /> },
          { path: 'keys', element: <KeysPage /> },
          { path: 'logs', element: <LogsPage /> },
          { path: 'docs', element: <DocsPage /> },
        ],
      },
    ],
  },
  { path: '*', element: <NotFoundPage /> },
])
