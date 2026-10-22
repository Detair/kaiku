/**
 * ConnectionIndicator - small banner while the realtime connection is down.
 *
 * Hidden while connected; appears only after the connection has been
 * non-connected for 2 s so brief blips stay invisible. Mounted once in
 * AppShell as a fixed overlay (desktop and mobile layouts).
 */

import {
  Component,
  Show,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
  createEffect,
} from "solid-js";
import { t } from "@/i18n";
import {
  canRetryConnectionNow,
  retryConnectionNow,
  wsState,
} from "@/stores/websocket";

const SHOW_AFTER_MS = 2000;

const ConnectionIndicator: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const [online, setOnline] = createSignal(
    typeof navigator === "undefined" ? true : navigator.onLine,
  );
  const connected = createMemo(() => wsState.status === "connected");

  let showTimer: ReturnType<typeof setTimeout> | undefined;
  const clearShowTimer = () => {
    if (showTimer !== undefined) {
      clearTimeout(showTimer);
      showTimer = undefined;
    }
  };

  // Re-run only when connected flips, so reconnecting -> connecting
  // transitions don't restart the 2 s delay.
  createEffect(
    on(connected, (isConnected) => {
      clearShowTimer();
      if (isConnected) {
        setVisible(false);
        return;
      }
      showTimer = setTimeout(() => setVisible(true), SHOW_AFTER_MS);
    }),
  );

  onMount(() => {
    const updateOnline = () => setOnline(navigator.onLine);
    window.addEventListener("online", updateOnline);
    window.addEventListener("offline", updateOnline);
    onCleanup(() => {
      window.removeEventListener("online", updateOnline);
      window.removeEventListener("offline", updateOnline);
    });
  });

  onCleanup(clearShowTimer);

  return (
    // The live region stays mounted so screen readers announce changes.
    <div
      role="status"
      aria-live="polite"
      class="fixed top-2 left-1/2 -translate-x-1/2 z-50 max-w-[calc(100vw-2rem)]"
    >
      <Show when={visible()}>
        <div class="rounded-lg bg-surface-base shadow-lg">
          <div class="flex items-center gap-3 rounded-lg border border-status-warning/50 bg-status-warning/15 px-4 py-2 text-sm text-text-primary">
            <span>
              {online() ? t("connection.reconnecting") : t("connection.offline")}
            </span>
            <Show when={canRetryConnectionNow()}>
              <button
                type="button"
                class="shrink-0 rounded-md px-2 py-0.5 font-medium text-text-primary underline underline-offset-2 hover:bg-status-warning/20 focus-visible:outline focus-visible:outline-2 focus-visible:outline-status-warning"
                onClick={() => retryConnectionNow()}
              >
                {t("connection.retryNow")}
              </button>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ConnectionIndicator;
