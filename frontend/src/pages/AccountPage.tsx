import { useQuery } from '@tanstack/react-query'
import { getMe, listVms } from '../api'

function QuotaBar({ used, limit, label }: { used: number; limit: number; label: string }) {
  const pct = Math.min((used / limit) * 100, 100)
  const color = pct >= 90 ? 'bg-red-500' : pct >= 70 ? 'bg-yellow-500' : 'bg-violet-500'
  return (
    <div>
      <div className="flex justify-between text-xs text-zinc-400 mb-1">
        <span>{label}</span>
        <span>
          {used} / {limit}
        </span>
      </div>
      <div className="h-1.5 bg-zinc-800 rounded-full overflow-hidden">
        <div className={`h-full rounded-full transition-all ${color}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  )
}

export function AccountPage() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe })
  const { data: vms } = useQuery({ queryKey: ['vms'], queryFn: listVms })

  const activeVms = vms?.filter((v) => v.status === 'running' || v.status === 'starting') ?? []
  const usedVcores = activeVms.reduce((s, v) => s + v.vcores, 0)
  const usedMem = activeVms.reduce((s, v) => s + v.memory_mb, 0)

  return (
    <>
      <h1 className="text-xl font-semibold mb-6">account</h1>

      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-6 mb-6">
        <p className="text-sm text-zinc-400 mb-1">email</p>
        <p className="font-mono">{me?.email}</p>
      </div>

      <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-6">
        <h2 className="text-sm font-semibold text-zinc-300 mb-4">quota</h2>
        <div className="flex flex-col gap-4">
          <QuotaBar used={activeVms.length} limit={me?.vm_limit ?? 0} label="vms" />
          <QuotaBar used={usedVcores} limit={me?.vcpu_limit ?? 0} label="vcores" />
          <QuotaBar used={usedMem} limit={me?.mem_limit_mb ?? 0} label="memory (mb)" />
        </div>
      </div>
    </>
  )
}
