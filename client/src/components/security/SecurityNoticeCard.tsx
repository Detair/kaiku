/**
 * SecurityNoticeCard — a notice in the Kaiku DM (C28): what happened, to
 * which account, each event's facts and its usable actions. High severity
 * adds an icon and "Important" (never colour alone). No message actions.
 */

import { Component, For, Show, createSignal } from "solid-js";
import { ShieldAlert, Shield } from "lucide-solid";
import { t } from "@/i18n";
import type { SecurityNotice } from "@/lib/types";
import { noticeText } from "./noticeText";
import SecurityEventDetails from "./SecurityEventDetails";
import NoticeActions from "./NoticeActions";

export interface SecurityNoticeCardProps {
  notice: SecurityNotice;
  meId: string;
}

/** Events shown before "Show all"; every supplied event keeps its actions. */
const SHOWN_EVENTS = 5;

const SecurityNoticeCard: Component<SecurityNoticeCardProps> = (props) => {
  // A coalesced notice keeps every event's actions reachable (Codex review
  // M5): the log lists events, but only here can they be acted on.
  const [all, setAll] = createSignal(false);
  const shown = () => (all() ? props.notice.events : props.notice.events.slice(0, SHOWN_EVENTS));
  const hidden = () => props.notice.events.length - shown().length;
  return (
  <article
    class="mx-4 my-3 p-4 rounded-xl bg-surface-layer2 border space-y-3"
    classList={{
      "border-status-warning/60": props.notice.severity === "high",
      "border-white/10": props.notice.severity !== "high",
    }}
    data-testid="security-notice"
  >
    <div class="flex items-start gap-3">
      <Show
        when={props.notice.severity === "high"}
        fallback={<Shield class="w-5 h-5 mt-0.5 shrink-0 text-text-secondary" aria-hidden="true" />}
      >
        <ShieldAlert class="w-5 h-5 mt-0.5 shrink-0 text-status-warning" aria-hidden="true" />
      </Show>
      <div class="space-y-1">
        <Show when={props.notice.severity === "high"}>
          <p class="text-sm font-semibold uppercase tracking-wide text-text-primary">{t("security.notice.important")}</p>
        </Show>
        <p class="text-base font-medium text-text-primary">{noticeText(props.notice, props.meId)}</p>
      </div>
    </div>
    <For each={shown()}>
      {(ev) => (
        <div class="pl-8 space-y-2">
          <SecurityEventDetails event={ev} />
          <NoticeActions event={ev} notice={props.notice} meId={props.meId} />
        </div>
      )}
    </For>
    <Show when={hidden() > 0}>
      <button
        type="button"
        class="ml-8 min-h-12 px-4 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-base text-text-primary"
        aria-expanded="false"
        onClick={() => setAll(true)}
      >
        {t("security.notice.showAll", { count: props.notice.events.length })}
      </button>
    </Show>
    <Show when={props.notice.events_truncated}>
      <p class="pl-8 text-sm text-text-secondary">{t("security.notice.truncated")}</p>
    </Show>
  </article>
  );
};

export default SecurityNoticeCard;
