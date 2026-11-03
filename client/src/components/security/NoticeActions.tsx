/**
 * NoticeActions — the usable actions of one security event (C28 computes
 * them per reader; only those are shown). A B.6.6 warning is shown and
 * confirmed first; part 2 asks for a fresh factor; "Too late" offers the next
 * step (remove the device, "Remove device now" for admins, or ask an admin).
 */

import { Component, For, Match, Show, Switch, createSignal } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import type { NoticeAction, SecurityEvent, SecurityNotice } from "@/lib/types";
import type { Part2Factor } from "@/lib/tauri/security";
import { currentUser } from "@/stores/auth";
import { noticeActionError, removeDevice, runNoticeAction, type NoticeResult } from "./runNoticeAction";
import Part2FactorDialog from "./Part2FactorDialog";

const LABEL: Record<Exclude<NoticeAction["action"], "unknown">, MessageKey> = {
  repudiate: "security.action.repudiate",
  cancel_request: "security.action.cancelRequest",
  remove_device: "security.action.removeDevice",
  confirm_contact: "security.action.willHelp",
};

const WARNING: Record<"voids_own_request" | "voids_own_bootstrap", MessageKey> = {
  voids_own_request: "security.action.warning.voidsOwnRequest",
  voids_own_bootstrap: "security.action.warning.voidsOwnBootstrap",
};

export interface NoticeActionsProps {
  event: SecurityEvent;
  notice: SecurityNotice;
  meId: string;
}

const button =
  "min-h-12 px-4 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-base text-text-primary state-disabled";

const NoticeActions: Component<NoticeActionsProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<NoticeResult | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [warning, setWarning] = createSignal<NoticeAction | null>(null);
  const [part2For, setPart2For] = createSignal<NoticeAction | null>(null);

  const usable = () => props.event.actions.filter((a) => a.action !== "unknown");
  const isAdmin = () => (currentUser() as { is_admin?: boolean } | null)?.is_admin === true;
  const own = () => props.notice.subject.user_id === props.meId;

  const label = (a: NoticeAction) => {
    if (a.action === "confirm_contact") return t("security.action.willHelp", { name: props.notice.subject.display_name });
    if (a.action === "remove_device" && !own()) return t("security.action.removeDeviceNow");
    return t(LABEL[a.action as keyof typeof LABEL]);
  };

  const run = async (a: NoticeAction, part2: Part2Factor | null = null) => {
    setBusy(true);
    setError(null);
    try {
      const r = await runNoticeAction(a, props.event, props.notice, props.meId, { part2, isAdmin: isAdmin() });
      if (r.kind !== "cancelled") setResult(r);
    } catch (err) {
      setError(noticeActionError(err));
    } finally {
      setBusy(false);
    }
  };

  const start = (a: NoticeAction) => {
    if (a.warning) {
      setWarning(a);
      return;
    }
    proceed(a);
  };
  const proceed = (a: NoticeAction) => {
    setWarning(null);
    if (a.via === "part2") setPart2For(a);
    else void run(a);
  };

  const followUp = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await removeDevice(props.event, props.notice, props.meId);
      if (r.kind !== "cancelled") setResult(r);
    } catch (err) {
      setError(noticeActionError(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={usable().length > 0}>
      <div class="space-y-2">
        <Switch>
          <Match when={result()?.kind === "done" && result()}>
            {(r) => <p class="text-base text-text-primary" role="status">{(r() as { text: string }).text}</p>}
          </Match>
          <Match when={result()?.kind === "tooLate" && (result() as Extract<NoticeResult, { kind: "tooLate" }>)}>
            {(r) => (
              <div class="space-y-2" role="status">
                <p class="text-base text-text-primary">{r().text}</p>
                <Switch>
                  <Match when={r().followUp === "removeDevice"}>
                    <button type="button" class={button} disabled={busy()} onClick={() => void followUp()}>
                      {t("security.action.removeDevice")}
                    </button>
                  </Match>
                  <Match when={r().followUp === "removeDeviceNow"}>
                    <button type="button" class={button} disabled={busy()} onClick={() => void followUp()}>
                      {t("security.action.removeDeviceNow")}
                    </button>
                  </Match>
                  <Match when={r().followUp === "askAdmin"}>
                    <p class="text-base text-text-secondary">{t("security.action.askAdmin")}</p>
                  </Match>
                </Switch>
              </div>
            )}
          </Match>
          <Match when={warning()}>
            {(a) => (
              <div class="space-y-2 p-3 rounded-lg border border-status-warning/50" role="alert">
                <p class="text-base text-text-primary">{t(WARNING[a().warning!])}</p>
                <div class="flex flex-wrap gap-2">
                  <button type="button" class={button} onClick={() => setWarning(null)}>
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    class="btn-primary min-h-12 text-base"
                    onClick={() => {
                      const action = a();
                      proceed(action);
                    }}
                  >
                    {t("security.action.continue")}
                  </button>
                </div>
              </div>
            )}
          </Match>
          <Match when={true}>
            <div class="flex flex-wrap gap-2">
              <For each={usable()}>
                {(a) => (
                  <button type="button" class={button} disabled={busy()} onClick={() => start(a)}>
                    {label(a)}
                  </button>
                )}
              </For>
            </div>
          </Match>
        </Switch>
        <Show when={error()}>
          <p role="alert" class="text-base text-text-primary">{error()}</p>
        </Show>
        <Show when={part2For()}>
          {(a) => (
            <Part2FactorDialog
              onClose={() => setPart2For(null)}
              onSubmit={(factor) => {
                const action = a();
                setPart2For(null);
                void run(action, factor);
              }}
            />
          )}
        </Show>
      </div>
    </Show>
  );
};

export default NoticeActions;
