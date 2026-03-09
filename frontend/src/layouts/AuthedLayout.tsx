import { Outlet, Link, useNavigate } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { getMe, logout } from '../api'

export function AuthedLayout() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe })

  const logoutMutation = useMutation({
    mutationFn: logout,
    onSuccess: () => {
      qc.clear()
      navigate({ to: '/login' })
    },
  })

  return (
    <div className="min-h-screen bg-zinc-950 text-zinc-100">
      <nav className="border-b border-zinc-800 px-6 py-3 flex items-center gap-6">
        <span className="font-mono font-bold text-violet-400 text-lg">spwn</span>
        <Link
          to="/vms"
          className="text-sm text-zinc-400 hover:text-zinc-100 [&.active]:text-zinc-100"
        >
          vms
        </Link>
        <Link
          to="/account"
          className="text-sm text-zinc-400 hover:text-zinc-100 [&.active]:text-zinc-100"
        >
          account
        </Link>
        <div className="ml-auto flex items-center gap-4">
          <span className="text-sm text-zinc-500">{me?.email}</span>
          <button
            onClick={() => logoutMutation.mutate()}
            disabled={logoutMutation.isPending}
            className="text-sm text-zinc-400 hover:text-zinc-100 disabled:opacity-50"
          >
            logout
          </button>
        </div>
      </nav>
      <main className="px-6 py-8 max-w-5xl mx-auto">
        <Outlet />
      </main>
    </div>
  )
}
