/**
 * ReauthDialog - asks the user to prove who they are before a sensitive
 * action (contract C24). Offers only the methods passed in `methods`:
 * password (plus authenticator code when MFA is on) and/or a provider login.
 * `mfaOnly` asks for the authenticator code alone.
 */

import { Component, createSignal, For, onCleanup, Show } from "solid-js";
import { X } from "lucide-solid";
import type { ReauthRequest } from "@/lib/types";
import {
  isRejectedCode,
  reauthErrorCode,
  reauthErrorText,
  type ReauthMethod,
} from "@/lib/reauth";
import { createDialog } from "@/lib/a11y/dialog";
import { t } from "@/i18n";

export interface ReauthDialogProps {
  title: string;
  mode: "password" | "mfaOnly";
  mfaEnabled: boolean;
  methods: ReauthMethod[];
  onSubmit: (req: ReauthRequest) => Promise<void>;
  /** Called when the user picks a provider method. */
  onOidc?: (
    method: Extract<ReauthMethod, { kind: "oidc" }>,
    mfaCode: string | null,
    signal: AbortSignal,
  ) => Promise<void>;
  onClose: () => void;
}

const ReauthDialog: Component<ReauthDialogProps> = (props) => {
  const dialog = createDialog({ onClose: () => props.onClose() });
  const [password, setPassword] = createSignal("");
  const [code, setCode] = createSignal("");
  // Closing the dialog stops a running provider flow and drops the secrets.
  const abort = new AbortController();
  onCleanup(() => {
    abort.abort();
    setPassword("");
    setCode("");
  });
  const [error, setError] = createSignal("");
  // A rejected code may have been used moments ago (one use per 30 s step).
  const [codeHint, setCodeHint] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  // The server can tell us mid-way that the password path is not for this
  // account (switch to the provider) or that nothing is available.
  const [passwordHidden, setPasswordHidden] = createSignal(false);
  const [serverUnavailable, setServerUnavailable] = createSignal(false);

  const mfaOnly = () => props.mode === "mfaOnly";
  const showPassword = () =>
    !mfaOnly() &&
    !passwordHidden() &&
    props.methods.some((m) => m.kind === "password");
  const oidcMethods = () =>
    props.methods.filter(
      (m): m is Extract<ReauthMethod, { kind: "oidc" }> => m.kind === "oidc",
    );
  // The code is needed on every path (password and provider alike).
  const showCode = () =>
    props.mfaEnabled && (mfaOnly() || showPassword() || oidcMethods().length > 0);
  const unavailable = () =>
    !mfaOnly() &&
    (serverUnavailable() || (!showPassword() && oidcMethods().length === 0));

  const run = async (action: () => Promise<void>, sentCode: boolean) => {
    setBusy(true);
    setError("");
    setCodeHint(false);
    try {
      await action();
    } catch (err) {
      if (abort.signal.aborted) return;
      const code = reauthErrorCode(err);
      if (code === "REAUTH_OIDC_REQUIRED" && oidcMethods().length > 0) {
        setPasswordHidden(true);
      } else if (code === "REAUTH_UNAVAILABLE") {
        setServerUnavailable(true);
      }
      setError(reauthErrorText(err));
      setCodeHint(sentCode && isRejectedCode(err));
    } finally {
      if (!abort.signal.aborted) setBusy(false);
    }
  };

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    if (busy()) return;
    if (showPassword() && password() === "") {
      setCodeHint(false);
      setError(t("admin.reauth.error.validation"));
      return;
    }
    const mfa = code().replace(/\s+/g, "");
    if (showCode() && mfa === "") {
      setCodeHint(false);
      setError(t("admin.reauth.error.mfaRequired"));
      return;
    }
    void run(
      () =>
        props.onSubmit({
          password: showPassword() ? password() : "",
          mfa_code: showCode() ? mfa : null,
        }),
      showCode(),
    );
  };

  const inputClass =
    "w-full px-3 py-2 rounded-lg bg-white/10 text-text-primary text-sm border border-white/10 focus:outline-none focus:border-accent-primary disabled:opacity-60";
  const secondaryBtn =
    "px-3 py-2 rounded-lg bg-white/10 hover:bg-white/20 transition-colors text-text-primary text-sm disabled:opacity-60 disabled:cursor-not-allowed";

  return (
    <div
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/60"
      onClick={() => props.onClose()}
    >
      <div
        class="bg-background-secondary rounded-xl shadow-2xl w-full max-w-sm mx-4"
        {...dialog.dialogProps}
        ref={dialog.ref}
        onClick={(e) => e.stopPropagation()}
      >
        <div class="flex items-center justify-between p-4 border-b border-white/10">
          <h2 id={dialog.titleId} class="text-lg font-semibold text-text-primary">
            {props.title}
          </h2>
          <button
            type="button"
            onClick={() => props.onClose()}
            aria-label={t("admin.reauth.cancel")}
            class="p-1.5 hover:bg-white/10 rounded-lg transition-colors text-text-secondary"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        <Show
          when={!unavailable()}
          fallback={
            <div class="p-4 space-y-4">
              <p class="text-sm text-text-primary">{t("admin.reauth.unavailable")}</p>
              <div class="flex justify-end">
                <button type="button" class={secondaryBtn} onClick={() => props.onClose()}>
                  {t("admin.reauth.cancel")}
                </button>
              </div>
            </div>
          }
        >
          <form onSubmit={handleSubmit} class="p-4 space-y-4">
            <p class="text-sm text-text-secondary">{t("admin.reauth.intro")}</p>

            <Show when={showPassword()}>
              <label class="block space-y-1">
                <span class="text-sm text-text-secondary">{t("admin.reauth.password")}</span>
                <input
                  type="password"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  autocomplete="current-password"
                  class={inputClass}
                />
              </label>
            </Show>

            <Show when={showCode()}>
              <label class="block space-y-1">
                <span class="text-sm text-text-secondary">{t("admin.reauth.mfaCode")}</span>
                <input
                  type="text"
                  inputmode="numeric"
                  value={code()}
                  onInput={(e) => setCode(e.currentTarget.value)}
                  autocomplete="one-time-code"
                  spellcheck={false}
                  class={`${inputClass} font-mono`}
                />
                <span class="block text-xs text-text-secondary">
                  {t("admin.reauth.mfaCodeHint")}
                </span>
              </label>
            </Show>

            <Show when={error()}>
              <div role="alert" class="text-sm text-status-error space-y-1">
                <p>{error()}</p>
                <Show when={codeHint()}>
                  <p>{t("admin.reauth.codeUsedHint")}</p>
                </Show>
              </div>
            </Show>

            <div class="flex justify-end gap-2">
              <button type="button" class={secondaryBtn} onClick={() => props.onClose()}>
                {t("admin.reauth.cancel")}
              </button>
              <Show when={showPassword() || mfaOnly()}>
                <button
                  type="submit"
                  disabled={busy()}
                  class="px-4 py-2 bg-accent-primary hover:bg-accent-primary/80 rounded-lg transition-colors text-on-accent text-sm font-medium disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {t("admin.reauth.submit")}
                </button>
              </Show>
            </div>

            <Show when={!mfaOnly() && oidcMethods().length > 0}>
              <Show when={showPassword()}>
                <p class="text-center text-xs text-text-secondary">{t("admin.reauth.or")}</p>
              </Show>
              <div class="space-y-2">
                <For each={oidcMethods()}>
                  {(m) => (
                    <button
                      type="button"
                      disabled={busy()}
                      class={`${secondaryBtn} w-full`}
                      onClick={() => {
                        const { onOidc } = props;
                        if (!onOidc) return;
                        const mfa = code().replace(/\s+/g, "");
                        if (showCode() && mfa === "") {
                          setCodeHint(false);
                          setError(t("admin.reauth.error.mfaRequired"));
                          return;
                        }
                        // The server uses the code up; never offer it for a retry.
                        void run(
                          () =>
                            onOidc(m, showCode() ? mfa : null, abort.signal).finally(() =>
                              setCode(""),
                            ),
                          showCode(),
                        );
                      }}
                    >
                      {t("admin.reauth.oidc", { provider: m.label })}
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </form>
        </Show>
      </div>
    </div>
  );
};

export default ReauthDialog;
