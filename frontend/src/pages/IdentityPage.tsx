import { useRef, useState, type ChangeEvent } from "react";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  getMe,
  listVms,
  updateProfile,
  uploadAvatar,
  avatarUrl,
  changeUsername,
  ApiError,
} from "@/api";
import { Separator } from "@/components/ui/separator";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { formatDataSize } from "@/lib/utils";

function QuotaBar({
  used,
  usedLabel,
  limit,
  limitLabel,
  label,
}: {
  used: number;
  usedLabel?: string;
  limit: number;
  limitLabel?: string;
  label: string;
}) {
  const pct = Math.min((used / limit) * 100, 100);
  const color =
    pct >= 90 ? "bg-destructive" : pct >= 70 ? "bg-yellow-500" : "bg-primary";
  return (
    <div>
      <div className="flex justify-between text-xs text-muted-foreground mb-1.5">
        <span>{label}</span>
        <span>
          {usedLabel ?? used} / {limitLabel ?? limit}
        </span>
      </div>
      <div className="h-1.5 bg-secondary rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${color}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

export function IdentityPage() {
  const qc = useQueryClient();
  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const { data: vms } = useQuery({ queryKey: ["vms"], queryFn: () => listVms() });

  const fileInputRef = useRef<HTMLInputElement>(null);

  const [displayNameDialogOpen, setDisplayNameDialogOpen] = useState(false);
  const [displayName, setDisplayName] = useState("");

  const [usernameDialogOpen, setUsernameDialogOpen] = useState(false);
  const [newUsername, setNewUsername] = useState("");
  const [usernameConfirm, setUsernameConfirm] = useState("");
  const [usernameError, setUsernameError] = useState<string | null>(null);

  const [dotfilesDialogOpen, setDotfilesDialogOpen] = useState(false);
  const [dotfilesRepo, setDotfilesRepo] = useState("");

  const activeVms =
    vms?.filter((v) => v.status === "running" || v.status === "starting") ?? [];
  const usedVcpus = activeVms.reduce((s, v) => s + v.vcpus, 0);
  const usedMem = activeVms.reduce((s, v) => s + v.memory_mb, 0);

  const profileMutation = useMutation({
    mutationFn: () => updateProfile({ display_name: displayName || null }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setDisplayNameDialogOpen(false);
    },
  });

  const dotfilesMutation = useMutation({
    mutationFn: () =>
      updateProfile({
        display_name: me?.display_name ?? null,
        dotfiles_repo: dotfilesRepo || null,
      }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setDotfilesDialogOpen(false);
    },
  });

  const usernameMutation = useMutation({
    mutationFn: () => changeUsername(newUsername),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setUsernameDialogOpen(false);
      setNewUsername("");
      setUsernameConfirm("");
      setUsernameError(null);
    },
  });

  const avatarMutation = useMutation({
    mutationFn: (file: File) => uploadAvatar(file),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
    },
  });

  function openDisplayNameDialog() {
    setDisplayName(me?.display_name ?? "");
    setDisplayNameDialogOpen(true);
  }

  function openDotfilesDialog() {
    setDotfilesRepo(me?.dotfiles_repo ?? "");
    setDotfilesDialogOpen(true);
  }

  function openUsernameDialog() {
    setNewUsername("");
    setUsernameConfirm("");
    setUsernameError(null);
    setUsernameDialogOpen(true);
  }

  function onAvatarChange(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    toast.promise(avatarMutation.mutateAsync(file), {
      loading: "uploading avatar...",
      success: "avatar updated",
      error: (err) =>
        err instanceof ApiError ? err.message : "failed to upload avatar",
    });
    e.target.value = "";
  }

  const usernameConfirmValid =
    usernameConfirm === newUsername && newUsername.length >= 3;

  return (
    <>
      <Dialog
        open={displayNameDialogOpen}
        onOpenChange={setDisplayNameDialogOpen}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>change display name</DialogTitle>
            <DialogDescription>
              this is shown instead of your username where supported.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="display-name-input">display name</Label>
              <Input
                id="display-name-input"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder={me?.username}
                autoFocus
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setDisplayNameDialogOpen(false)}
              disabled={profileMutation.isPending}
            >
              cancel
            </Button>
            <Button
              onClick={() =>
                toast.promise(profileMutation.mutateAsync(), {
                  loading: "saving...",
                  success: "display name updated",
                  error: "failed to update display name",
                })
              }
              disabled={profileMutation.isPending}
            >
              {profileMutation.isPending ? "saving..." : "save"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={usernameDialogOpen} onOpenChange={setUsernameDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>change username</DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-2 text-sm text-muted-foreground">
                <p>
                  this will rewrite all of your VM subdomains. any services
                  pointed at{" "}
                  <span className="font-mono text-foreground">
                    *.{me?.username}.spwn.pub
                  </span>{" "}
                  will break immediately.
                </p>
                <p className="font-medium text-destructive">
                  there is no grace period. old subdomains stop working the
                  moment you confirm.
                </p>
              </div>
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="new-username">new username</Label>
              <Input
                id="new-username"
                value={newUsername}
                onChange={(e) => setNewUsername(e.target.value.toLowerCase())}
                placeholder="letters, numbers, hyphens"
                autoComplete="off"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="username-confirm">
                type your new username again to confirm
              </Label>
              <Input
                id="username-confirm"
                value={usernameConfirm}
                onChange={(e) =>
                  setUsernameConfirm(e.target.value.toLowerCase())
                }
                placeholder={newUsername || "confirm new username"}
                autoComplete="off"
              />
            </div>
            {usernameError && (
              <p className="text-sm text-destructive">{usernameError}</p>
            )}
          </div>

          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setUsernameDialogOpen(false)}
              disabled={usernameMutation.isPending}
            >
              cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                const toastId = toast.loading("changing username...");
                usernameMutation
                  .mutateAsync()
                  .then(() =>
                    toast.success("username changed", { id: toastId }),
                  )
                  .catch((err: unknown) => {
                    toast.dismiss(toastId);
                    if (err instanceof ApiError) {
                      if (err.status === 409)
                        setUsernameError("username already taken");
                      else setUsernameError(err.message);
                    } else {
                      setUsernameError("something went wrong");
                    }
                  });
              }}
              disabled={!usernameConfirmValid || usernameMutation.isPending}
            >
              {usernameMutation.isPending ? "changing..." : "change username"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={dotfilesDialogOpen} onOpenChange={setDotfilesDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>dotfiles repository</DialogTitle>
            <DialogDescription>
              git URL cloned to /root/.dotfiles on first VM boot. if an
              install.sh exists it will be run automatically.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="dotfiles-repo-input">repository URL</Label>
              <Input
                id="dotfiles-repo-input"
                value={dotfilesRepo}
                onChange={(e) => setDotfilesRepo(e.target.value)}
                placeholder="https://github.com/you/dotfiles"
                autoFocus
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setDotfilesDialogOpen(false)}
              disabled={dotfilesMutation.isPending}
            >
              cancel
            </Button>
            <Button
              onClick={() =>
                toast.promise(dotfilesMutation.mutateAsync(), {
                  loading: "saving...",
                  success: "dotfiles repo updated",
                  error: "failed to update dotfiles repo",
                })
              }
              disabled={dotfilesMutation.isPending}
            >
              {dotfilesMutation.isPending ? "saving..." : "save"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="space-y-5 mt-0.5">
        <h2 className="text-2xl font-semibold">identity</h2>
        <div className="rounded-lg border bg-card">
          <div className="flex items-center gap-4 p-4">
            <div className="relative group shrink-0">
              <div className="w-18 h-18 rounded-full overflow-hidden bg-secondary flex items-center justify-center">
                {me?.has_avatar ? (
                  <img
                    src={avatarUrl(me.id)}
                    alt="avatar"
                    className="w-full h-full object-cover"
                  />
                ) : (
                  <span className="text-lg text-muted-foreground font-mono select-none">
                    {me?.username?.[0]?.toUpperCase() ?? "?"}
                  </span>
                )}
              </div>
              <button
                onClick={() => fileInputRef.current?.click()}
                disabled={avatarMutation.isPending}
                className="absolute inset-0 rounded-full bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center text-white text-[10px] cursor-pointer"
              >
                {avatarMutation.isPending ? "..." : "change"}
              </button>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/png,image/jpeg,image/webp"
                className="hidden"
                onChange={onAvatarChange}
              />
            </div>

            <div className="min-w-0">
              <p className="text-base font-medium leading-tight">
                {me?.display_name || me?.username}
              </p>
              <p className="text-sm text-muted-foreground font-mono">
                @{me?.username}
              </p>
              <p className="text-sm text-muted-foreground mt-0.5">
                {me?.email}
              </p>
            </div>
          </div>

          <Separator />

          <div className="divide-y divide-border">
            <div className="flex items-center justify-between px-4 py-3">
              <div>
                <p className="text-sm font-medium">display name</p>
                <p className="text text-muted-foreground">
                  {me?.display_name || <span className="italic">not set</span>}
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-sm px-2"
                onClick={openDisplayNameDialog}
              >
                change
              </Button>
            </div>

            <div className="flex items-center justify-between px-4 py-3">
              <div>
                <p className="text-sm font-medium">username</p>
                <p className="text text-muted-foreground font-mono">
                  @{me?.username}
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-sm px-2"
                onClick={openUsernameDialog}
              >
                change
              </Button>
            </div>

            <div className="flex items-center justify-between px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">dotfiles repo</p>
                <p className="text text-muted-foreground font-mono truncate">
                  {me?.dotfiles_repo || (
                    <span className="italic">not set</span>
                  )}
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-sm px-2 ml-2 shrink-0"
                onClick={openDotfilesDialog}
              >
                change
              </Button>
            </div>
          </div>
        </div>
        <div>
          <p className="text-lg font-medium mb-4">quota</p>
          <div className="rounded-lg border bg-card p-4 space-y-4">
            <QuotaBar
              used={activeVms.length}
              limit={me?.vm_limit ?? 0}
              label="vms"
            />
            <QuotaBar
              used={usedVcpus / 1000}
              limit={(me?.vcpu_limit ?? 0) / 1000}
              limitLabel={`${(me?.vcpu_limit ?? 0) / 1000} vCPU${(me?.vcpu_limit ?? 0) / 1000 !== 1 ? "s" : ""}`}
              label="vcpus"
            />
            <QuotaBar
              used={usedMem}
              usedLabel={formatDataSize(usedMem * 1024 * 1024)}
              limit={me?.mem_limit_mb ?? 0}
              limitLabel={formatDataSize((me?.mem_limit_mb ?? 0) * 1024 * 1024)}
              label="memory"
            />
          </div>
        </div>
      </div>
    </>
  );
}
