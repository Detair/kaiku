import { Component, createSignal, onMount, For, Show } from "solid-js";
import { Monitor, Smartphone, LogOut } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import type { SessionInfo } from "@/lib/types";

const SessionsSection: Component = () => {
  const [sessions, setSessions] = createSignal<SessionInfo[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  const loadSessions = async () => {
    try {
      setLoading(true);
      setError(null);
      const resp = await tauri.listSessions();
      setSessions(resp.sessions);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  onMount(loadSessions);

  const handleRevoke = async (sessionId: string) => {
    try {
      await tauri.revokeSession(sessionId);
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleRevokeAll = async () => {
    try {
      await tauri.revokeAllOtherSessions();
      setSessions((prev) => prev.filter((s) => s.is_current));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const formatLocation = (session: SessionInfo): string => {
    if (session.city && session.country)
      return `${session.city}, ${session.country}`;
    if (session.country) return session.country;
    return "Unknown location";
  };

  const formatRelativeTime = (dateStr: string): string => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
  };

  const getDeviceIcon = (device: string) => {
    const lower = device.toLowerCase();
    if (
      lower.includes("mobile") ||
      lower.includes("android") ||
      lower.includes("iphone")
    ) {
      return Smartphone;
    }
    return Monitor;
  };

  return (
    <div class="space-y-3">
      <div class="flex items-center justify-between">
        <h4 class="text-sm font-semibold text-text-secondary uppercase tracking-wide">
          Active Sessions
        </h4>
        <Show when={sessions().filter((s) => !s.is_current).length > 0}>
          <button
            onClick={handleRevokeAll}
            class="text-xs px-2 py-1 rounded-lg text-status-danger hover:bg-status-danger/10 transition-colors"
          >
            Log out all other devices
          </button>
        </Show>
      </div>

      <Show when={loading()}>
        <div class="text-sm text-text-secondary py-4 text-center">
          Loading sessions...
        </div>
      </Show>

      <Show when={error()}>
        <div class="text-sm text-status-danger p-3 rounded-xl bg-status-danger/10 border border-status-danger/20">
          {error()}
        </div>
      </Show>

      <Show when={!loading() && !error()}>
        <div class="space-y-2">
          <For each={sessions()}>
            {(session) => {
              const Icon = getDeviceIcon(session.device);
              return (
                <div class="flex items-center gap-3 p-3 rounded-xl bg-surface-layer2 border border-white/5">
                  <div class="shrink-0 text-text-secondary">
                    <Icon size={20} />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-text-primary truncate">
                        {session.device}
                      </span>
                      <Show when={session.is_current}>
                        <span class="px-1.5 py-0.5 text-[10px] rounded-md bg-status-success/20 text-text-primary font-medium">
                          Current
                        </span>
                      </Show>
                    </div>
                    <div class="flex items-center gap-2 text-xs text-text-secondary mt-0.5">
                      <span>{formatLocation(session)}</span>
                      <span class="opacity-40">&middot;</span>
                      <span>{session.ip_address ?? "Unknown IP"}</span>
                      <span class="opacity-40">&middot;</span>
                      <span>{formatRelativeTime(session.created_at)}</span>
                    </div>
                  </div>
                  <Show when={!session.is_current}>
                    <button
                      onClick={() => handleRevoke(session.id)}
                      class="shrink-0 p-1.5 rounded-lg text-text-secondary hover:text-status-danger hover:bg-status-danger/10 transition-colors"
                      title="Revoke session"
                    >
                      <LogOut size={16} />
                    </button>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default SessionsSection;
