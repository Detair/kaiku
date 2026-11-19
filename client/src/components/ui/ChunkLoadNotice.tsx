/**
 * What the app shows when part of it could not load (a lazy chunk, an
 * on-demand text area): a sentence and a reload, never a silent gap
 * (Fable r2 R1/R6, Codex r3). A reload also picks up a new deploy's chunks.
 */

import { Component, JSX, ParentProps, Show, onMount } from "solid-js";
import { t } from "@/i18n";

function reloadPage(): void {
  window.location.reload();
}

const Notice: Component<{ class: string; onReload?: () => void }> = (props) => (
  <div
    role="alert"
    class={`flex flex-wrap items-center gap-3 rounded-xl border border-status-warning/60 bg-surface-layer2 p-3 shadow-lg ${props.class}`}
  >
    <p class="flex-1 min-w-48 text-base text-text-primary">{t("common.loadFailed")}</p>
    <button
      type="button"
      class="btn-primary min-h-11 px-4 text-base"
      onClick={() => (props.onReload ?? reloadPage)()}
    >
      {t("common.reload")}
    </button>
  </div>
);

/** Inline notice for a lazy part of the page whose chunk failed. */
export const ChunkLoadNotice: Component<{
  position: "top" | "bottom";
  onReload?: () => void;
  /** Called once when the notice shows (the part is known to be missing). */
  onShown?: () => void;
}> = (props) => {
  onMount(() => props.onShown?.());
  const place = () => (props.position === "top" ? "top-2" : "bottom-4");
  return (
    <Notice
      class={`fixed left-1/2 -translate-x-1/2 z-[90] w-[min(36rem,calc(100vw-1rem))] ${place()}`}
      onReload={props.onReload}
    />
  );
};

/** Startup: the app renders only with all its texts, else a page that says so. */
export const StartupGate: Component<ParentProps<{ ready: boolean; onReload?: () => void }>> = (props) => {
  const failed = (): JSX.Element => (
    <div class="h-dvh flex items-center justify-center p-4 bg-background-tertiary">
      <Notice class="w-[min(36rem,100%)]" onReload={props.onReload} />
    </div>
  );
  return (
    <Show when={props.ready} fallback={failed()}>
      {props.children}
    </Show>
  );
};
