import { invoke } from "@tauri-apps/api/core";

export type NetworkAccessState = "requested" | "blocked" | "attempted" | "redirected" | "completed" | "failed" | "cancelled";
export type NetworkAccess = {
  id: number;
  runId: number | null;
  actionId: number | null;
  destination: string;
  state: NetworkAccessState;
  httpStatus: number | null;
  createdAt: number;
};
export type NetworkLogPage = { entries: NetworkAccess[]; hasMore: boolean; nextBefore: number | null };
export const accessApi = {
  readPreference: () => invoke<{ allowed: boolean }>("access_read_preference"),
  setReadPreference: (allowed: boolean) => invoke<{ allowed: boolean }>("access_set_read_preference", { allowed }),
  networkLog: (conversationId: number, beforeId: number | null = null) => invoke<NetworkLogPage>("access_network_log", { conversationId, beforeId }),
};

export function safeNetworkDestination(destination: string): string | null {
  try {
    const url = new URL(destination);
    if (!["http:", "https:"].includes(url.protocol) || !url.hostname) return null;
    return `${url.protocol}//${url.host}/[redacted]`;
  } catch { return null; }
}

export function mergeNetworkEntries(current: NetworkAccess[], incoming: NetworkAccess[]): NetworkAccess[] {
  const entries = new Map(current.map((entry) => [entry.id, entry]));
  incoming.forEach((entry) => entries.set(entry.id, entry));
  return [...entries.values()].sort((first, second) => second.id - first.id);
}
