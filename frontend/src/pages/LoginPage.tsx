import { useState, type FormEvent } from 'react'
import { useNavigate, Link } from '@tanstack/react-router'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { login, getMe, ApiError } from '../api'

export function LoginPage() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: () => login(email, password),
    onSuccess: async () => {
      const me = await getMe()
      qc.setQueryData(['me'], me)
      navigate({ to: '/vms' })
    },
    onError: (err) => {
      if (err instanceof ApiError && err.status === 401) {
        setError('invalid email or password')
      } else {
        setError('something went wrong')
      }
    },
  })

  function submit(e: FormEvent) {
    e.preventDefault()
    setError(null)
    mutation.mutate()
  }

  return (
    <div className="min-h-screen bg-zinc-950 flex items-center justify-center px-4">
      <div className="w-full max-w-sm">
        <h1 className="font-mono font-bold text-violet-400 text-2xl mb-8">spwn</h1>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1">
            <label className="text-sm text-zinc-400">email</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              className="bg-zinc-900 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-violet-500"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-sm text-zinc-400">password</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              className="bg-zinc-900 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:border-violet-500"
            />
          </div>
          {error && <p className="text-sm text-red-400">{error}</p>}
          <button
            type="submit"
            disabled={mutation.isPending}
            className="bg-violet-600 hover:bg-violet-500 disabled:opacity-50 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
          >
            {mutation.isPending ? 'logging in...' : 'log in'}
          </button>
        </form>
        <p className="mt-6 text-sm text-zinc-500">
          no account?{' '}
          <Link to="/signup" className="text-violet-400 hover:text-violet-300">
            sign up
          </Link>
        </p>
      </div>
    </div>
  )
}
