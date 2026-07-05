import type { InputHTMLAttributes, SelectHTMLAttributes, ReactNode } from 'react'
import { cn } from '../../lib/cn'

const base =
  'w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 ' +
  'placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-accent-500 focus:border-accent-500'

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cn(base, className)} {...rest} />
}

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  children: ReactNode
}
export function Select({ className, children, ...rest }: SelectProps) {
  return (
    <select className={cn(base, 'appearance-none pr-8', className)} {...rest}>
      {children}
    </select>
  )
}

export function Field({
  label,
  htmlFor,
  hint,
  children,
  className,
}: {
  label: string
  htmlFor?: string
  hint?: string
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('space-y-1', className)}>
      <label htmlFor={htmlFor} className="block text-xs font-medium text-zinc-600">
        {label}
      </label>
      {children}
      {hint && <p className="text-xs text-zinc-400">{hint}</p>}
    </div>
  )
}
