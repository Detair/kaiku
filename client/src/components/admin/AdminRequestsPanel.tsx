/**
 * Open account requests of every account (C27, elevated), each with Cancel.
 * Refetched on every `account_request_updated` hint. A cancel that lost
 * against completion says so and opens the account, where the new device
 * can be removed at once.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { t } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import { listOpenAccountRequests } from "@/lib/tauri/adminSecurity";
import { cancelAccountRequest } from "@/lib/tauri/security";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { withStepUp } from "@/lib/stepUp";
import type { AccountRequestSummary } from "@/lib/types";
import { accountRequestHints } from "@/stores/security";
import { loadUserDetails, selectUser } from "@/stores/admin";
import { securityErrorText } from "@/components/settings/security/securityErrors";

const AdminRequestsPanel: Component = () => {
  // The source starts at 1: a falsy source would never fetch.
  const [requests, { mutate, refetch }] = createResource(
    () => accountRequestHints() + 1,
    async () => {
      try {
        return await listOpenAccountRequests();
      } catch {
        return [] as AccountRequestSummary[];
      }
    },
  );
  const [error, setError] = createSignal<string | null>(null);
  const [tooLate, setTooLate] = createSignal<AccountRequestSummary | null>(null);
  const [busy, setBusy] = createSignal(false);

  const cancel = async (r: AccountRequestSummary) => {
    setBusy(true);
    setError(null);
    setTooLate(null);
    try {
      await withStepUp({ action: "request.cancel", target: r.id, targetLabel: r.subject.display_name }, () =>
        cancelAccountRequest(r.id, null, null),
      );
      mutate((list) => (list ?? []).filter((x) => x.id !== r.id));
    } catch (err) {
      if (err instanceof HttpError && codeIs(err.code, "ACCOUNT_REQUEST_COMPLETED")) {
        setTooLate(r);
        void refetch();
      } else {
        setError(securityErrorText(err, "admin.userSecurity.failed"));
      }
    } finally {
      setBusy(false);
    }
  };

  const openUser = (r: AccountRequestSummary) => {
    selectUser(r.subject.user_id);
    void loadUserDetails(r.subject.user_id);
  };

  return (
    <Show when={(requests() ?? []).length > 0 || tooLate()}>
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
        <Show when={tooLate()}>
          {(r) => (
            <div class="flex flex-wrap items-center gap-2">
              <p role="alert" class="flex-1 text-sm text-text-primary">{t("admin.userSecurity.tooLate")}</p>
              <button
                type="button"
                class="min-h-11 px-3 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-sm text-text-primary"
                onClick={() => openUser(r())}
              >
                {t("admin.requests.openUser", { name: r().subject.display_name })}
              </button>
            </div>
          )}
        </Show>
      </section>
    </Show>
  );
};

export default AdminRequestsPanel;
