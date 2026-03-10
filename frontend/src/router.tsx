import {
  createRouter,
  createRootRoute,
  createRoute,
  redirect,
  isRedirect,
} from "@tanstack/react-router";
import { queryClient } from "./queryClient";
import { getMe } from "./api";
import { RootLayout } from "./layouts/RootLayout";
import { AuthedLayout } from "./layouts/AuthedLayout";
import { LoginPage } from "./pages/LoginPage";
import { SignupPage } from "./pages/SignupPage";
import { VmListPage } from "./pages/VmListPage";
import { VmDetailPage } from "./pages/VmDetailPage";
import { AccountLayout } from "./pages/AccountPage";
import { IdentityPage } from "./pages/IdentityPage";
import { ThemesPage } from "./pages/ThemesPage";
import { SshKeysPage } from "./pages/SshKeysPage";
import { CliAuthPage } from "./pages/CliAuthPage";
import HomePage from "./pages/HomePage";

const rootRoute = createRootRoute({
  component: RootLayout,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: async () => {
    try {
      await queryClient.fetchQuery({
        queryKey: ["me"],
        queryFn: getMe,
        staleTime: 30_000,
      });
      // authenticated — send to vms dashboard
      throw redirect({ to: "/vms" });
    } catch (e: unknown) {
      if (isRedirect(e)) throw e;
      // not authenticated — render the public home/landing page
      return;
    }
  },
  component: HomePage,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginPage,
});

const signupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/signup",
  component: SignupPage,
});

const cliAuthRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/cli-auth",
  validateSearch: (search: Record<string, unknown>) => ({
    code: (search.code as string) ?? "",
  }),
  component: CliAuthPage,
});

// auth-guarded layout
const authedRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "_authed",
  beforeLoad: async ({ location }) => {
    try {
      await queryClient.fetchQuery({
        queryKey: ["me"],
        queryFn: getMe,
        staleTime: 30_000,
      });
    } catch {
      throw redirect({ to: "/login", search: { redirect: location.href } });
    }
  },
  component: AuthedLayout,
});

const vmListRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: "/vms",
  component: VmListPage,
});

const vmDetailRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: "/vms/$vmId",
  component: VmDetailPage,
});

const accountRoute = createRoute({
  getParentRoute: () => authedRoute,
  path: "/account",
  component: AccountLayout,
});

const accountIndexRoute = createRoute({
  getParentRoute: () => accountRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/account/identity" });
  },
  component: () => null,
});

const accountIdentityRoute = createRoute({
  getParentRoute: () => accountRoute,
  path: "/identity",
  component: IdentityPage,
});

const accountThemesRoute = createRoute({
  getParentRoute: () => accountRoute,
  path: "/themes",
  component: ThemesPage,
});

const accountSshKeysRoute = createRoute({
  getParentRoute: () => accountRoute,
  path: "/ssh-keys",
  component: SshKeysPage,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  loginRoute,
  signupRoute,
  cliAuthRoute,
  authedRoute.addChildren([
    vmListRoute,
    vmDetailRoute,
    accountRoute.addChildren([
      accountIndexRoute,
      accountIdentityRoute,
      accountThemesRoute,
      accountSshKeysRoute,
    ]),
  ]),
]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
