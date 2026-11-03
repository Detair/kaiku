/** Open account requests of every account (C27, elevated), each with Cancel. */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { t } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import { listOpenAccountRequests } from "@/lib/tauri/adminSecurity";
import { cancelAccountRequest } from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import type { AccountRequestSummary } from "@/lib/types";
import { securityErrorText } from "@/components/settings/security/securityErrors";

const AdminRequestsPanel: Component = () => {
  const [requests, { mutate }] = createResource(async () => {
    try {
      return await listOpenAccountRequests();
    } catch {
      return [] as AccountRequestSummary[];
    }
  });
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const cancel = async (r: AccountRequestSummary) => {
    setBusy(true);
    setError(null);
    try {
      await withStepUp({ action: "request.cancel", target: r.id, targetLabel: r.subject.display_name }, () =>
        cancelAccountRequest(r.id, null, null),
      );
      mutate((list) => (list ?? []).filter((x) => x.id !== r.id));
    } catch (err) {
      setError(securityErrorText(err, "admin.userSecurity.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={(requests() ?? []).length > 0}>
      <section class="m-3 p-3 rounded-lg border border-status-warning/50 space-y-2" data-testid="admin-open-requests">
        <h4 class="text-sm font-semibold text-text-primary">{t("admin.requests.title")}</h4>
        <ul class="space-y-2">
          <For each={requests()}>
            {(r) => (
              <li class="flex flex-wrap items-center gap-2 text-sm">
                <span class="flex-1 text-text-primary">
                  <strong>{r.subject.display_name}</strong>
                  <span class="text-text-secondary">
                    {" · "}
                    {t("admin.requests.line", {
                      by: r.started_by?.username ?? r.subject.username,
                      time: formatTimestamp(r.ready_at),
                    })}
                  </span>
                </span>
                <button
                  type="button"
                  class="min-h-11 px-3 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-sm text-text-primary state-disabled"
                  disabled={busy()}
                  aria-label={t("admin.requests.cancelNamed", { name: r.subject.display_name })}
                  onClick={() => void cancel(r)}
                >
                  {t("common.cancel")}
                </button>
              </li>
            )}
          </For>
        </ul>
        <Show when={error()}>
          <p role="alert" class="text-sm text-text-primary">{error()}</p>
        </Show>
      </section>
    </Show>
  );
};

export default AdminRequestsPanel;
