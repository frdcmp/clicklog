import type { ReactNode } from 'react'
import { cn } from '../../lib/cn'

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        'inline-block size-4 animate-spin rounded-full border-2 border-zinc-300 border-t-accent-600',
        className,
      )}
    />
  )
}

export function EmptyState({ title, hint }: { title: string; hint?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-1 rounded-lg border border-dashed border-zinc-300 bg-white px-6 py-12 text-center">
      <p className="text-sm font-medium text-zinc-600">{title}</p>
      {hint && <p className="max-w-sm text-xs text-zinc-400">{hint}</p>}
    </div>
  )
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
      {children}
    </div>
  )
}
