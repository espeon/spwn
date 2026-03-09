import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { getMe } from '@/api'

export function useTheme() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe })

  useEffect(() => {
    const theme = me?.theme ?? 'catppuccin-latte'
    document.documentElement.setAttribute('data-theme', theme)
  }, [me?.theme])
}
