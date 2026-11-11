/**
 * Replacement for `window.confirm()` (ADMIN-13): `ask()` opens the shared
 * ConfirmActionDialog with a named target and consequence; render `<Host />`
 * once in the component. The action runs inside the dialog, so a failure
 * keeps it open with the reason.
 */
import { Component, Show, createSignal } from "solid-js";
import ConfirmActionDialog from "./ConfirmActionDialog";

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  run: () => Promise<unknown> | unknown;
}

export function createConfirm(): { ask: (req: ConfirmRequest) => void; Host: Component } {
  const [pending, setPending] = createSignal<ConfirmRequest | null>(null);
  const Host: Component = () => (
    <Show when={pending()}>
      {(req) => (
        <ConfirmActionDialog
          title={req().title}
          message={req().message}
          confirmLabel={req().confirmLabel}
          danger={req().danger}
          onConfirm={async () => {
            await req().run();
          }}
          onDone={() => setPending(null)}
          onCancel={() => setPending(null)}
        />
      )}
    </Show>
  );
  return { ask: (req) => setPending(req), Host };
}
