/**
 * /register/bind — right after registering (F8): within 30 minutes bind a
 * passkey (browser only), the Kaiku app on a phone, or an authenticator app,
 * or the account is deleted. No session is made here; the user then signs in.
 */

import { Component, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { A, useSearchParams } from "@solidjs/router";
import { plural, t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import { isTauri } from "@/lib/tauri/common";
import {
  finishRegistrationPasskey,
  getRegistration,
  startRegistrationPasskey,
  startRegistrationTotp,
  verifyRegistrationTotp,
} from "@/lib/tauri/registration";
import { createPasskeyFromJSON } from "@/lib/passkeySeam";
import { safeReturnUrl } from "@/lib/returnUrl";
import type { TotpSetup } from "@/lib/types";
import QrCodeImage from "@/components/auth/QrCodeImage";
import RecoveryCodesDisplay from "@/components/auth/RecoveryCodesDisplay";
import EnrollmentIssuerPanel from "@/components/settings/security/EnrollmentIssuerPanel";
import AuthPageShell, { errorStyle, fieldInput, fieldLabel } from "@/components/auth/AuthPageShell";

type Choice = null | "phone" | "totp";
type Done = null | "passkey" | "phone" | "totp";

const choice = "w-full min-h-14 px-4 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-lg text-text-primary text-left state-disabled";

const RegistrationBind: Component = () => {
  document.title = `${t("auth.bind.title")} | Kaiku`;
  const [searchParams] = useSearchParams();
  const desktop = isTauri();
  const [expiresAt, setExpiresAt] = createSignal<number | null>(null);
  const [now, setNow] = createSignal(Date.now());
  const [picked, setPicked] = createSignal<Choice>(null);
  const [done, setDone] = createSignal<Done>(null);
  const [totp, setTotp] = createSignal<TotpSetup | null>(null);
  const [code, setCode] = createSignal("");
  const [codes, setCodes] = createSignal<string[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const timer = setInterval(() => setNow(Date.now()), 15_000);
  onCleanup(() => clearInterval(timer));

  const loginHref = () => {
    const back = safeReturnUrl(searchParams.returnUrl);
    return back ? `/login?returnUrl=${encodeURIComponent(back)}` : "/login";
  };

  onMount(async () => {
    try {
      const r = await getRegistration();
      if (r.bound) setDone("phone");
      const at = Date.parse(r.expires_at);
      if (!Number.isNaN(at)) setExpiresAt(at);
    } catch (err) {
      setError(errorText(err, "auth.bind.loadFailed", { NO_PRE_SESSION_SECRET: "auth.bind.lost" }));
    }
  });

  const minutesLeft = () => {
    const at = expiresAt();
    return at === null ? null : Math.max(0, Math.ceil((at - now()) / 60_000));
  };

  const run = async (job: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await job();
    } catch (err) {
      setError(errorText(err, "auth.bind.failed"));
    } finally {
      setBusy(false);
    }
  };

  const usePasskey = () =>
    run(async () => {
      const start = await startRegistrationPasskey(null);
      const credential = await createPasskeyFromJSON(start.options, new AbortController().signal);
      await finishRegistrationPasskey(start.ceremony_id, credential);
      setDone("passkey");
    });

  const useTotp = () =>
    run(async () => {
      setPicked("totp");
      setTotp(await startRegistrationTotp());
    });

  const verify = (e: Event) => {
    e.preventDefault();
    const c = code().replace(/\D+/g, "");
    void run(async () => {
      setCodes(await verifyRegistrationTotp(c));
      setDone("totp");
    });
  };

  return (
    <AuthPageShell title={t("auth.bind.title")}>
      <Show when={minutesLeft() !== null && !done()}>
        <p class="text-base text-text-primary" role="status">
          {plural(minutesLeft()!, { one: "auth.bind.countdown.one", other: "auth.bind.countdown.other" })}
        </p>
      </Show>
      <Switch>
        <Match when={codes()}>
          {(list) => <RecoveryCodesDisplay codes={list()} onDone={() => setCodes(null)} />}
        </Match>
        <Match when={done()}>
          {(how) => (
            <>
              <p class="text-lg text-text-primary" role="status">
                {how() === "passkey"
                  ? t("auth.bind.donePasskey")
                  : how() === "phone"
                    ? t("auth.bind.donePhone")
                    : t("auth.bind.doneTotp")}
              </p>
              <A href={loginHref()} class="btn-primary w-full min-h-12 text-base flex items-center justify-center">
                {t("auth.setup.signIn")}
              </A>
            </>
          )}
        </Match>
        <Match when={picked() === "phone"}>
          <EnrollmentIssuerPanel issuer={{ kind: "registration" }} onDone={() => setDone("phone")} onCancel={() => setPicked(null)} />
        </Match>
        <Match when={picked() === "totp"}>
          <Show when={totp()} fallback={<p class="text-base text-text-secondary">{t("common.loading")}</p>}>
            {(setup) => (
              <form class="space-y-3" onSubmit={verify} noValidate>
                <p class="text-base text-text-primary">{t("auth.bind.totpIntro")}</p>
                <div class="flex justify-center">
                  <QrCodeImage payload={setup().otpauth_uri} alt={t("auth.request.totpQrAlt")} />
                </div>
                <label for="bind-totp" class={fieldLabel}>{t("auth.totp.label")}</label>
                <input id="bind-totp" class={`${fieldInput} font-mono`} inputmode="numeric" autocomplete="one-time-code" value={code()} onInput={(e) => setCode(e.currentTarget.value)} />
                <button type="submit" class="btn-primary w-full min-h-12 text-base" disabled={busy()}>{t("security.code.confirm")}</button>
              </form>
            )}
          </Show>
        </Match>
        <Match when={true}>
          <p class="text-base text-text-primary">{t("auth.bind.intro")}</p>
          <div class="space-y-2">
            <Show when={!desktop}>
              <button type="button" class={choice} disabled={busy()} onClick={() => void usePasskey()}>
                {t("auth.bind.passkey")}
              </button>
            </Show>
            <button type="button" class={choice} onClick={() => setPicked("phone")}>
              {t("auth.bind.phone")}
            </button>
            <button type="button" class={choice} disabled={busy()} onClick={() => void useTotp()}>
              {t("auth.bind.totp")}
            </button>
          </div>
        </Match>
      </Switch>
      <Show when={error()}>
        <p role="alert" class="p-3 rounded-md text-sm" style={errorStyle}>{error()}</p>
      </Show>
    </AuthPageShell>
  );
};

export default RegistrationBind;
