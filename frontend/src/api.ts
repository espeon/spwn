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
  vcpu_limit: number;
  mem_limit_mb: number;
  vm_limit: number;
}

export interface UpdateProfileRequest {
  display_name: string | null;
}

export interface Vm {
  id: string;
  name: string;
  status: VmStatus;
  subdomain: string;
  vcores: number;
  memory_mb: number;
  ip_address: string;
  exposed_port: number;
  created_at: number;
}

export interface CreateVmRequest {
  name: string;
  vcores: number;
  memory_mb: number;
  exposed_port: number;
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
  const resp = await fetch(path, {
    headers: { "Content-Type": "application/json", ...options.headers },
    ...options,
  });
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

export function listVms(): Promise<Vm[]> {
  return request("/api/vms");
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

export function snapshotVm(id: string): Promise<void> {
  return request(`/api/vms/${id}/snapshot`, { method: "POST" });
}

export { ApiError };
