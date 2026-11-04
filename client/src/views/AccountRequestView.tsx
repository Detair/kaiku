/**
 * /account-request — the request client (SM3): while it waits, when it is
 * ready and what to stage (a passkey in this browser, a phone, a password, an
 * authenticator app, a recovery contact), Finish with the new recovery codes,
 * and the end states. The request secret is a cookie (browser) or stays in
 * Rust (desktop); the page never sees it.
 */

import { Component, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { A } from "@solidjs/router";
import { t, type MessageKey } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import { isTauri } from "@/lib/tauri/common";
import {
  finishRequestPasskey,
  setRequestPassword,
  setRequestRecoveryContact,
  startRequestPasskey,
  startRequestTotp,
  verifyRequestTotp,
} from "@/lib/tauri/accountRequests";
import { PasskeyError, createPasskeyCredential } from "@/lib/tauri/passkeys";
import { requestStepError } from "./requestErrors";
import { passkeyFailureText } from "@/lib/authErrors";
import type { AccountRequestState, TotpSetup } from "@/lib/types";
import { finish, reloadRequest, request, withdraw } from "@/stores/accountRequest";
import PasswordInput from "@/components/ui/PasswordInput";
import QrCodeImage from "@/components/auth/QrCodeImage";
import RecoveryCodesDisplay from "@/components/auth/RecoveryCodesDisplay";
import EnrollmentIssuerPanel from "@/components/settings/security/EnrollmentIssuerPanel";
import AuthPageShell, { errorStyle, fieldInput, fieldLabel } from "@/components/auth/AuthPageShell";

const END_TEXT: Partial<Record<AccountRequestState, MessageKey>> = {
  completed: "auth.request.completed",
  cancelled: "auth.request.cancelled",
  withdrawn: "auth.request.withdrawn",
  expired: "auth.request.expired",
  void: "auth.request.void",
  unknown: "auth.request.unknownState",
};


const item = "w-full min-h-12 px-4 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-base text-text-primary text-left state-disabled";

type Stage = null | "phone" | "password" | "totp" | "contact";

const AccountRequestView: Component = () => {
  document.title = `${t("auth.request.title")} | Kaiku`;
  const desktop = isTauri();
  const [stage, setStage] = createSignal<Stage>(null);
  const [codes, setCodes] = createSignal<string[] | null>(null);
  const [finished, setFinished] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [password, setPassword] = createSignal("");
  const [totp, setTotp] = createSignal<TotpSetup | null>(null);
  const [totpCode, setTotpCode] = createSignal("");
  const [contact, setContact] = createSignal("");
  let timer: ReturnType<typeof setInterval> | undefined;

  const load = async () => {
    try {
      await reloadRequest();
    } catch (err) {
      setError(requestStepError(err, "auth.request.loadFailed"));
    }
  };
  onMount(() => {
    void load();
    // A waiting request becomes ready by itself; check now and then.
    timer = setInterval(() => {
      if (request()?.state === "waiting") void load();
    }, 60_000);
  });
  onCleanup(() => clearInterval(timer));

  const run = async (job: () => Promise<unknown>, reload = true) => {
    setBusy(true);
    setError(null);
    try {
      await job();
      if (reload) await reloadRequest();
      setStage(null);
    } catch (err) {
      setError(
        err instanceof PasskeyError ? passkeyFailureText(err, "auth.request.failed") || null : requestStepError(err, "auth.request.failed"),
      );
    } finally {
      setBusy(false);
    }
  };

  const addPasskey = () =>
    run(async () => {
      const start = await startRequestPasskey(null);
      const credential = await createPasskeyCredential(start.options, new AbortController().signal);
      await finishRequestPasskey(start.ceremony_id, credential);
    });

  const beginTotp = async () => {
    setTotp(null);
    setStage("totp");
    setError(null);
    try {
      setTotp(await startRequestTotp());
    } catch (err) {
      // Back to the choices, with the reason (Argon review 9 #1).
      setStage(null);
      setError(requestStepError(err, "auth.request.failed"));
    }
  };

  const doFinish = async () => {
    setBusy(true);
    setError(null);
    try {
      setCodes(await finish());
      setFinished(true);
    } catch (err) {
      setError(requestStepError(err, "auth.request.failed"));
    } finally {
      setBusy(false);
    }
  };

  const r = () => request();
  const staged = () => r()?.staged;

  return (
    <AuthPageShell title={t("auth.request.title")}>
      <Switch
        fallback={
          // A request that cannot be loaded (secret gone, ended) offers a new
          // start instead of "Loading" for ever (Argon review 9 #4).
          <Show when={!error()} fallback={
            <A href="/recover" class="btn-primary w-full min-h-12 text-base flex items-center justify-center">
              {t("auth.request.startAgain")}
            </A>
          }>
            <p class="text-base text-text-secondary">{t("common.loading")}</p>
          </Show>
        }
      >
        <Match when={codes()}>
          {(list) => <RecoveryCodesDisplay codes={list()} onDone={() => setCodes(null)} />}
        </Match>
        <Match when={finished()}>
          <p class="text-base text-text-primary" role="status">{t("auth.request.finished")}</p>
          <A href="/login" class="btn-primary w-full min-h-12 text-base flex items-center justify-center">
            {t("auth.request.signIn")}
          </A>
        </Match>
        <Match when={r()?.state === "waiting"}>
          <p class="text-base text-text-primary">{t("auth.request.waiting", { time: formatTimestamp(r()!.ready_at) })}</p>
          <button type="button" class={item} disabled={busy()} onClick={() => void run(withdraw, false)}>
            {t("auth.request.withdraw")}
          </button>
        </Match>
        <Match when={r()?.state === "ready"}>
          <Show when={r()!.expires_at}>
            {(deadline) => <p class="text-base text-text-primary">{t("auth.request.finishBy", { time: formatTimestamp(deadline()) })}</p>}
          </Show>
          <p class="text-base text-text-secondary">{t("auth.request.stageIntro")}</p>
          <Show when={staged()?.authenticator}>
            {(a) => <p class="text-base text-text-primary" role="status">{t("auth.request.stagedAuthenticator", { label: a().label })}</p>}
          </Show>
          <Switch>
            <Match when={stage() === "phone"}>
              <EnrollmentIssuerPanel
                issuer={{ kind: "request" }}
                onDone={() => {
                  // Back to the choices, where "Finish" is (Argon review 9 #1).
                  setStage(null);
                  void load();
                }}
                onCancel={() => setStage(null)}
              />
            </Match>
            <Match when={stage() === "password"}>
              <form class="space-y-3" onSubmit={(e) => { e.preventDefault(); const pw = password(); void run(() => setRequestPassword(pw)); }} noValidate>
                <label for="req-password" class={fieldLabel}>{t("security.password.new")}</label>
                <PasswordInput id="req-password" class={fieldInput} autocomplete="new-password" value={password()} onInput={(e) => setPassword(e.currentTarget.value)} />
                <button type="submit" class="btn-primary min-h-12 text-base" disabled={busy()}>{t("security.password.save")}</button>
                <button type="button" class={item} onClick={() => setStage(null)}>{t("auth.flow.back")}</button>
              </form>
            </Match>
            <Match when={stage() === "totp"}>
              <Show when={totp()} fallback={<p class="text-base text-text-secondary">{t("common.loading")}</p>}>
                {(setup) => (
                  <form class="space-y-3" onSubmit={(e) => { e.preventDefault(); const c = totpCode().replace(/\D+/g, ""); void run(() => verifyRequestTotp(c)); }} noValidate>
                    <div class="flex justify-center">
                      <QrCodeImage payload={setup().otpauth_uri} alt={t("auth.request.totpQrAlt")} />
                    </div>
                    <label for="req-totp" class={fieldLabel}>{t("auth.totp.label")}</label>
                    <input id="req-totp" class={`${fieldInput} font-mono`} inputmode="numeric" autocomplete="one-time-code" value={totpCode()} onInput={(e) => setTotpCode(e.currentTarget.value)} />
                    <button type="submit" class="btn-primary min-h-12 text-base" disabled={busy()}>{t("security.code.confirm")}</button>
                    <button type="button" class={item} onClick={() => setStage(null)}>{t("auth.flow.back")}</button>
                  </form>
                )}
              </Show>
            </Match>
            <Match when={stage() === "contact"}>
              <form class="space-y-3" onSubmit={(e) => { e.preventDefault(); const u = contact().trim(); void run(() => setRequestRecoveryContact(u)); }} noValidate>
                <label for="req-contact" class={fieldLabel}>{t("auth.request.contactUsername")}</label>
                <input id="req-contact" class={fieldInput} value={contact()} onInput={(e) => setContact(e.currentTarget.value)} />
                <button type="submit" class="btn-primary min-h-12 text-base" disabled={busy()}>{t("common.save")}</button>
                <button type="button" class={item} onClick={() => setStage(null)}>{t("auth.flow.back")}</button>
              </form>
            </Match>
            <Match when={stage() === null}>
              <div class="space-y-2">
                <Show when={!desktop} fallback={<p class="text-base text-text-secondary">{t("auth.request.passkeyInBrowser")}</p>}>
                  <button type="button" class={item} disabled={busy()} onClick={() => void addPasskey()}>
                    {t("auth.request.addPasskey")}
                  </button>
                </Show>
                <button type="button" class={item} onClick={() => setStage("phone")}>{t("security.setupPhone.button")}</button>
                <button type="button" class={item} onClick={() => setStage("password")}>
                  {staged()?.password ? t("auth.request.passwordStaged") : t("auth.request.newPassword")}
                </button>
                <button type="button" class={item} onClick={() => void beginTotp()}>
                  {staged()?.totp ? t("auth.request.totpStaged") : t("auth.request.newTotp")}
                </button>
                <button type="button" class={item} onClick={() => setStage("contact")}>
                  {staged()?.recovery_contact ? t("auth.request.contactStaged", { name: staged()!.recovery_contact! }) : t("auth.request.contact")}
                </button>
              </div>
              <button type="button" class="btn-primary w-full min-h-12 text-base" disabled={busy()} onClick={() => void doFinish()}>
                {t("auth.request.finish")}
              </button>
            </Match>
          </Switch>
        </Match>
        <Match when={r() && END_TEXT[r()!.state]}>
          {(key) => (
            <>
              <p class="text-base text-text-primary" role="status">{t(key())}</p>
              <A href="/recover" class="btn-primary w-full min-h-12 text-base flex items-center justify-center">
                {t("auth.request.startAgain")}
              </A>
            </>
          )}
        </Match>
      </Switch>
      <Show when={error()}>
        <p role="alert" class="p-3 rounded-md text-sm" style={errorStyle}>{error()}</p>
      </Show>
    </AuthPageShell>
  );
};

export default AccountRequestView;
