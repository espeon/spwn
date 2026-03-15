const KEY = "spwn:recent-vms";
const MAX = 5;

export interface RecentVm {
  id: string;
  name: string;
  status: string;
  subdomain: string | null;
}

export function getRecentVms(): RecentVm[] {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "[]");
  } catch {
    return [];
  }
}

export function addRecentVm(vm: RecentVm): void {
  const prev = getRecentVms();
  const next = [vm, ...prev.filter((r) => r.id !== vm.id)].slice(0, MAX);
  localStorage.setItem(KEY, JSON.stringify(next));
}
