import {
  createRouter,
  createRootRoute,
  createRoute,
  redirect,
  Outlet,
} from '@tanstack/react-router'
import { queryClient } from './queryClient'
import { getMe } from './api'
import { RootLayout } from './layouts/RootLayout'
import { AuthedLayout } from './layouts/AuthedLayout'
import { LoginPage } from './pages/LoginPage'
import { SignupPage } from './pages/SignupPage'
import { VmListPage } from './pages/VmListPage'
import { VmDetailPage } from './pages/VmDetailPage'
import { AccountPage } from './pages/AccountPage'

const rootRoute = createRootRoute({
  component: RootLayout,
})

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  beforeLoad: async () => {
    try {
      await queryClient.fetchQuery({ queryKey: ['me'], queryFn: getMe, staleTime: 30_000 })
      throw redirect({ to: '/vms' })
    } catch (e: unknown) {
      if (e && typeof e === 'object' && 'isRedirect' in e) throw e
      throw redirect({ to: '/login' })
    }
  },
  component: () => <Outlet />,
})

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  component: LoginPage,
})

const signupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/signup',
  component: SignupPage,
})

// auth-guarded layout
const authedRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: '_authed',
  beforeLoad: async ({ location }) => {
    try {
      await queryClient.fetchQuery({ queryKey: ['me'], queryFn: getMe, staleTime: 30_000 })
    } catch {
      throw redirect({ to: '/login', search: { redirect: location.href } })
    }
  },
  component: AuthedLayout,
})

const vmListRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: '/vms',
  component: VmListPage,
})

const vmDetailRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: '/vms/$vmId',
  component: VmDetailPage,
})

const accountRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: '/account',
  component: AccountPage,
})

const routeTree = rootRoute.addChildren([
  indexRoute,
  loginRoute,
  signupRoute,
  authedRoute.addChildren([vmListRoute, vmDetailRoute, accountRoute]),
])

export const router = createRouter({ routeTree })

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
