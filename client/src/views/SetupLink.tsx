/**
 * /setup — a setup link opened in a browser (C27, C29). On Android: "Open in
 * Kaiku". In the desktop app: open it in a browser instead (registration
 * never runs there). On a computer: add a passkey for the account the link
 * was made for, show the code to type on the issuing device, and wait.
 */

import { Component, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { A } from "@solidjs/router";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { formatTimestamp } from "@/i18n/format";
import { isTauri } from "@/lib/tauri/common";
import { claimStatus, finishPasskeyClaim, startPasskeyClaim } from "@/lib/tauri/enrollments";
import { androidIntentUrl, isAndroidBrowser, parseSetupFragment, type SetupFragment } from "@/lib/setupLink";
import { createPasskeyCredential } from "@/lib/tauri/passkeys";
import { passkeyFailureText } from "@/lib/authErrors";
import { formatCode } from "@/components/auth/CodeEntry";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import type { ClaimStatus, PasskeyClaimStart } from "@/lib/types";

const SETUP_ERRORS = {
  ENROLLMENT_TOKEN_INVALID: "auth.setup.invalid",
  ENROLLMENT_ALREADY_CLAIMED: "auth.setup.used",
  PASSKEY_ALREADY_REGISTERED: "auth.setup.alreadyRegistered",
  PASSKEYS_UNAVAILABLE: "auth.setup.passkeysUnavailable",
} as const;

type Phase =
  | { kind: "loading" }
  | { kind: "ask"; start: PasskeyClaimStart }
  | { kind: "code"; code: string; status: ClaimStatus | null };

const SetupLink: Component = () => {
  document.title = `${t("auth.setup.pageTitle")} | Kaiku`;
  // Read once, then drop the token from the address bar (and history).
  const fragment: SetupFragment = parseSetupFragment(window.location.hash);
  if (window.location.hash) {
    window.history.replaceState(null, "", window.location.pathname + window.location.search);
  }
  const token = "token" in fragment ? fragment.token : "";
  const android = isAndroidBrowser(navigator.userAgent);
  const desktop = isTauri();

  const [phase, setPhase] = createSignal<Phase>({ kind: "loading" });
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const abort = new AbortController();
  onCleanup(() => abort.abort());

  onMount(async () => {
    if (!token || android || desktop) return;
    try {
      setPhase({ kind: "ask", start: await startPasskeyClaim(token) });
    } catch (err) {
      setError(errorText(err, "auth.setup.failed", SETUP_ERRORS));
    }
  });

  const poll = async (enrollmentId: string, code: string) => {
    for (;;) {
      if (abort.signal.aborted) return;
      const started = Date.now();
      try {
        const status = await claimStatus(enrollmentId, 25);
        if (abort.signal.aborted) return;
        setPhase({ kind: "code", code, status });
        if (status.state !== "claimed" && status.state !== "issued") return;
      } catch (err) {
        if (abort.signal.aborted) return;
        // A refused status (claim secret gone, enrolment unknown) will not
        // change by asking again (Argon review 10 #6).
        if (err instanceof HttpError && [401, 403, 404, 410].includes(err.status)) {
          setError(errorText(err, "auth.setup.failed", SETUP_ERRORS));
          return;
        }
        console.warn("[Setup] Status poll failed:", err instanceof Error ? err.message : err);
        await new Promise((r) => setTimeout(r, 5_000));
        continue;
      }
      // Never more often than once a second, even if a server answers at once.
      const elapsed = Date.now() - started;
      if (elapsed < 1_000) await new Promise((r) => setTimeout(r, 1_000 - elapsed));
    }
  };

  const createPasskey = async () => {
    const p = phase();
    if (p.kind !== "ask") return;
    setBusy(true);
    setError(null);
    try {
      const credential = await createPasskeyCredential(p.start.options, abort.signal);
      const claimed = await finishPasskeyClaim(token, p.start.ceremony_id, credential, null);
      setPhase({ kind: "code", code: claimed.code, status: null });
      void poll(claimed.enrollment_id, claimed.code);
    } catch (err) {
      setError(passkeyFailureText(err, "auth.setup.failed", SETUP_ERRORS) || null);
      // The ceremony is single use: a retry needs a fresh one, from the token
      // kept in memory (the address bar no longer has it) (Argon 10 #3).
      const dead =
        err instanceof HttpError && ["ENROLLMENT_TOKEN_INVALID", "ENROLLMENT_ALREADY_CLAIMED"].some((c) => codeIs(err.code, c));
      if (!dead && !abort.signal.aborted) {
        try {
          setPhase({ kind: "ask", start: await startPasskeyClaim(token) });
        } catch (again) {
          setError(errorText(again, "auth.setup.failed", SETUP_ERRORS));
        }
      }
    } finally {
      setBusy(false);
    }
  };

  const statusText = (s: ClaimStatus) => {
    if (s.state === "done" && s.authenticator_state === "pending" && s.ready_at) {
      return t("auth.setup.pending", { time: formatTimestamp(s.ready_at) });
    }
    if (s.state === "done") return t("auth.setup.done");
    if (s.state === "cancelled") return t("auth.setup.cancelled");
    if (s.state === "expired") return t("auth.setup.expired");
    if (s.state === "failed") return t("auth.setup.wrongCodes");
    return null;
  };

  return (
    <div class="min-h-dvh flex items-center justify-center bg-background-primary p-4">
      <main class="w-full max-w-md bg-background-secondary rounded-xl shadow-lg p-6 space-y-5 text-center">
        <h1 class="text-2xl font-bold text-text-primary">{t("auth.setup.heading")}</h1>
        <Switch>
          <Match when={"error" in fragment && fragment.error === "version"}>
            <p class="text-base text-text-primary">{t("auth.setup.update")}</p>
          </Match>
          <Match when={!token}>
            <p class="text-base text-text-primary">{t("auth.setup.invalid")}</p>
          </Match>
          <Match when={android}>
            <a
              href={androidIntentUrl(window.location.origin, token)}
              class="btn-primary w-full min-h-14 text-lg flex items-center justify-center"
            >
              {t("auth.setup.openInApp")}
            </a>
            <p class="text-base text-text-secondary">{t("auth.setup.noApp")}</p>
          </Match>
          <Match when={desktop}>
            <p class="text-base text-text-primary">{t("auth.setup.openInBrowser")}</p>
          </Match>
          <Match when={(() => { const p = phase(); return p.kind === "ask" ? p : null; })()}>
            {(p) => (
              <>
                <p class="text-lg text-text-primary">
                  {t("auth.setup.ask", { name: p().start.subject.display_name, server: p().start.server_name })}
                </p>
                <button type="button" class="btn-primary w-full min-h-12 text-base" disabled={busy()} onClick={() => void createPasskey()}>
                  {t("auth.setup.createPasskey")}
                </button>
              </>
            )}
          </Match>
          <Match when={(() => { const p = phase(); return p.kind === "code" ? p : null; })()}>
            {(p) => (
              <Show
                when={p().status && statusText(p().status!)}
                fallback={
                  <>
                    <p class="text-base text-text-primary">{t("auth.setup.typeCode")}</p>
                    <p class="font-mono text-5xl font-bold tracking-widest text-text-primary" aria-live="polite">
                      {formatCode(p().code)}
                    </p>
                  </>
                }
              >
                {(text) => (
                  <>
                    <p class="text-lg text-text-primary" role="status">{text()}</p>
                    {/* A pending passkey cannot sign in yet (409 PASSKEY_PENDING). */}
                    <Show when={p().status?.state === "done" && p().status?.authenticator_state !== "pending"}>
                      <A href="/login" class="btn-primary w-full min-h-12 text-base flex items-center justify-center">
                        {t("auth.setup.signIn")}
                      </A>
                    </Show>
                  </>
                )}
              </Show>
            )}
          </Match>
        </Switch>
        <Show when={error()}>
          <p
            role="alert"
            class="p-3 rounded-md text-sm"
            style={{ "background-color": "var(--color-error-bg)", border: "1px solid var(--color-error-border)", color: "var(--color-error-text)" }}
          >
            {error()}
          </p>
        </Show>
      </main>
    </div>
  );
};

export default SetupLink;
