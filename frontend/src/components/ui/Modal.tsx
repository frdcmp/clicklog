import type { ReactNode } from 'react'
import { useEffect } from 'react'
import { cn } from '../../lib/cn'
import { Button } from './Button'

function useEscClose(open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) return
    const h = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [open, onClose])
}

/** Centered dialog for short forms/confirmations. */
export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
}: {
  open: boolean
  onClose: () => void
  title: ReactNode
  children: ReactNode
  footer?: ReactNode
}) {
  useEscClose(open, onClose)
  if (!open) return null
  return (
    <div className="fixed inset-0 z-50 flex items-end justify-center sm:items-center">
      <div className="absolute inset-0 bg-zinc-900/40" onClick={onClose} />
      <div className="relative z-10 w-full max-w-lg rounded-t-xl bg-white shadow-xl sm:rounded-xl">
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-3">
          <h2 className="text-sm font-semibold text-zinc-900">{title}</h2>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close">✕</Button>
        </div>
        <div className="px-5 py-4">{children}</div>
        {footer && <div className="flex justify-end gap-2 border-t border-zinc-200 px-5 py-3">{footer}</div>}
      </div>
    </div>
  )
}

/** Right-side slide-over for detail views (log event inspection). */
export function Drawer({
  open,
  onClose,
  title,
  children,
}: {
  open: boolean
  onClose: () => void
  title: ReactNode
  children: ReactNode
}) {
  useEscClose(open, onClose)
  return (
    <div className={cn('fixed inset-0 z-50', open ? '' : 'pointer-events-none')}>
      <div
        className={cn('absolute inset-0 bg-zinc-900/40 transition-opacity', open ? 'opacity-100' : 'opacity-0')}
        onClick={onClose}
      />
      <div
        className={cn(
          'absolute right-0 top-0 flex h-full w-full max-w-xl flex-col bg-white shadow-xl transition-transform',
          open ? 'translate-x-0' : 'translate-x-full',
        )}
      >
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-3">
          <h2 className="truncate text-sm font-semibold text-zinc-900">{title}</h2>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close">✕</Button>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
      </div>
    </div>
  )
}
