import { useParams, useNavigate } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { getVm, startVm, stopVm, snapshotVm, deleteVm, ApiError } from '../api'
import { StatusBadge } from '../components/StatusBadge'

export function VmDetailPage() {
  const { vmId } = useParams({ from: '/_authed/vms/$vmId' })
  const navigate = useNavigate()
  const qc = useQueryClient()

  const { data: vm, isLoading, error } = useQuery({
    queryKey: ['vms', vmId],
    queryFn: () => getVm(vmId),
  })

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['vms', vmId] })
    qc.invalidateQueries({ queryKey: ['vms'] })
  }

  const startMutation = useMutation({ mutationFn: () => startVm(vmId), onSuccess: invalidate })
  const stopMutation = useMutation({ mutationFn: () => stopVm(vmId), onSuccess: invalidate })
  const snapshotMutation = useMutation({ mutationFn: () => snapshotVm(vmId), onSuccess: invalidate })
  const deleteMutation = useMutation({
    mutationFn: () => deleteVm(vmId),
    onSuccess: () => navigate({ to: '/vms' }),
  })

  if (isLoading) return <p className="text-zinc-500 text-sm">loading...</p>

  if (error) {
    const status = error instanceof ApiError ? error.status : null
    return (
      <p className="text-red-400 text-sm">
        {status === 404 ? 'vm not found' : 'failed to load vm'}
      </p>
    )
  }

  if (!vm) return null

  const isTransitioning =
    vm.status === 'starting' || vm.status === 'stopping' || vm.status === 'snapshotting'
  const canStart = vm.status === 'stopped' || vm.status === 'error'
  const canStop = vm.status === 'running'
  const canSnapshot = vm.status === 'running'

  const mutError =
    startMutation.error?.message ||
    stopMutation.error?.message ||
    snapshotMutation.error?.message ||
    deleteMutation.error?.message

  return (
    <>
      <div className="flex items-start justify-between mb-6">
        <div>
          <div className="flex items-center gap-3 mb-1">
            <h1 className="text-xl font-semibold">{vm.name}</h1>
            <StatusBadge status={vm.status} />
          </div>
          <p className="text-sm text-zinc-500 font-mono">{vm.subdomain}</p>
        </div>
        <button
          onClick={() => {
            if (confirm('delete this vm?')) deleteMutation.mutate()
          }}
          disabled={deleteMutation.isPending || isTransitioning}
          className="text-sm text-red-400 hover:text-red-300 disabled:opacity-40"
        >
          delete
        </button>
      </div>

      <div className="grid grid-cols-2 gap-3 mb-6 sm:grid-cols-4">
        {[
          ['vcores', vm.vcores],
          ['memory', `${vm.memory_mb} mb`],
          ['ip', vm.ip_address],
          ['port', vm.exposed_port],
        ].map(([label, value]) => (
          <div key={label as string} className="bg-zinc-900 border border-zinc-800 rounded-lg px-4 py-3">
            <p className="text-xs text-zinc-500 mb-1">{label}</p>
            <p className="font-mono text-sm">{value}</p>
          </div>
        ))}
      </div>

      {mutError && <p className="text-sm text-red-400 mb-4">{mutError}</p>}

      <div className="flex gap-3">
        <button
          onClick={() => startMutation.mutate()}
          disabled={!canStart || isTransitioning || startMutation.isPending}
          className="bg-green-700 hover:bg-green-600 disabled:opacity-40 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
        >
          {startMutation.isPending ? 'starting...' : 'start'}
        </button>
        <button
          onClick={() => stopMutation.mutate()}
          disabled={!canStop || isTransitioning || stopMutation.isPending}
          className="bg-zinc-700 hover:bg-zinc-600 disabled:opacity-40 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
        >
          {stopMutation.isPending ? 'stopping...' : 'stop'}
        </button>
        <button
          onClick={() => snapshotMutation.mutate()}
          disabled={!canSnapshot || isTransitioning || snapshotMutation.isPending}
          className="bg-blue-700 hover:bg-blue-600 disabled:opacity-40 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
        >
          {snapshotMutation.isPending ? 'snapshotting...' : 'snapshot'}
        </button>
      </div>
    </>
  )
}
