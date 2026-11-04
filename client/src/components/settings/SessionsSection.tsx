/**
 * Signed-in sessions (C27): each with its assurance ("Full" / "Chat only"),
 * client, the sign-in method that created it and last use. "End" and "Sign
 * out everywhere else" need a step-up (`sessions.end`/id or `others`). A key
 * device's own session is shown as "Remove device" and removes the phone.
 * A chat-only session sees the list read-only.
 */

import { Component, createSignal, onMount, For, Show } from "solid-js";
import { Monitor, Smartphone, Globe } from "lucide-solid";
import * as tauri from "@/lib/tauri";
import { removeKeyDevice } from "@/lib/tauri/security";
import { formatRelativeTimeShort } from "@/lib/utils";
import type { SessionInfo } from "@/lib/types";
import { t } from "@/i18n";
import { withStepUp } from "@/lib/stepUp";
import { isProbation } from "@/stores/auth";
import { ReadOnlyNote, dangerButton, errorBox, errorStyle, secondaryButton } from "./security/SectionCard";
import { securityErrorText } from "./security/securityErrors";

const SessionsSection: Component = () => {
  const [sessions, setSessions] = createSignal<SessionInfo[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const loadSessions = async () => {
    try {
      setLoading(true);
      setError(null);
      const resp = await tauri.listSessions();
      setSessions(resp.sessions);
    } catch (err) {
      setError(securityErrorText(err, "settings.sessions.loadFailed"));
    } finally {
      setLoading(false);
    }
  };

  onMount(loadSessions);

  const act = async (job: () => Promise<void>, fallback: "settings.sessions.revokeFailed" | "settings.sessions.revokeAllFailed") => {
    setBusy(true);
    setError(null);
    try {
      await job();
    } catch (err) {
      setError(securityErrorText(err, fallback));
    } finally {
      setBusy(false);
    }
  };

  const handleRevoke = (session: SessionInfo) =>
    act(async () => {
      await withStepUp({ action: "sessions.end", target: session.id, targetLabel: session.device }, () =>
        tauri.revokeSession(session.id),
      );
      setSessions((prev) => prev.filter((s) => s.id !== session.id));
    }, "settings.sessions.revokeFailed");

  const handleRemoveDevice = (session: SessionInfo, deviceId: string, label: string) =>
    act(async () => {
      await withStepUp({ action: "key_device.remove", target: deviceId, targetLabel: label }, () =>
        removeKeyDevice(deviceId),
      );
      setSessions((prev) => prev.filter((s) => s.id !== session.id));
    }, "settings.sessions.revokeFailed");

  const handleRevokeAll = () =>
    act(async () => {
      await withStepUp({ action: "sessions.end", target: "others", targetLabel: "" }, () =>
        tauri.revokeAllOtherSessions(),
      );
      setSessions((prev) => prev.filter((s) => s.is_current));
    }, "settings.sessions.revokeAllFailed");

  const icon = (s: SessionInfo) => {
    if (s.client_class === "android") return Smartphone;
    if (s.client_class === "web") return Globe;
    const lower = s.device.toLowerCase();
    if (lower.includes("android") || lower.includes("iphone") || lower.includes("mobile")) return Smartphone;
    return Monitor;
  };

  const keyDevice = (s: SessionInfo) => (s.authenticator?.type === "key_device" ? s.authenticator : null);

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h4 class="text-base font-semibold text-text-primary">{t("settings.sessions.title")}</h4>
        <Show when={!isProbation() && sessions().filter((s) => !s.is_current).length > 0}>
          <button type="button" onClick={() => void handleRevokeAll()} disabled={busy()} class={dangerButton}>
            {t("security.sessions.endAll")}
          </button>
        </Show>
      </div>

      <Show when={loading()}>
        <div class="text-base text-text-secondary py-4 text-center">{t("settings.sessions.loading")}</div>
      </Show>

      <Show when={error()}>
        <div role="alert" class={errorBox} style={errorStyle}>
          {error()}
        </div>
      </Show>

      <Show when={!loading()}>
        <ul class="space-y-2">
          <For each={sessions()}>
            {(session) => {
              const Icon = icon(session);
              return (
                <li class="flex flex-wrap items-center gap-3 p-3 rounded-xl bg-surface-layer2 border border-white/5">
                  <div class="shrink-0 text-text-secondary">
                    <Icon size={20} aria-hidden="true" />
                  </div>
                  <div class="flex-1 min-w-0 space-y-0.5">
                    <div class="flex flex-wrap items-center gap-2">
                      <span class="text-base font-medium text-text-primary">{session.device}</span>
                      <span class="px-2 py-0.5 text-xs rounded-md border border-white/15 text-text-primary">
                        {session.assurance === "full" ? t("security.sessions.full") : t("security.sessions.chatOnly")}
                      </span>
                      <Show when={session.is_current}>
                        <span class="px-2 py-0.5 text-xs rounded-md bg-status-success/20 text-text-primary font-medium">
                          {t("settings.sessions.current")}
                        </span>
                      </Show>
                    </div>
                    <Show when={session.authenticator}>
                      {(a) => <p class="text-sm text-text-secondary">{t("security.sessions.with", { label: a().label })}</p>}
                    </Show>
                    <p class="text-sm text-text-secondary">
                      {t("security.sessions.lastUsed", {
                        time: formatRelativeTimeShort(session.last_used_at ?? session.created_at),
                      })}
                    </p>
                  </div>
                  <Show when={!session.is_current && !isProbation()}>
                    <Show
                      when={keyDevice(session)}
                      fallback={
                        <button
                          type="button"
                          onClick={() => void handleRevoke(session)}
                          disabled={busy()}
                          class={secondaryButton}
                          aria-label={t("security.stepUp.action.sessionsEnd", { target: session.device })}
                        >
                          {t("security.sessions.end")}
                        </button>
                      }
                    >
                      {(device) => (
                        <button
                          type="button"
                          onClick={() => void handleRemoveDevice(session, device().id, device().label)}
                          disabled={busy()}
                          class={dangerButton}
                          aria-label={t("security.keyDevices.removeNamed", { name: device().label })}
                        >
                          {t("security.keyDevices.remove")}
                        </button>
                      )}
                    </Show>
                  </Show>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
      <Show when={isProbation()}>
        <ReadOnlyNote />
      </Show>
    </div>
  );
};

export default SessionsSection;
