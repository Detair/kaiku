/**
 * What the app shows when part of it could not load (a lazy chunk, an
 * on-demand text area): a sentence and a reload, never a silent gap
 * (Fable r2 R1/R6, Codex r3). A reload also picks up a new deploy's chunks.
 * A window that loaded but failed while rendering gets its own sentence and
 * a retry instead (`WindowErrorNotice`): the connection is not the problem.
 */

import { Component, JSX, ParentProps, Show, onMount } from "solid-js";
import { X } from "lucide-solid";
import { t, type MessageKey } from "@/i18n";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";

function reloadPage(): void {
  window.location.reload();
}

const Notice: Component<{
  class: string;
  style?: JSX.CSSProperties;
  text?: MessageKey;
  action?: MessageKey;
  onAction?: () => void;
  onDismiss?: () => void;
}> = (props) => (
  <div
    role="alert"
    class={`flex flex-wrap items-center gap-3 rounded-xl border border-status-warning/60 bg-surface-layer2 p-3 shadow-lg ${props.class}`}
    style={props.style}
  >
    <p class="flex-1 min-w-48 text-base text-text-primary">{t(props.text ?? "common.loadFailed")}</p>
    <button
      type="button"
      class="btn-primary min-h-11 px-4 text-base"
      onClick={() => (props.onAction ?? reloadPage)()}
    >
      {t(props.action ?? "common.reload")}
    </button>
    <Show when={props.onDismiss}>
      {(dismiss) => (
        <button
          type="button"
          class="w-11 h-11 flex items-center justify-center rounded-lg text-text-primary hover:bg-white/10"
          aria-label={t("common.close")}
          onClick={() => dismiss()()}
        >
          <X class="w-5 h-5" aria-hidden="true" />
        </button>
      )}
    </Show>
  </div>
);

/** Fixed at the top or bottom centre, above every open window. */
const floating = (position: "top" | "bottom") =>
  `fixed left-1/2 -translate-x-1/2 w-[min(36rem,calc(100vw-1rem))] ${position === "top" ? "top-2" : "bottom-4"}`;
const above = { "z-index": MODAL_LAYER.notice };

/** Inline notice for a lazy part of the page whose chunk failed. */
export const ChunkLoadNotice: Component<{
  position: "top" | "bottom";
  onReload?: () => void;
  /** Called once when the notice shows (the part is known to be missing). */
  onShown?: () => void;
  /** Offers a close button that calls this. */
  onDismiss?: () => void;
}> = (props) => {
  onMount(() => props.onShown?.());
  return (
    <Notice
      class={floating(props.position)}
      style={above}
      onAction={props.onReload}
      onDismiss={props.onDismiss}
    />
  );
};

/** A window whose code loaded but which failed while rendering. */
export const WindowErrorNotice: Component<{ onRetry: () => void; onDismiss: () => void }> = (
  props,
) => (
  <Notice
    class={floating("bottom")}
    style={above}
    text="common.windowFailed"
    action="common.tryAgain"
    onAction={props.onRetry}
    onDismiss={props.onDismiss}
  />
);

/**
 * Polite "Loading…" while a window's code downloads (`LazyDialog` sets
 * `visible` after a short delay, so fast loads show nothing). The live region
 * is there from the start, empty, so screen readers announce the text.
 */
export const LoadingNotice: Component<{ visible: boolean }> = (props) => (
  <div role="status" aria-live="polite" class={`${floating("bottom")} flex justify-center pointer-events-none`} style={above}>
    <Show when={props.visible}>
      <span class="rounded-full border border-border-subtle bg-surface-layer2 px-4 py-2 text-sm text-text-primary shadow-lg">
        {t("common.loading")}
      </span>
    </Show>
  </div>
);

/** Startup: the app renders only with all its texts, else a page that says so. */
export const StartupGate: Component<ParentProps<{ ready: boolean; onReload?: () => void }>> = (props) => {
  const failed = (): JSX.Element => (
    <div class="h-dvh flex items-center justify-center p-4 bg-background-tertiary">
      <Notice class="w-[min(36rem,100%)]" onAction={props.onReload} />
    </div>
  );
  return (
    <Show when={props.ready} fallback={failed()}>
      {props.children}
    </Show>
  );
};
