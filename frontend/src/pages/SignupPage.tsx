import { useState, type FormEvent } from "react";
import { useNavigate, Link } from "@tanstack/react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { signup, login, getMe, ApiError } from "@/api";
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

export function SignupPage() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [inviteCode, setInviteCode] = useState("");
  const [error, setError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: async () => {
      await signup(email, password, username, inviteCode);
      await login(email, password);
      const me = await getMe();
      qc.setQueryData(["me"], me);
    },
    onSuccess: () => navigate({ to: "/vms" }),
    onError: (err) => {
      if (err instanceof ApiError) {
        if (err.status === 403) setError("invalid invite code");
        else if (err.status === 400) setError(err.message);
        else setError("something went wrong");
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
        <h1 className="font-serif text-4xl text-center">spwn</h1>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">create account</CardTitle>
          </CardHeader>
          <CardContent>
            <form id="signup-form" onSubmit={submit} className="space-y-4">
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
                <Label htmlFor="username">username</Label>
                <Input
                  id="username"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value.toLowerCase())}
                  required
                  autoComplete="username"
                  placeholder="letters, numbers, hyphens"
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
                  autoComplete="new-password"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="invite">invite code</Label>
                <Input
                  id="invite"
                  type="text"
                  value={inviteCode}
                  onChange={(e) => setInviteCode(e.target.value)}
                  required
                />
              </div>
              {error && <p className="text-sm text-destructive">{error}</p>}
            </form>
          </CardContent>
          <CardFooter className="flex-col gap-3">
            <Button
              type="submit"
              form="signup-form"
              disabled={mutation.isPending}
              className="w-full"
            >
              {mutation.isPending ? "creating account..." : "create account"}
            </Button>
            <p className="text-sm text-muted-foreground">
              already have an account?{" "}
              <Link
                to="/login"
                className="text-foreground underline underline-offset-4 hover:no-underline"
              >
                log in
              </Link>
            </p>
          </CardFooter>
        </Card>
      </div>
    </div>
  );
}
