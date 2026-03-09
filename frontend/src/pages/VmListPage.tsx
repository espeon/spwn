import { useState, type FormEvent } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { listVms, createVm, type CreateVmRequest } from '../api'
import { StatusBadge } from '../components/StatusBadge'

function CreateVmModal({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient()
  const [name, setName] = useState('')
  const [vcores, setVcores] = useState(1)
  const [memoryMb, setMemoryMb] = useState(512)
  const [port, setPort] = useState(8080)
  const [error, setError] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: (req: CreateVmRequest) => createVm(req),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['vms'] })
      onClose()
    },
    onError: (err) => setError(err.message),
  })

  function submit(e: FormEvent) {
    e.preventDefault()
    setError(null)
    mutation.mutate({ name, vcores, memory_mb: memoryMb, exposed_port: port })
  }

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 px-4">
      <div className="bg-zinc-900 border border-zinc-700 rounded-lg w-full max-w-md p-6">
        <h2 className="text-lg font-semibold mb-4">create vm</h2>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1">
            <label className="text-sm text-zinc-400">name</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-violet-500"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1">
              <label className="text-sm text-zinc-400">vcores</label>
              <input
                type="number"
                min={1}
                max={8}
                value={vcores}
                onChange={(e) => setVcores(Number(e.target.value))}
                className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-violet-500"
              />
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-sm text-zinc-400">memory (mb)</label>
              <input
                type="number"
                min={128}
                step={128}
                value={memoryMb}
                onChange={(e) => setMemoryMb(Number(e.target.value))}
                className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-violet-500"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-sm text-zinc-400">exposed port</label>
            <input
              type="number"
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
              className="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-violet-500"
            />
          </div>
          {error && <p className="text-sm text-red-400">{error}</p>}
          <div className="flex gap-3 mt-2">
            <button
              type="button"
              onClick={onClose}
              className="flex-1 border border-zinc-700 rounded px-4 py-2 text-sm hover:bg-zinc-800 transition-colors"
            >
              cancel
            </button>
            <button
              type="submit"
              disabled={mutation.isPending}
              className="flex-1 bg-violet-600 hover:bg-violet-500 disabled:opacity-50 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
            >
              {mutation.isPending ? 'creating...' : 'create'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

export function VmListPage() {
  const [showCreate, setShowCreate] = useState(false)
  const { data: vms, isLoading, error } = useQuery({ queryKey: ['vms'], queryFn: listVms })

  if (isLoading) return <p className="text-zinc-500 text-sm">loading...</p>
  if (error) return <p className="text-red-400 text-sm">failed to load vms</p>

  return (
    <>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold">virtual machines</h1>
        <button
          onClick={() => setShowCreate(true)}
          className="bg-violet-600 hover:bg-violet-500 text-white rounded px-4 py-2 text-sm font-medium transition-colors"
        >
          + new vm
        </button>
      </div>

      {vms && vms.length === 0 ? (
        <div className="text-center py-24 text-zinc-500">
          <p className="text-sm">no vms yet.</p>
          <button
            onClick={() => setShowCreate(true)}
            className="mt-3 text-violet-400 hover:text-violet-300 text-sm"
          >
            create your first vm
          </button>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {vms?.map((vm) => (
            <Link
              key={vm.id}
              to="/vms/$vmId"
              params={{ vmId: vm.id }}
              className="block bg-zinc-900 border border-zinc-800 hover:border-zinc-600 rounded-lg px-5 py-4 transition-colors"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <span className="font-medium">{vm.name}</span>
                  <StatusBadge status={vm.status} />
                </div>
                <span className="text-xs text-zinc-500 font-mono">{vm.subdomain}</span>
              </div>
              <div className="mt-2 flex gap-4 text-xs text-zinc-500">
                <span>{vm.vcores}c</span>
                <span>{vm.memory_mb}mb</span>
                <span>:{vm.exposed_port}</span>
              </div>
            </Link>
          ))}
        </div>
      )}

      {showCreate && <CreateVmModal onClose={() => setShowCreate(false)} />}
    </>
  )
}
