import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Vm } from '../api'

export function useVmEvents() {
  const qc = useQueryClient()

  useEffect(() => {
    const es = new EventSource('/api/events')

    es.addEventListener('vm_status', (e: MessageEvent) => {
      const { vm_id, status } = JSON.parse(e.data) as { vm_id: string; status: string }

      qc.setQueryData<Vm>(['vms', vm_id], (old) =>
        old ? { ...old, status: status as Vm['status'] } : old
      )
      qc.setQueryData<Vm[]>(['vms'], (old) =>
        old?.map((vm) => (vm.id === vm_id ? { ...vm, status: status as Vm['status'] } : vm))
      )
    })

    return () => es.close()
  }, [qc])
}
