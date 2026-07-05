import { useState, type FormEvent } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'
import { useAuth } from '../auth/useAuth'
import { useLogin } from '../query/auth'
import { ApiError } from '../api/client'
import { Button } from '../components/ui/Button'
import { Field, Input } from '../components/ui/Field'
import { ErrorNote } from '../components/ui/Feedback'

export function LoginPage() {
  const { isAuthed, login } = useAuth()
  const navigate = useNavigate()
  const mutation = useLogin()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  if (isAuthed) return <Navigate to="/" replace />

  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    mutation.mutate(
      { email, password },
      {
        onSuccess: (res) => {
          login(res.token, res.email)
          navigate('/', { replace: true })
        },
      },
    )
  }

  const errMsg =
    mutation.error instanceof ApiError
      ? mutation.error.status === 503
        ? 'Auth is not configured on the server (JWT_SECRET unset).'
        : mutation.error.message
      : mutation.error
        ? 'Login failed — is the ingest-api reachable?'
        : null

  return (
    <div className="flex min-h-full items-center justify-center bg-zinc-50 px-4 py-12">
      <div className="w-full max-w-sm">
        <div className="mb-6 text-center">
          <img src="/logo.svg" alt="" className="mx-auto mb-3 size-12 [image-rendering:pixelated]" />
          <h1 className="text-lg font-semibold text-zinc-900">clicklog admin</h1>
          <p className="text-sm text-zinc-500">Sign in to manage the ingest gateway.</p>
        </div>

        <form onSubmit={onSubmit} className="space-y-4 rounded-xl border border-zinc-200 bg-white p-6 shadow-sm">
          <Field label="Email" htmlFor="email">
            <Input
              id="email"
              type="email"
              autoComplete="username"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
          </Field>
          <Field label="Password" htmlFor="password">
            <Input
              id="password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </Field>
          {errMsg && <ErrorNote>{errMsg}</ErrorNote>}
          <Button type="submit" className="w-full" loading={mutation.isPending}>
            Sign in
          </Button>
        </form>
      </div>
    </div>
  )
}
