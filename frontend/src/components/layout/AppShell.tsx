import { useState } from 'react'
import { NavLink, Outlet } from 'react-router-dom'
import { cn } from '../../lib/cn'
import { useAuth } from '../../auth/useAuth'
import { Button } from '../ui/Button'

const NAV = [
  { to: '/', label: 'Dashboard', end: true, icon: '▦' },
  { to: '/keys', label: 'API Keys', icon: '🔑' },
  { to: '/logs', label: 'Logs', icon: '🔎' },
  { to: '/docs', label: 'Docs', icon: '📖' },
]

function NavItems({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <nav className="flex flex-col gap-1">
      {NAV.map((n) => (
        <NavLink
          key={n.to}
          to={n.to}
          end={n.end}
          onClick={onNavigate}
          className={({ isActive }) =>
            cn(
              'flex items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium transition-colors',
              isActive ? 'bg-accent-50 text-accent-700' : 'text-zinc-600 hover:bg-zinc-100',
            )
          }
        >
          <span className="text-base leading-none">{n.icon}</span>
          {n.label}
        </NavLink>
      ))}
    </nav>
  )
}

function Brand() {
  return (
    <div className="flex items-center gap-2 px-2">
      <span className="grid size-7 place-items-center rounded-md bg-accent-600 text-sm font-bold text-white">f</span>
      <div className="leading-tight">
        <div className="text-sm font-semibold text-zinc-900">clicklog</div>
        <div className="text-[10px] uppercase tracking-wide text-zinc-400">admin</div>
      </div>
    </div>
  )
}

export function AppShell() {
  const { email, logout } = useAuth()
  const [mobileOpen, setMobileOpen] = useState(false)

  return (
    <div className="flex h-full">
      {/* Desktop sidebar */}
      <aside className="hidden w-60 shrink-0 flex-col border-r border-zinc-200 bg-white p-3 md:flex">
        <div className="py-2">
          <Brand />
        </div>
        <div className="mt-4 flex-1">
          <NavItems />
        </div>
        <div className="border-t border-zinc-200 pt-3">
          <div className="truncate px-2 pb-2 text-xs text-zinc-400" title={email ?? ''}>
            {email}
          </div>
          <Button variant="secondary" size="sm" className="w-full" onClick={logout}>
            Sign out
          </Button>
        </div>
      </aside>

      {/* Mobile drawer */}
      {mobileOpen && (
        <div className="fixed inset-0 z-40 md:hidden">
          <div className="absolute inset-0 bg-zinc-900/40" onClick={() => setMobileOpen(false)} />
          <aside className="absolute left-0 top-0 flex h-full w-64 flex-col bg-white p-3 shadow-xl">
            <div className="py-2">
              <Brand />
            </div>
            <div className="mt-4 flex-1">
              <NavItems onNavigate={() => setMobileOpen(false)} />
            </div>
            <div className="border-t border-zinc-200 pt-3">
              <div className="truncate px-2 pb-2 text-xs text-zinc-400">{email}</div>
              <Button variant="secondary" size="sm" className="w-full" onClick={logout}>
                Sign out
              </Button>
            </div>
          </aside>
        </div>
      )}

      <div className="flex min-w-0 flex-1 flex-col">
        {/* Mobile top bar */}
        <header className="flex items-center justify-between border-b border-zinc-200 bg-white px-4 py-2.5 md:hidden">
          <button
            className="rounded-md p-1.5 text-zinc-600 hover:bg-zinc-100"
            onClick={() => setMobileOpen(true)}
            aria-label="Open menu"
          >
            ☰
          </button>
          <Brand />
          <span className="w-8" />
        </header>

        <main className="min-w-0 flex-1 overflow-y-auto">
          <div className="mx-auto max-w-7xl p-4 sm:p-6">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  )
}
