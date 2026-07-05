import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { cn } from '../../lib/cn'

interface Toast {
  id: number
  message: string
  tone: 'success' | 'error'
}

interface ToastApi {
  success: (msg: string) => void
  error: (msg: string) => void
}

const ToastContext = createContext<ToastApi | null>(null)

// eslint-disable-next-line react-refresh/only-export-components
export function useToast() {
  const ctx = useContext(ToastContext)
  if (!ctx) throw new Error('useToast must be used within <ToastProvider>')
  return ctx
}

let nextId = 1

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([])

  const push = useCallback((message: string, tone: Toast['tone']) => {
    const id = nextId++
    setToasts((t) => [...t, { id, message, tone }])
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4000)
  }, [])

  const api: ToastApi = {
    success: (m) => push(m, 'success'),
    error: (m) => push(m, 'error'),
  }

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="fixed bottom-4 right-4 z-[60] flex flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={cn(
              'max-w-sm rounded-md px-3.5 py-2.5 text-sm shadow-lg',
              t.tone === 'success' ? 'bg-zinc-900 text-white' : 'bg-red-600 text-white',
            )}
          >
            {t.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}
