/** The security log (C28): every event, newest first, read-only, "Show older". */

import { Component, For, Show, createSignal, onMount } from "solid-js";
import { ScrollText } from "lucide-solid";
import { t } from "@/i18n";
import { listSecurityEvents } from "@/lib/tauri/security";
import type { SecurityEvent } from "@/lib/types";
import { currentUser } from "@/stores/auth";
import { eventText } from "@/components/security/noticeText";
import SecurityEventDetails from "@/components/security/SecurityEventDetails";
import { errorBox, errorStyle, secondaryButton } from "./SectionCard";
import { securityErrorText } from "./securityErrors";

const PAGE = 50;

export interface SecurityLogSectionProps {
  /** Admin log of another account (A13); defaults to the own log. */
  load?: (before: string | undefined, limit: number) => ReturnType<typeof listSecurityEvents>;
  subject?: { user_id: string; display_name: string };
}

const SecurityLogSection: Component<SecurityLogSectionProps> = (props) => {
  const [items, setItems] = createSignal<SecurityEvent[]>([]);
  const [cursor, setCursor] = createSignal<string | null>(null);
  const [more, setMore] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const me = () => currentUser()?.id ?? "";
  const subject = () => props.subject ?? { user_id: me(), display_name: currentUser()?.display_name ?? "" };

  const load = async (before?: string) => {
    setBusy(true);
    setError(null);
    try {
      const page = await (props.load ?? listSecurityEvents)(before, PAGE);
      setItems((prev) => [...prev, ...page.items]);
      setCursor(page.next_cursor);
      setMore(page.has_more && page.next_cursor !== null);
    } catch (err) {
      setError(securityErrorText(err, "security.log.loadFailed"));
    } finally {
      setBusy(false);
    }
  };
  onMount(() => void load());

  return (
    <section class="bg-surface-base rounded-xl p-4 space-y-3" data-testid="security-log">
      <div class="flex items-center gap-3">
        <ScrollText class="w-5 h-5 text-text-secondary" aria-hidden="true" />
        <h4 class="text-base font-semibold text-text-primary">{t("security.log.title")}</h4>
      </div>
      <Show when={items().length > 0} fallback={<Show when={!busy()}><p class="text-base text-text-secondary">{t("security.log.empty")}</p></Show>}>
        <ol class="space-y-2">
          <For each={items()}>
            {(ev) => (
              <li class="p-3 rounded-lg bg-surface-layer2 space-y-1">
                <p class="text-base text-text-primary">{eventText(ev, subject(), me())}</p>
                <SecurityEventDetails event={ev} />
              </li>
            )}
          </For>
        </ol>
      </Show>
      <Show when={error()}>
        <p role="alert" class={errorBox} style={errorStyle}>{error()}</p>
      </Show>
      <Show when={more()}>
        <button type="button" class={secondaryButton} disabled={busy()} onClick={() => void load(cursor() ?? undefined)}>
          {t("security.log.older")}
        </button>
      </Show>
    </section>
  );
};

export default SecurityLogSection;
