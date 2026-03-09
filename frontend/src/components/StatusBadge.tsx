import type { VmStatus } from '../api'

const colors: Record<VmStatus, string> = {
  running: 'bg-green-900 text-green-300',
  starting: 'bg-yellow-900 text-yellow-300',
  stopping: 'bg-yellow-900 text-yellow-300',
  stopped: 'bg-zinc-800 text-zinc-400',
  snapshotting: 'bg-blue-900 text-blue-300',
  error: 'bg-red-900 text-red-300',
}

export function StatusBadge({ status }: { status: VmStatus }) {
  return (
    <span className={`text-xs font-mono px-2 py-0.5 rounded ${colors[status]}`}>
      {status}
    </span>
  )
}
