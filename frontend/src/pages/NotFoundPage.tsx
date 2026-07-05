import { Link } from 'react-router-dom'

export function NotFoundPage() {
  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-3 text-center">
      <div className="text-4xl font-semibold text-zinc-300">404</div>
      <p className="text-sm text-zinc-500">This page does not exist.</p>
      <Link to="/" className="text-sm font-medium text-accent-600 hover:underline">
        Back to dashboard
      </Link>
    </div>
  )
}
