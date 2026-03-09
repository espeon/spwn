import { useRef, useState, type ChangeEvent } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  getMe,
  listVms,
  updateProfile,
  uploadAvatar,
  avatarUrl,
  changeUsername,
  updateTheme,
  ApiError,
} from "@/api";
import themes from "@/themes.json";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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

function QuotaBar({
  used,
  limit,
  label,
}: {
  used: number;
  limit: number;
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
          {used} / {limit}
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

export function AccountPage() {
  const qc = useQueryClient();
  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const { data: vms } = useQuery({ queryKey: ["vms"], queryFn: listVms });

  const [profileError, setProfileError] = useState<string | null>(null);
  const [avatarError, setAvatarError] = useState<string | null>(null);
  const [themeError, setThemeError] = useState<string | null>(null);

  const [displayNameDialogOpen, setDisplayNameDialogOpen] = useState(false);
  const [displayName, setDisplayName] = useState("");
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [usernameDialogOpen, setUsernameDialogOpen] = useState(false);
  const [newUsername, setNewUsername] = useState("");
  const [usernameConfirm, setUsernameConfirm] = useState("");
  const [usernameError, setUsernameError] = useState<string | null>(null);

  const activeVms =
    vms?.filter((v) => v.status === "running" || v.status === "starting") ?? [];
  const usedVcores = activeVms.reduce((s, v) => s + v.vcores, 0);
  const usedMem = activeVms.reduce((s, v) => s + v.memory_mb, 0);

  const profileMutation = useMutation({
    mutationFn: () => updateProfile({ display_name: displayName || null }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setDisplayNameDialogOpen(false);
      setProfileError(null);
    },
    onError: () => setProfileError("failed to update profile"),
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
    onError: (err) => {
      if (err instanceof ApiError) {
        if (err.status === 409) setUsernameError("username already taken");
        else setUsernameError(err.message);
      } else {
        setUsernameError("something went wrong");
      }
    },
  });

  const themeMutation = useMutation({
    mutationFn: (themeId: string) => updateTheme(themeId),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setThemeError(null);
    },
    onError: () => setThemeError("failed to update theme"),
  });

  const avatarMutation = useMutation({
    mutationFn: (file: File) => uploadAvatar(file),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
      setAvatarError(null);
    },
    onError: (err) => {
      if (err instanceof ApiError) setAvatarError(err.message);
      else setAvatarError("failed to upload avatar");
    },
  });

  function openDisplayNameDialog() {
    setDisplayName(me?.display_name ?? "");
    setProfileError(null);
    setDisplayNameDialogOpen(true);
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
    setAvatarError(null);
    avatarMutation.mutate(file);
    e.target.value = "";
  }

  const usernameConfirmValid =
    usernameConfirm === newUsername && newUsername.length >= 3;

  return (
    <>
      <h1 className="text-base font-semibold mb-3">account</h1>

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
            {profileError && (
              <p className="text-sm text-destructive">{profileError}</p>
            )}
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
              onClick={() => profileMutation.mutate()}
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
              onClick={() => usernameMutation.mutate()}
              disabled={!usernameConfirmValid || usernameMutation.isPending}
            >
              {usernameMutation.isPending ? "changing..." : "change username"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="space-y-4">
        <Card>
          <CardContent className="p-0">
            {/* view */}
            <div className="flex items-center gap-4 p-4">
              <div className="relative group shrink-0">
                <div className="w-12 h-12 rounded-full overflow-hidden bg-secondary flex items-center justify-center">
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
                <p className="text-sm font-medium leading-tight">
                  {me?.display_name || me?.username}
                </p>
                <p className="text-xs text-muted-foreground font-mono">
                  @{me?.username}
                </p>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {me?.email}
                </p>
              </div>
            </div>

            <Separator />

            {/* edit */}
            <div className="p-4 space-y-2">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-xs font-medium">display name</p>
                  <p className="text-xs text-muted-foreground">
                    {me?.display_name || (
                      <span className="italic">not set</span>
                    )}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs px-2"
                  onClick={openDisplayNameDialog}
                >
                  change
                </Button>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <p className="text-xs font-medium">username</p>
                  <p className="text-xs text-muted-foreground font-mono">
                    @{me?.username}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs px-2"
                  onClick={openUsernameDialog}
                >
                  change
                </Button>
              </div>

              {avatarError && (
                <p className="text-xs text-destructive">{avatarError}</p>
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">quota</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <QuotaBar
              used={activeVms.length}
              limit={me?.vm_limit ?? 0}
              label="vms"
            />
            <QuotaBar
              used={usedVcores}
              limit={me?.vcpu_limit ?? 0}
              label="vcores"
            />
            <QuotaBar
              used={usedMem}
              limit={me?.mem_limit_mb ?? 0}
              label="memory (mb)"
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium">theme</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            {Array.from(new Set(themes.map((t) => t.category))).map(
              (category) => (
                <div key={category}>
                  <p className="text-sm text-muted-foreground lowercase font-medium mb-2">
                    {category}
                  </p>
                  <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                    {themes
                      .filter((t) => t.category === category)
                      .map((t) => (
                        <button
                          key={t.id}
                          onClick={() => themeMutation.mutate(t.id)}
                          disabled={themeMutation.isPending}
                          style={{
                            backgroundColor: t.preview[0],
                            color: t.preview[4],
                            borderColor:
                              me?.theme === t.id
                                ? t.preview[2]
                                : t.preview[3] + "44",
                          }}
                          className="text-left rounded-md border-2 px-3 py-2 text-xs transition-opacity hover:opacity-90"
                        >
                          <span
                            className="block font-medium"
                            style={{ color: t.baseText }}
                          >
                            {t.name}
                          </span>
                          <div className="flex justify-end gap-1 mt-1.5">
                            {t.preview.slice(1).map((color) => (
                              <span
                                key={color}
                                className="block w-3 h-3 rounded-full"
                                style={{ backgroundColor: color }}
                              />
                            ))}
                          </div>
                        </button>
                      ))}
                  </div>
                </div>
              ),
            )}
            {themeError && (
              <p className="text-xs text-destructive">{themeError}</p>
            )}
          </CardContent>
        </Card>
      </div>
    </>
  );
}
