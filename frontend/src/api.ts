export interface Image {
  id: string;
  name: string;
  tag: string;
  source: string;
  status: string;
  size_bytes: number;
  created_at: number;
}

export interface AdminImage extends Image {
  error: string | null;
  build_log: string;
}

export interface BuildImageRequest {
  source: string;
  name: string;
  tag?: string;
}

export type VmStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "snapshotting"
  | "error";

export interface Account {
  id: string;
  email: string;
  username: string;
  display_name: string | null;
  has_avatar: boolean;
  theme: string;
  vcpu_limit: number;
  mem_limit_mb: number;
  vm_limit: number;
  role: string;
  dotfiles_repo: string | null;
}

export interface UpdateProfileRequest {
  display_name: string | null;
  dotfiles_repo?: string | null;
}

export interface Vm {
  id: string;
  name: string;
  status: VmStatus;
  subdomain: string;
  image: string;
  vcpus: number;
  memory_mb: number;
  ip_address: string;
  exposed_port: number;
  created_at: number;
  last_started_at: number | null;
  region: string | null;
}

export interface CreateVmRequest {
  name: string;
  image: string;
  vcpus: number;
  memory_mb: number;
  exposed_port: number;
  region?: string;
  namespace_id?: string;
}

class ApiError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string>),
  };
  if (options.body !== undefined) {
    headers["Content-Type"] = "application/json";
  }
  const resp = await fetch(path, { ...options, headers });
  if (!resp.ok) {
    const text = await resp.text().catch(() => resp.statusText);
    throw new ApiError(resp.status, text);
  }
  const contentType = resp.headers.get("content-type");
  if (resp.status === 204 || !contentType?.includes("application/json")) {
    return undefined as unknown as T;
  }
  return resp.json();
}

// ── config ────────────────────────────────────────────────────────────────────

export interface ServerConfig {
  ssh_gateway_addr: string;
  public_url: string;
}

export function getConfig(): Promise<ServerConfig> {
  return request("/api/config");
}

// ── auth ──────────────────────────────────────────────────────────────────────

export function getMe(): Promise<Account> {
  return request("/auth/me");
}

export function signup(
  email: string,
  password: string,
  username: string,
  inviteCode: string,
): Promise<void> {
  return request("/auth/signup", {
    method: "POST",
    body: JSON.stringify({
      email,
      password,
      username,
      invite_code: inviteCode,
    }),
  });
}

export function updateProfile(req: UpdateProfileRequest): Promise<void> {
  return request("/auth/me", {
    method: "PATCH",
    body: JSON.stringify(req),
  });
}

export function updateTheme(theme: string): Promise<void> {
  return request("/auth/me/theme", {
    method: "PATCH",
    body: JSON.stringify({ theme }),
  });
}

export function uploadAvatar(file: File): Promise<void> {
  return request("/auth/me/avatar", {
    method: "POST",
    headers: { "Content-Type": file.type },
    body: file,
  });
}

export function avatarUrl(accountId: string): string {
  return `/auth/avatar/${accountId}`;
}

export function changeUsername(username: string): Promise<void> {
  return request("/api/account/username", {
    method: "POST",
    body: JSON.stringify({ username }),
  });
}

export function login(email: string, password: string): Promise<void> {
  return request("/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });
}

export function logout(): Promise<void> {
  return request("/auth/logout", { method: "POST" });
}

// ── vms ───────────────────────────────────────────────────────────────────────

export function listVms(namespace_id?: string): Promise<Vm[]> {
  const qs = namespace_id ? `?namespace_id=${encodeURIComponent(namespace_id)}` : "";
  return request(`/api/vms${qs}`);
}

export function getVm(id: string): Promise<Vm> {
  return request(`/api/vms/${id}`);
}

export function createVm(req: CreateVmRequest): Promise<Vm> {
  return request("/api/vms", { method: "POST", body: JSON.stringify(req) });
}

export function deleteVm(id: string): Promise<void> {
  return request(`/api/vms/${id}`, { method: "DELETE" });
}

export function startVm(id: string): Promise<void> {
  return request(`/api/vms/${id}/start`, { method: "POST" });
}

export function stopVm(id: string): Promise<void> {
  return request(`/api/vms/${id}/stop`, { method: "POST" });
}

export interface RegionInfo {
  name: string;
  active: boolean;
}

export function listRegions(): Promise<RegionInfo[]> {
  return request("/api/regions");
}

export interface Snapshot {
  id: string;
  vm_id: string;
  label: string | null;
  size_bytes: number;
  created_at: number;
}

export function snapshotVm(id: string): Promise<Snapshot> {
  return request(`/api/vms/${id}/snapshot`, { method: "POST" });
}

export function listSnapshots(vmId: string): Promise<Snapshot[]> {
  return request(`/api/vms/${vmId}/snapshots`);
}

export function deleteSnapshot(vmId: string, snapId: string): Promise<void> {
  return request(`/api/vms/${vmId}/snapshots/${snapId}`, { method: "DELETE" });
}

export function restoreSnapshot(vmId: string, snapId: string): Promise<void> {
  return request(`/api/vms/${vmId}/restore/${snapId}`, { method: "POST" });
}

export function resizeVmResources(
  id: string,
  vcpus?: number,
  memory_mb?: number,
): Promise<Vm> {
  return request(`/api/vms/${id}/resources`, {
    method: "POST",
    body: JSON.stringify({ vcpus, memory_mb }),
  });
}

export function patchVm(id: string, patch: { name?: string; exposed_port?: number }): Promise<Vm> {
  return request(`/api/vms/${id}`, { method: "PATCH", body: JSON.stringify(patch) });
}

export function cloneVm(
  id: string,
  name: string,
  includeMemory: boolean,
): Promise<Vm> {
  return request(`/api/vms/${id}/clone`, {
    method: "POST",
    body: JSON.stringify({ name, include_memory: includeMemory }),
  });
}

export function cliAuthorize(code: string): Promise<void> {
  return request("/auth/cli/authorize", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code }),
  });
}

export function cliDeny(code: string): Promise<void> {
  return request("/auth/cli/deny", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code }),
  });
}

// ── SSH keys ──────────────────────────────────────────────────────────────────

export interface SshKey {
  id: string;
  name: string;
  fingerprint: string;
  created_at: number;
}

export function listSshKeys(): Promise<SshKey[]> {
  return request("/api/account/keys");
}

export function addSshKey(name: string, public_key: string): Promise<SshKey> {
  return request("/api/account/keys", {
    method: "POST",
    body: JSON.stringify({ name, public_key }),
  });
}

export function deleteSshKey(id: string): Promise<void> {
  return request(`/api/account/keys/${id}`, { method: "DELETE" });
}

// ── admin ─────────────────────────────────────────────────────────────────────

export interface Host {
  id: string;
  name: string;
  address: string;
  status: string;
  vcpu_total: number;
  vcpu_used: number;
  mem_total_mb: number;
  mem_used_mb: number;
  labels: Record<string, string>;
  snapshot_addr: string;
  last_seen_at: number;
}

export function listHosts(): Promise<Host[]> {
  return request("/api/admin/hosts");
}

export interface AdminVm {
  id: string;
  name: string;
  status: string;
  host_id: string | null;
  account_id: string;
  username: string;
  vcpus: number;
  memory_mb: number;
  disk_usage_mb: number;
  subdomain: string;
  region: string | null;
}

export function listAdminVms(): Promise<AdminVm[]> {
  return request("/api/admin/vms");
}

export function setHostStatus(id: string, status: string): Promise<void> {
  return request(`/api/admin/hosts/${id}/status`, {
    method: "POST",
    body: JSON.stringify({ status }),
  });
}

export function adminMigrateVm(
  vmId: string,
  targetHostId: string,
): Promise<void> {
  return request(`/api/admin/vms/${vmId}/migrate`, {
    method: "POST",
    body: JSON.stringify({ target_host_id: targetHostId }),
  });
}

// ── vm events ─────────────────────────────────────────────────────────────────

export interface VmEvent {
  id: string;
  vm_id: string;
  event: string;
  metadata: string | null;
  created_at: number;
}

export function listVmEvents(vmId: string, limit = 20): Promise<VmEvent[]> {
  return request(`/api/vms/${vmId}/events?limit=${limit}`);
}

// ── images ────────────────────────────────────────────────────────────────────

export function listImages(): Promise<Image[]> {
  return request("/api/images");
}

export function listAdminImages(): Promise<AdminImage[]> {
  return request("/api/admin/images");
}

export function buildImage(req: BuildImageRequest): Promise<AdminImage> {
  return request("/api/admin/images", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

export function deleteAdminImage(id: string): Promise<void> {
  return request(`/api/admin/images/${id}`, { method: "DELETE" });
}

// ── api tokens ────────────────────────────────────────────────────────────────

export interface ApiToken {
  id: string;
  name: string;
  created_at: number;
  last_used_at: number | null;
}

export interface CreatedApiToken extends ApiToken {
  token: string;
}

export function listTokens(): Promise<ApiToken[]> {
  return request("/api/account/tokens");
}

export function createToken(name: string): Promise<CreatedApiToken> {
  return request("/api/account/tokens", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export function deleteToken(id: string): Promise<void> {
  return request(`/api/account/tokens/${id}`, { method: "DELETE" });
}

// ── namespaces ────────────────────────────────────────────────────────────────

export interface Namespace {
  id: string;
  slug: string;
  kind: "personal" | "org";
  display_name: string | null;
  owner_id: string;
  vcpu_limit: number;
  mem_limit_mb: number;
  vm_limit: number;
  created_at: number;
}

export interface NamespaceMember {
  account_id: string;
  username: string;
  role: string;
  joined_at: number;
}

export function listNamespaces(): Promise<Namespace[]> {
  return request("/api/namespaces");
}

export function createNamespace(slug: string, display_name?: string): Promise<Namespace> {
  return request("/api/namespaces", {
    method: "POST",
    body: JSON.stringify({ slug, display_name }),
  });
}

export function getNamespace(id: string): Promise<Namespace> {
  return request(`/api/namespaces/${id}`);
}

export function updateNamespace(id: string, display_name: string): Promise<Namespace> {
  return request(`/api/namespaces/${id}`, {
    method: "PATCH",
    body: JSON.stringify({ display_name }),
  });
}

export function listNamespaceMembers(id: string): Promise<NamespaceMember[]> {
  return request(`/api/namespaces/${id}/members`);
}

export function addNamespaceMember(id: string, username: string, role: string): Promise<void> {
  return request(`/api/namespaces/${id}/members`, {
    method: "POST",
    body: JSON.stringify({ username, role }),
  });
}

export function removeNamespaceMember(id: string, account_id: string): Promise<void> {
  return request(`/api/namespaces/${id}/members/${account_id}`, { method: "DELETE" });
}

export interface NamespaceUsage {
  used_vcpus: number;
  used_mem_mb: number;
  used_vms: number;
}

export function getNamespaceUsage(id: string): Promise<NamespaceUsage> {
  return request(`/api/namespaces/${id}/usage`);
}

export { ApiError };
