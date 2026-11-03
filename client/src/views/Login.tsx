import { Component, createEffect, createMemo, createSignal, createResource, Match, Show, Switch, For, on, onCleanup, onMount } from "solid-js";
import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { authState, beginSignInChoice, clearError, loginWithPasskey } from "@/stores/auth";
import { cancelAttempt, fetchServerSettings, getDefaultServerUrl, oidcAuthorize } from "@/lib/tauri";
import { decodeLoginStep, type LoginStep, type OidcProvider } from "@/lib/types";
import { KeyRound, Smartphone } from "lucide-solid";
import { Github, Chrome } from "@/components/icons/BrandIcons";
import PasswordInput from "@/components/ui/PasswordInput";
import { getThemeImage } from "@/lib/themeImage";
import { t } from "@/i18n";
import { isTauri } from "@/lib/tauri/common";
import { openBlankSsoPopup, reloadAfterSsoPopup, watchSsoPopup } from "@/lib/oidcPopup";
import { safeReturnUrl } from "@/lib/returnUrl";
import { setSignInServer } from "@/lib/signInServer";
import { errorText, SSO_ERRORS } from "@/i18n/errors";
import { formatPasskeyError, LOGIN_ERRORS, loginErrorText } from "@/lib/authErrors";
import PasskeyButton from "@/components/auth/PasskeyButton";
import LinuxPasskeyHint from "@/components/auth/LinuxPasskeyHint";
import { PasskeyError } from "@/lib/tauri/passkeys";
import { isConditionalMediationAvailable, isWebAuthnAvailable } from "@/lib/webauthn";
import {
  applyLoginStep,
  leaveFlow,
  loginFlow,
  loginFlowBusy,
  loginFlowError,
  retryLoginFlow,
  setFlowServer,
  startPhoneSignIn,
  submitPassword,
  submitTotp,
  usePhoneNotAtHand,
} from "@/stores/loginFlow";
import QrLoginPanel from "@/components/auth/login/QrLoginPanel";
import ConfirmationPanel from "@/components/auth/login/ConfirmationPanel";
import TotpStepPanel from "@/components/auth/login/TotpStepPanel";
import AttemptEnded from "@/components/auth/login/AttemptEnded";

/** A conditional ceremony lives 300 s on the server; restart it before. */
const CONDITIONAL_RESTART_MS = 270_000;
/** Pause after a server change before autofill starts again. */
const SERVER_CHANGE_DEBOUNCE_MS = 500;

/** Map icon_hint to a Lucide icon component. */
function providerIcon(hint: string | null) {
  switch (hint) {
    case "github":
      return Github;
    case "chrome":
    case "google":
      return Chrome;
    default:
      return KeyRound;
  }
}

const Login: Component = () => {
  document.title = `${t("common.login")} | Kaiku`;
  onCleanup(() => { document.title = "Kaiku"; });
  const navigate = useNavigate();
  // SSO popup watcher: ends with the view so a late result cannot act on a dead page.
  let stopWatching: (() => void) | undefined;
  let ssoPopup: Window | null = null;
  // The popup closed without reporting back: offer a reload (see reloadAfterSsoPopup).
  const [popupClosed, setPopupClosed] = createSignal(false);
  let disposed = false;
  const dismissPopupClosed = () => {
    setPopupClosed(false);
    setOidcLoading(null);
  };
  onCleanup(() => {
    disposed = true;
    stopWatching?.();
    ssoPopup?.close();
    // SM1 "close the screen": a live attempt is cancelled.
    void leaveFlow();
  });
  const [searchParams] = useSearchParams();
  const defaultServerUrl = import.meta.env.VITE_SERVER_URL || window.location.origin;
  const [serverUrl, setServerUrl] = createSignal(defaultServerUrl);
  // Native mode: a tester override (KAIKU_SERVER_URL) beats the build-time default.
  onMount(async () => {
    try {
      const override = await getDefaultServerUrl();
      if (override) setServerUrl(override);
    } catch (err) {
      console.warn("[Login] Could not read default server URL:", err instanceof Error ? err.message : err);
    }
  });
  // The pre-session pages (recover, secure my account) use this server on
  // a desktop start without a session (Codex review M3).
  createEffect(() => setSignInServer(serverUrl()));
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [localError, setLocalError] = createSignal("");
  const [oidcLoading, setOidcLoading] = createSignal<string | null>(null);

  // Fetch server settings from hardcoded beta URL
  const [settings] = createResource(serverUrl, async (url) => {
    if (!url.trim()) return null;
    try {
      return await fetchServerSettings(url);
    } catch (err) {
      console.warn("[Login] Failed to fetch server settings:", err instanceof Error ? err.message : err);
      return null;
    }
  });

  // A sign-in step produced the session: continue where the user came from.
  createEffect(() => {
    if (authState.user && !authState.passwordChangeRequired) {
      navigate(safeReturnUrl(searchParams.returnUrl) ?? "/", { replace: true });
    }
  });

  // Passkeys (F1, C29): autofill (conditional mediation) runs in the
  // background in browsers; the button runs a modal ceremony (desktop: the
  // system browser).
  const desktop = isTauri();
  const showPasskey = () => desktop || isWebAuthnAvailable();
  const [passkeyBusy, setPasskeyBusy] = createSignal(false);
  let conditionalCtl: AbortController | undefined;
  let modalCtl: AbortController | undefined;
  let restartTimer: ReturnType<typeof setTimeout> | undefined;
  /** A pending autofill start after a server change. */
  let serverChangeTimer: ReturnType<typeof setTimeout> | undefined;

  // Bumped by every start and stop: a start still waiting for
  // `isConditionalMediationAvailable` gives way to anything later (Argon B R2).
  let conditionalGeneration = 0;
  const stopConditional = () => {
    conditionalGeneration += 1;
    conditionalCtl?.abort();
    conditionalCtl = undefined;
    clearTimeout(restartTimer);
    clearTimeout(serverChangeTimer);
  };

  /** Words a passkey failure; silent for cancels and aborts. */
  const showPasskeyError = (err: unknown) => {
    if (!(err instanceof PasskeyError)) {
      setLocalError(errorText(err, "passkeys.error.failed", LOGIN_ERRORS));
      return;
    }
    // PASSKEY_UNKNOWN was already signalled; PASSKEY_PENDING never is (B.1).
    const text = formatPasskeyError(err);
    if (text) setLocalError(text);
  };

  const afterPasskey = () =>
    navigate(safeReturnUrl(searchParams.returnUrl) ?? "/", { replace: true });

  // After a failed autofill (cancelled sheet, server error) it restarts,
  // with a growing pause so an offline server is not hammered.
  const FIRST_BACKOFF_MS = 2_000;
  let conditionalBackoff = FIRST_BACKOFF_MS;
  const startConditional = async () => {
    // Never beside the button's ceremony (it restarts autofill when done),
    // and only while the form (with its user-name field) is shown.
    if (desktop || disposed || modalCtl || !onForm() || !isWebAuthnAvailable()) return;
    const generation = ++conditionalGeneration;
    const available = await isConditionalMediationAvailable();
    if (!available || disposed || generation !== conditionalGeneration) return;
    stopConditional();
    const ctl = new AbortController();
    conditionalCtl = ctl;
    restartTimer = setTimeout(() => {
      if (conditionalCtl === ctl) void startConditional();
    }, CONDITIONAL_RESTART_MS);
    try {
      await loginWithPasskey(serverUrl(), {
        mediation: "conditional",
        signal: ctl.signal,
        // The name typed by the time a passkey is picked.
        username: () => username().trim() || undefined,
      });
      afterPasskey();
    } catch (err) {
      if (ctl.signal.aborted || conditionalCtl !== ctl) return;
      // Autofill stays quiet, except for a removed or a pending passkey.
      if (err instanceof PasskeyError && ["PASSKEY_UNKNOWN", "PASSKEY_PENDING"].includes(err.code)) {
        showPasskeyError(err);
      }
      clearTimeout(restartTimer);
      const delay = conditionalBackoff;
      conditionalBackoff = Math.min(conditionalBackoff * 2, 60_000);
      restartTimer = setTimeout(() => {
        if (conditionalCtl === ctl) void startConditional();
      }, delay);
    }
  };
  // A changed server gets its own autofill ceremony: the old one stops at
  // once, the new one starts after a pause (no ceremony per keystroke) and
  // with the first backoff again. The button's ceremony restarts autofill
  // itself when it ends.
  createEffect(
    on(
      serverUrl,
      () => {
        if (modalCtl) return;
        stopConditional();
        conditionalBackoff = FIRST_BACKOFF_MS;
        serverChangeTimer = setTimeout(() => void startConditional(), SERVER_CHANGE_DEBOUNCE_MS);
      },
      { defer: true },
    ),
  );
  onMount(() => {
    void startConditional();
  });
  onCleanup(() => {
    stopConditional();
    modalCtl?.abort();
    oidcCtl?.abort();
  });

  const handlePasskey = async () => {
    setLocalError("");
    clearError();
    // A new sign-in choice: an SSO wait, a password or phone attempt in
    // flight are stale now (Codex review M1).
    beginSignInChoice();
    stopConditional();
    modalCtl?.abort();
    const ctl = new AbortController();
    modalCtl = ctl;
    setPasskeyBusy(true);
    try {
      await loginWithPasskey(serverUrl(), {
        mediation: "optional",
        signal: ctl.signal,
        username: username().trim() || undefined,
      });
      afterPasskey();
    } catch (err) {
      if (!ctl.signal.aborted) showPasskeyError(err);
    } finally {
      if (modalCtl === ctl) {
        modalCtl = undefined;
        setPasskeyBusy(false);
        if (!disposed) void startConditional();
      }
    }
  };

  const cancelPasskey = () => {
    modalCtl?.abort();
    modalCtl = undefined;
    setPasskeyBusy(false);
  };

  /** "Use a passkey instead" on the QR: back to the form, then the button's ceremony. */
  const usePasskeyInstead = () => {
    void leaveFlow();
    void handlePasskey();
  };

  // Autofill belongs to the form: it stops while a sign-in step (QR,
  // confirmation, TOTP) replaces the form and starts again when the form
  // comes back.
  const onForm = createMemo(() => loginFlow().step === "form");
  createEffect(
    on(
      onForm,
      (shown) => {
        if (!shown) stopConditional();
        else if (!disposed) void startConditional();
      },
      { defer: true },
    ),
  );

  const handleLogin = async (e: Event) => {
    e.preventDefault();
    setLocalError("");
    clearError();

    if (!username().trim()) {
      setLocalError(t("auth.login.usernameRequired"));
      return;
    }
    if (!password()) {
      setLocalError(t("auth.login.passwordRequired"));
      return;
    }

    stopConditional();
    try {
      // Never a session: the answer is the confirmation QR or the TOTP step.
      await submitPassword(serverUrl(), username(), password());
      setPassword("");
    } catch (err) {
      if (!authState.error) setLocalError(loginErrorText(err));
      // The form stays: so does passkey autofill.
      if (!disposed) void startConditional();
    }
  };

  const handlePhoneSignIn = () => {
    setLocalError("");
    clearError();
    stopConditional();
    void startPhoneSignIn(serverUrl());
  };

  const handleTotp = (code: string) => {
    submitTotp(code).catch(() => {
      // The store keeps the step and words the failure (loginFlowError).
    });
  };

  /** The SSO flow of the current choice (desktop: cancels the Rust wait). */
  let oidcCtl: AbortController | undefined;

  const handleOidcLogin = async (provider: OidcProvider) => {
    setLocalError("");
    clearError();
    stopConditional();
    // One choice at a time (Codex review M1): this SSO sign-in makes every
    // other one stale, and it is stale itself once another starts. Its
    // server is the one chosen now, not when the answer arrives.
    const choice = beginSignInChoice();
    const server = serverUrl();
    oidcCtl?.abort();
    const ctl = new AbortController();
    oidcCtl = ctl;
    const stale = () => disposed || ctl.signal.aborted || !choice.isCurrent();
    choice.signal.addEventListener("abort", () => ctl.abort(), { once: true });

    // Browser: open the popup inside the click, before any await, or popup
    // blockers reject it. It is pointed at the provider once the server answers.
    stopWatching?.();
    const popup = isTauri() ? null : openBlankSsoPopup("oidc-login");
    ssoPopup = popup;
    if (!isTauri() && !popup) {
      setLocalError(t("auth.login.popupBlocked"));
      return;
    }
    setOidcLoading(provider.slug);
    ctl.signal.addEventListener(
      "abort",
      () => {
        stopWatching?.();
        popup?.close();
        if (oidcLoading() === provider.slug) setOidcLoading(null);
      },
      { once: true },
    );

    /** A step that arrived for a choice the user left: its attempt goes. */
    const dropStale = (attemptId: string) => {
      void cancelAttempt(attemptId).catch(() => undefined);
    };

    try {
      const result = await oidcAuthorize(server, provider.slug, ctl.signal);

      if (result.mode === "tauri") {
        // Tauri: Rust redeemed the hand-off code; the PC confirms next (F3).
        if (stale()) {
          dropStale(result.step.attempt.attempt_id);
          return;
        }
        setOidcLoading(null);
        setFlowServer(server);
        applyLoginStep(result.step, "oidc", server, choice);
        return;
      }

      // The view went away, or another sign-in was chosen: start nothing.
      if (stale()) {
        popup?.close();
        return;
      }

      // Browser: send the popup to the provider and wait for its callback
      const expectedOrigin = new URL(server).origin;
      stopWatching = watchSsoPopup({
        popup: popup!,
        expectedOrigin,
        onResult: (data) => {
          // The callback carries the next step (C27), never a token.
          setOidcLoading(null);
          let step: LoginStep;
          try {
            step = decodeLoginStep(data);
          } catch (err) {
            setLocalError(errorText(err, "auth.login.ssoFailed", SSO_ERRORS.byCode));
            return;
          }
          if (stale()) {
            dropStale(step.attempt.attempt_id);
            return;
          }
          setFlowServer(server);
          applyLoginStep(step, "oidc", server, choice);
        },
        onClosedWithoutResult: () => {
          // Closed by the user, or the callback lost its opener (IdP with
          // COOP) after the sign-in already set the refresh cookie.
          if (!stale()) setPopupClosed(true);
        },
      });
      popup!.location.href = result.authUrl;
    } catch (err) {
      stopWatching?.();
      popup?.close();
      if (stale()) return;
      setLocalError(errorText(err, "auth.login.ssoFailed", SSO_ERRORS.byCode));
      setOidcLoading(null);
    }
  };

  const showLocalLogin = () => {
    const s = settings();
    // Show local login if settings haven't loaded yet, or if local auth is enabled
    return !s || s.auth_methods.local;
  };

  const showOidc = () => {
    const s = settings();
    return s?.oidc_enabled && s.oidc_providers.length > 0;
  };

  // Offer registration only when the server says it is open.
  // Invite-only servers need an invite, so offer it only when we came from one.
  const inviteReturnUrl = () => {
    const url = safeReturnUrl(searchParams.returnUrl);
    return url && /^\/invite\/[A-Za-z0-9]+/.test(url) ? url : null;
  };
  const cameWithInvite = () => inviteReturnUrl() !== null;
  const showRegisterLink = () => {
    const policy = settings()?.registration_policy;
    return policy === "open" || (policy === "invite_only" && cameWithInvite());
  };

  const error = () => localError() || authState.error || loginFlowError();

  return (
    <div class="flex items-center justify-center min-h-dvh overflow-y-auto bg-background-primary">
      <div class="flex w-full max-w-4xl mx-4 bg-background-secondary rounded-lg shadow-lg overflow-hidden">
        {/* Left: Floki illustration */}
        <div class="hidden lg:flex w-1/2 items-center justify-center p-8 bg-surface-base">
          <img src={getThemeImage("floki_auth_welcome.png")} alt={t("auth.login.illustrationAlt")} class="w-full max-w-xs object-contain" loading="eager" decoding="async" fetchpriority="high" />
        </div>
        {/* Right: Form */}
        <div class="w-full lg:w-1/2 p-8">
        <h1 class="text-2xl font-bold mb-2 text-center text-text-primary">
          {t("auth.login.heading")}
        </h1>
        <p class="text-text-secondary text-center mb-6">
          {t("auth.login.subtitle")}
        </p>

        <Switch>
          <Match when={(() => { const f = loginFlow(); return f.step === "qr" ? f : null; })()}>
            {(f) => (
              <QrLoginPanel
                attempt={f().attempt}
                payload={f().qrPayload}
                onBack={() => void leaveFlow()}
                onUsePasskey={showPasskey() ? usePasskeyInstead : undefined}
              />
            )}
          </Match>
          <Match when={(() => { const f = loginFlow(); return f.step === "confirm" ? f : null; })()}>
            {(f) => (
              <ConfirmationPanel
                attempt={f().attempt}
                payload={f().qrPayload}
                via={f().via}
                onPhoneNotAtHand={usePhoneNotAtHand}
                onBack={() => void leaveFlow()}
              />
            )}
          </Match>
          <Match when={loginFlow().step === "totp"}>
            <TotpStepPanel
              error={loginFlowError()}
              busy={loginFlowBusy()}
              onSubmit={handleTotp}
              onBack={() => void leaveFlow()}
            />
          </Match>
          <Match when={(() => { const f = loginFlow(); return f.step === "ended" ? f : null; })()}>
            {(f) => (
              <AttemptEnded
                state={f().state}
                via={f().via}
                onRetry={() => void retryLoginFlow()}
                onBack={() => void leaveFlow()}
              />
            )}
          </Match>
          <Match when={loginFlow().step === "form"}>
          {/* Passkeys first (F1) */}
          <Show when={showPasskey()}>
            <div class="mb-4 space-y-2">
              <PasskeyButton busy={passkeyBusy()} onClick={() => void handlePasskey()} />
              <Show when={desktop && passkeyBusy()}>
                <div role="status" class="p-3 rounded-lg border border-white/10 bg-white/5 text-sm text-text-primary space-y-2">
                  <p>{t("passkeys.login.browserHint")}</p>
                  <LinuxPasskeyHint />
                  <button
                    type="button"
                    class="w-full min-h-12 rounded-lg border border-white/10 text-text-primary hover:bg-white/10"
                    onClick={cancelPasskey}
                  >
                    {t("passkeys.login.cancel")}
                  </button>
                </div>
              </Show>
            </div>
          </Show>
          <button
            type="button"
            class="btn-primary w-full min-h-12 flex items-center justify-center gap-2 text-base mb-4"
            disabled={authState.isLoading || loginFlowBusy()}
            onClick={handlePhoneSignIn}
          >
            <Smartphone class="w-5 h-5" aria-hidden="true" />
            {t("auth.phone.signInButton")}
          </button>

          {/* SSO Buttons */}
          <Show when={showOidc()}>
            <div class="space-y-2 mb-4">
              <For each={settings()!.oidc_providers}>
                {(provider) => {
                  const Icon = providerIcon(provider.icon_hint);
                  return (
                    <button
                      type="button"
                      class="w-full min-h-12 flex items-center justify-center gap-3 px-4 py-2.5 rounded-lg border border-white/10 bg-white/5 hover:bg-white/10 text-text-primary text-base font-medium transition-colors"
                      disabled={authState.isLoading || !!oidcLoading()}
                      onClick={() => handleOidcLogin(provider)}
                    >
                      <Show
                        when={oidcLoading() !== provider.slug}
                        fallback={
                          <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                        }
                      >
                        <Icon class="w-4 h-4" />
                      </Show>
                      {t("auth.login.continueWith", { provider: provider.display_name })}
                    </button>
                  );
                }}
              </For>
            </div>
            <Show when={popupClosed()}>
              <div class="mb-4 flex items-center justify-between gap-3 text-sm text-text-secondary" role="status">
                <span>{t("auth.login.popupClosed")}</span>
                <span class="flex shrink-0 gap-3">
                  <button type="button" class="underline" onClick={reloadAfterSsoPopup}>
                    {t("auth.login.reload")}
                  </button>
                  <button type="button" class="underline" onClick={dismissPopupClosed}>
                    {t("common.cancel")}
                  </button>
                </span>
              </div>
            </Show>
          </Show>

          <Show when={showLocalLogin()}>
            <div class="relative my-5">
              <div class="absolute inset-0 flex items-center">
                <div class="w-full border-t border-white/10" />
              </div>
              <div class="relative flex justify-center text-xs">
                <span class="bg-background-secondary px-3 text-text-secondary">
                  {t("auth.login.or")}
                </span>
              </div>
            </div>
          </Show>

          {/* Local Login Form */}
          <Show when={showLocalLogin()}>
            <form onSubmit={handleLogin} method="post" noValidate class="space-y-4">
              <div>
                <label for="login-username" class="block text-sm font-medium text-text-secondary mb-1">
                  {t("common.username")}
                </label>
                <input
                  id="login-username"
                  type="text"
                  class="input-field"
                  name="username"
                  autocomplete="username webauthn"
                  placeholder={t("auth.login.usernamePlaceholder")}
                  data-testid="login-username"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <div>
                <label for="login-password" class="block text-sm font-medium text-text-secondary mb-1">
                  {t("auth.login.password")}
                </label>
                <PasswordInput
                  id="login-password"
                  class="input-field"
                  name="password"
                  autocomplete="current-password"
                  placeholder={t("auth.login.passwordPlaceholder")}
                  data-testid="login-password"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                  disabled={authState.isLoading}
                  required
                />
              </div>

              <Show when={error()}>
                <div
                  role="alert"
                  class="p-3 rounded-md text-sm"
                  data-testid="login-error"
                  style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
                >
                  {error()}
                </div>
              </Show>

              <button
                type="submit"
                class="btn-primary w-full min-h-12 flex items-center justify-center gap-2 text-base"
                data-testid="login-submit"
                disabled={authState.isLoading}
              >
                <Show
                  when={!authState.isLoading}
                  fallback={
                    <>
                      <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      {t("auth.login.submitting")}
                    </>
                  }
                >
                  {t("common.login")}
                </Show>
              </button>
            </form>
          </Show>

          {/* Error display when no local login form */}
          <Show when={!showLocalLogin() && error()}>
            <div
              role="alert"
              class="p-3 rounded-md text-sm"
              style={{"background-color":"var(--color-error-bg)","border":"1px solid var(--color-error-border)","color":"var(--color-error-text)"}}
            >
              {error()}
            </div>
          </Show>

          <nav class="mt-5 flex flex-col items-center gap-2 text-base" aria-label={t("auth.login.helpLinks")}>
            <A href="/recover" class="text-primary auth-link py-2">
              {t("auth.login.lostPhone")}
            </A>
            <A href="/secure-account" class="text-primary auth-link py-2">
              {t("auth.login.secureAccount")}
            </A>
          </nav>

          <Show when={showRegisterLink()}>
            <p class="flex justify-center gap-1 text-sm text-text-secondary mt-4">
              <span>{t("auth.login.noAccount")}</span>
              <A
                href={
                  inviteReturnUrl()
                    ? `/register?returnUrl=${encodeURIComponent(inviteReturnUrl()!)}`
                    : "/register"
                }
                class="text-primary auth-link"
              >
                {t("auth.login.registerLink")}
              </A>
            </p>
          </Show>
          </Match>
        </Switch>
        </div>
      </div>
    </div>
  );
};

export default Login;
