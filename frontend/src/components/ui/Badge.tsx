import { cn } from '../../lib/cn'

const severityStyles: Record<string, string> = {
  debug: 'bg-zinc-100 text-zinc-600',
  info: 'bg-sky-100 text-sky-700',
  warn: 'bg-amber-100 text-amber-800',
  error: 'bg-red-100 text-red-700',
}

export function SeverityBadge({ severity }: { severity: string }) {
  const s = severityStyles[severity] ?? 'bg-zinc-100 text-zinc-600'
  return (
    <span className={cn('inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium', s)}>
      {severity || 'info'}
    </span>
  )
}

export function Badge({
  children,
  tone = 'neutral',
  className,
}: {
  children: React.ReactNode
  tone?: 'neutral' | 'accent' | 'green' | 'red'
  className?: string
}) {
  const tones = {
    neutral: 'bg-zinc-100 text-zinc-600',
    accent: 'bg-accent-100 text-accent-700',
    green: 'bg-emerald-100 text-emerald-700',
    red: 'bg-red-100 text-red-700',
  }
  return (
    <span
      className={cn('inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium', tones[tone], className)}
    >
      {children}
    </span>
  )
}

/** HTTP status pill coloured by class. */
export function StatusBadge({ status }: { status: number }) {
  if (!status) return <span className="text-zinc-300">—</span>
  const tone =
    status >= 500 ? 'bg-red-100 text-red-700'
    : status >= 400 ? 'bg-amber-100 text-amber-800'
    : status >= 300 ? 'bg-sky-100 text-sky-700'
    : 'bg-emerald-100 text-emerald-700'
  return <span className={cn('inline-flex rounded px-1.5 py-0.5 text-xs font-medium tabular-nums', tone)}>{status}</span>
}
