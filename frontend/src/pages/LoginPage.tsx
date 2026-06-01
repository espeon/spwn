import { useState, type FormEvent } from "react";
import { useNavigate, Link, useSearch } from "@tanstack/react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { login, getMe, listVms, generateVmAuthToken, ApiError } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { IconGalaxy } from "@tabler/icons-react";

function isVmDomain(url: string): boolean {
  try {
    const hostname = new URL(url).hostname;
    return hostname.endsWith(".spwn.town");
  } catch {
    return false;
  }
}

function getSubdomain(url: string): string | null {
  try {
    const hostname = new URL(url).hostname;
    const suffix = ".spwn.town";
    if (hostname.endsWith(suffix)) {
      return hostname.slice(0, -suffix.length);
    }
    return null;
  } catch {
    return null;
  }
}

export function LoginPage() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as Record<string, string>;
  const redirect = search.redirect || "/vms";
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      await login(email, password);
      const me = await getMe();
      qc.setQueryData(["me"], me);

      // If redirecting to a VM subdomain, generate an auth token.
      if (isVmDomain(redirect)) {
        const subdomain = getSubdomain(redirect);
        if (subdomain) {
          const vms = await listVms();
          const vm = vms.find((v) => v.subdomain === subdomain);
          if (vm) {
            const { token } = await generateVmAuthToken(vm.id);
            const url = new URL(redirect);
            url.searchParams.set("auth_token", token);
            window.location.href = url.toString();
            return;
          }
        }
      }

      // Default: navigate to the redirect URL.
      window.location.href = redirect;
    },
    onError: (err) => {
      if (err instanceof ApiError && err.status === 401) {
        setError("invalid email or password");
      } else {
        setError("something went wrong");
      }
    },
  });

  function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    mutation.mutate();
  }

  return (
    <div className="min-h-screen flex items-center justify-center px-4">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex flex-row items-center justify-center gap-1">
          <IconGalaxy className="size-8!" />
          <h1 className="text-4xl text-center mb-1.5">spwn</h1>
        </div>
        <Card>
          <CardHeader>
            <CardTitle className="">log in</CardTitle>
          </CardHeader>
          <CardContent>
            <form id="login-form" onSubmit={submit} className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="email">email</Label>
                <Input
                  id="email"
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  required
                  autoComplete="email"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="password">password</Label>
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                  autoComplete="current-password"
                />
              </div>
              {error && <p className="text-sm text-destructive">{error}</p>}
            </form>
          </CardContent>
          <CardFooter className="flex-col gap-3">
            <Button
              type="submit"
              form="login-form"
              disabled={mutation.isPending}
              className="w-full"
            >
              {mutation.isPending ? "logging in..." : "log in"}
            </Button>
            <p className="text-sm text-muted-foreground">
              no account?{" "}
              <Link
                to="/signup"
                className="text-foreground underline underline-offset-4 hover:no-underline"
              >
                sign up
              </Link>
            </p>
          </CardFooter>
        </Card>
      </div>
    </div>
  );
}
