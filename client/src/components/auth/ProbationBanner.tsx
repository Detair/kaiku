/**
 * ProbationBanner — a chat-only session (B.4) says what it cannot do and how
 * to get a full session: "Sign in with a passkey" when the account has one,
 * otherwise where to add one. It collapses to a labelled chip for this run of
 * the app and is never hidden for good. It takes its own row above the page and
 * never covers the channel header or the message box.
 */

import { Component, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { ShieldAlert } from "lucide-solid";
import { t } from "@/i18n";
import { isProbation, replaceSessionWithPasskey } from "@/stores/auth";
import { meSecurity, refreshMeSecurity } from "@/stores/security";
import { canUsePasskeys } from "@/lib/tauri/passkeys";
import { passkeyFailureText } from "@/lib/authErrors";
import { isTauri } from "@/lib/tauri/common";
import LinuxPasskeyHint from "./LinuxPasskeyHint";

// Per chat-only session, not stored: the banner comes back after a restart
// and for every new chat-only session.
const [collapsed, setCollapsed] = createSignal(false);

const ProbationBanner: Component = () => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  // The banner stays mounted while sessions change: whenever this turns into
  // a chat-only session, load the overview (passkey button or hint); a full
  // session in between opens the banner again for the next one.
  /** The running sign-in; it ends with the banner or the session (Fable m6). */
  let attempt: AbortController | null = null;
  onCleanup(() => attempt?.abort());
  createEffect(() => {
    if (isProbation()) void refreshMeSecurity();
    else {
      // A full session in between: the next chat-only session starts clean
      // (Argon review 1 #2).
      attempt?.abort();
      setCollapsed(false);
      setError(null);
    }
  });
  const hasPasskey = () => (meSecurity()?.passkeys ?? 0) > 0;
  const canSignIn = () => hasPasskey() && canUsePasskeys();

  const signIn = async () => {
    attempt?.abort();
    const ctl = new AbortController();
    attempt = ctl;
    setBusy(true);
    setError(null);
    try {
      await replaceSessionWithPasskey(ctl.signal);
    } catch (err) {
      if (!ctl.signal.aborted) setError(passkeyFailureText(err, "auth.probation.signInFailed") || null);
    } finally {
      if (attempt === ctl) {
        attempt = null;
        setBusy(false);
      }
    }
  };

  return (
    <Show when={isProbation()}>
      <Show
        when={!collapsed()}
        fallback={
          <button
            type="button"
            class="shrink-0 w-full flex items-center justify-center gap-2 px-4 min-h-11 border-b border-status-warning/60 bg-surface-layer2 text-sm text-text-primary"
            onClick={() => setCollapsed(false)}
            aria-expanded="false"
          >
            <ShieldAlert class="w-5 h-5 text-status-warning" aria-hidden="true" />
            {t("auth.probation.chip")}
          </button>
        }
      >
        <div
          role="region"
          aria-label={t("auth.probation.chip")}
          class="w-full shrink-0 flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2 bg-surface-layer2 border-b border-status-warning/60"
        >
          <div class="flex items-start gap-3 flex-1 min-w-64">
            <ShieldAlert class="w-5 h-5 mt-0.5 text-status-warning shrink-0" aria-hidden="true" />
            <p class="text-sm text-text-primary">{t("auth.probation.text")}</p>
          </div>
          <Show when={error()}>
            <p role="alert" class="order-last w-full text-sm text-text-primary">{error()}</p>
          </Show>
          <div class="flex flex-wrap shrink-0 max-w-full items-center justify-end gap-2">
            <Show
              when={canSignIn()}
              fallback={
                <Show when={meSecurity() && !hasPasskey()}>
                  <p class="text-sm text-text-secondary">{t("auth.probation.addPasskeyHint")}</p>
                </Show>
              }
            >
              <button type="button" class="btn-primary min-h-11 text-sm" disabled={busy()} onClick={() => void signIn()}>
                {t("auth.probation.signIn")}
              </button>
            </Show>
            <Show when={isTauri() && busy()}>
              <div role="status" class="w-full space-y-1 text-sm">
                <p class="text-text-primary">{t("passkeys.login.browserHint")}</p>
                <LinuxPasskeyHint />
              </div>
            </Show>
            <button
              type="button"
              class="min-h-11 px-3 rounded-lg text-sm text-text-secondary hover:text-text-primary hover:bg-white/5"
              onClick={() => setCollapsed(true)}
              aria-expanded="true"
            >
              {t("auth.probation.hide")}
            </button>
          </div>
        </div>
      </Show>
    </Show>
  );
};

export default ProbationBanner;
