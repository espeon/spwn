import { useState } from "react";
import { useSearch, Link } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { getMe, cliAuthorize, cliDeny, ApiError } from "@/api";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";

type PageState = "idle" | "authorized" | "denied";

export function CliAuthPage() {
  const { code } = useSearch({ from: "/cli-auth" });
  const [pageState, setPageState] = useState<PageState>("idle");

  const { data: me, isLoading: meLoading, isError: notLoggedIn } = useQuery({
    queryKey: ["me"],
    queryFn: getMe,
    retry: false,
  });

  const authorizeMutation = useMutation({
    mutationFn: () => cliAuthorize(code),
    onSuccess: () => setPageState("authorized"),
  });

  const denyMutation = useMutation({
    mutationFn: () => cliDeny(code),
    onSuccess: () => setPageState("denied"),
  });

  const pending =
    authorizeMutation.isPending || denyMutation.isPending;

  if (!code) {
    return <Shell><p className="text-sm text-muted-foreground">missing code parameter</p></Shell>;
  }

  if (meLoading) {
    return (
      <Shell>
        <p className="text-sm text-muted-foreground">loading…</p>
      </Shell>
    );
  }

  if (notLoggedIn || !me) {
    return (
      <Shell>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">authorize CLI access</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">
              you need to be logged in to authorize CLI access.
            </p>
          </CardContent>
          <CardFooter>
            <Link
              to="/login"
              search={{ redirect: `/cli-auth?code=${code}` }}
              className="text-sm underline underline-offset-4"
            >
              log in
            </Link>
          </CardFooter>
        </Card>
      </Shell>
    );
  }

  if (pageState === "authorized") {
    return (
      <Shell>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">authorized</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">
              CLI access granted. you can close this tab and return to your
              terminal.
            </p>
          </CardContent>
        </Card>
      </Shell>
    );
  }

  if (pageState === "denied") {
    return (
      <Shell>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">denied</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">
              CLI access denied. you can close this tab.
            </p>
          </CardContent>
        </Card>
      </Shell>
    );
  }

  const authError =
    authorizeMutation.error instanceof ApiError
      ? authorizeMutation.error
      : denyMutation.error instanceof ApiError
        ? denyMutation.error
        : null;

  return (
    <Shell>
      <Card>
        <CardHeader>
          <CardTitle className="text-base">authorize CLI access</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-sm text-muted-foreground">
            grant CLI access as{" "}
            <span className="text-foreground font-medium">{me.email}</span>?
          </p>
          <p className="text-xs text-muted-foreground">
            this will create an API token for the{" "}
            <code className="text-foreground">spwn</code> CLI. you can revoke
            it from your account settings.
          </p>
          {authError?.status === 410 && (
            <p className="text-sm text-destructive">
              this code has expired or already been used. run{" "}
              <code>spwn login</code> again.
            </p>
          )}
          {authError && authError.status !== 410 && (
            <p className="text-sm text-destructive">something went wrong</p>
          )}
        </CardContent>
        <CardFooter className="flex gap-2">
          <Button
            onClick={() => authorizeMutation.mutate()}
            disabled={pending}
            size="sm"
          >
            authorize
          </Button>
          <Button
            variant="outline"
            onClick={() => denyMutation.mutate()}
            disabled={pending}
            size="sm"
          >
            deny
          </Button>
        </CardFooter>
      </Card>
    </Shell>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center px-4 gap-6">
      <h1 className="font-serif text-4xl">spwn</h1>
      <div className="w-full max-w-sm">{children}</div>
    </div>
  );
}
