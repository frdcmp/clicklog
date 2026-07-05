import type { ReactNode } from 'react'
import { cn } from '../../lib/cn'

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cn('rounded-lg border border-zinc-200 bg-white', className)}>{children}</div>
  )
}

export function StatCard({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <Card className="p-4">
      <div className="text-xs font-medium uppercase tracking-wide text-zinc-400">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-zinc-900 tabular-nums">{value}</div>
      {sub && <div className="mt-0.5 text-xs text-zinc-400">{sub}</div>}
    </Card>
  )
}
