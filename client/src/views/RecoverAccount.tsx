/**
 * /recover — "I lost my phone / forgot my password" (F6): a recovery code
 * plus a second factor start a request that completes after the delay
 * unless the owner, the recovery contact or an admin cancels it.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import { t } from "@/i18n";
import { fetchServerSettings } from "@/lib/tauri/auth";
import { resolveSignInServer } from "@/lib/signInServer";
import { isAuthenticated } from "@/stores/auth";
import PasswordInput from "@/components/ui/PasswordInput";
import RecoveryCodeInput from "@/components/auth/RecoveryCodeInput";
import AuthPageShell, { errorStyle, fieldInput, fieldLabel } from "@/components/auth/AuthPageShell";
import { requestServer, startRecovery } from "@/stores/accountRequest";
import { requestOidcCode } from "@/lib/requestOidc";
import type { RecoveryStart } from "@/lib/types";
import { requestStartError } from "./requestErrors";

type Second = "password" | "totp" | "oidc";

const RecoverAccount: Component = () => {
  document.title = `${t("auth.recover.title")} | Kaiku`;
  const navigate = useNavigate();
  // Desktop: a fresh start has no session; the sign-in page's server (or
  // the native override) is the one to ask (Codex review M3).
  const [serverChoice] = createResource(() => resolveSignInServer(isAuthenticated()));
  const server = () => serverChoice() ?? requestServer();
  const [settings] = createResource(serverChoice, async (url) => {
    try {
      return await fetchServerSettings(url);
    } catch {
      return null;
    }
  });
  const [username, setUsername] = createSignal("");
  const [code, setCode] = createSignal("");
  const [second, setSecond] = createSignal<Second>("password");
  const [password, setPassword] = createSignal("");
  const [totp, setTotp] = createSignal("");
  const [provider, setProvider] = createSignal("");
  const [lostPhone, setLostPhone] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const providers = () => settings()?.oidc_providers ?? [];

  const submit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    if (!username().trim() || code().length < 8) {
      setError(t("auth.recover.missing"));
      return;
    }
    // C27: a valid recovery code with a failed second factor is spent (B.5),
    // so an empty factor never goes out with it (Argon review 10 #1).
    const totpDigits = totp().replace(/\D+/g, "");
    if ((second() === "password" && !password()) || (second() === "totp" && totpDigits.length < 6)) {
      setError(t("auth.recover.missing"));
      return;
    }
    setBusy(true);
    try {
      let factor: RecoveryStart["second"];
      if (second() === "password") factor = { type: "password", password: password() };
      else if (second() === "totp") factor = { type: "totp", code: totp().replace(/\D+/g, "") };
      else factor = { type: "oidc", oidc_code: await requestOidcCode(server(), provider() || providers()[0]?.slug || "") };
      await startRecovery({
        username: username().trim(),
        purpose: "recovery",
        recovery_code: code(),
        second: factor,
        lost: lostPhone() ? "all_key_devices" : "none",
      });
      navigate("/account-request", { replace: true });
    } catch (err) {
      setError(requestStartError(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AuthPageShell title={t("auth.recover.title")}>
      <p class="text-base text-text-primary">{t("auth.recover.intro")}</p>
      <p class="text-base text-text-primary" role="note">{t("auth.recover.stolen")}</p>
      <form class="space-y-4" onSubmit={submit} noValidate>
        <div>
          <label for="recover-username" class={fieldLabel}>{t("common.username")}</label>
          <input id="recover-username" class={fieldInput} autocomplete="username" value={username()} onInput={(e) => setUsername(e.currentTarget.value)} />
        </div>
        <RecoveryCodeInput value={code()} onInput={setCode} label={t("auth.recover.code")} />
        <fieldset class="space-y-2">
          <legend class={fieldLabel}>{t("auth.recover.second")}</legend>
          <For each={[["password", "auth.recover.second.password"], ["totp", "auth.recover.second.totp"], ["oidc", "auth.recover.second.oidc"]] as const}>
            {([value, key]) => (
              <Show when={value !== "oidc" || providers().length > 0}>
                <label class="flex items-center gap-3 min-h-12 text-base text-text-primary">
                  <input type="radio" name="recover-second" checked={second() === value} onClick={() => setSecond(value)} />
                  {t(key)}
                </label>
              </Show>
            )}
          </For>
        </fieldset>
        <Show when={second() === "password"}>
          <div>
            <label for="recover-password" class={fieldLabel}>{t("auth.login.password")}</label>
            <PasswordInput id="recover-password" class={fieldInput} autocomplete="current-password" value={password()} onInput={(e) => setPassword(e.currentTarget.value)} />
          </div>
        </Show>
        <Show when={second() === "totp"}>
          <div>
            <label for="recover-totp" class={fieldLabel}>{t("auth.totp.label")}</label>
            <input id="recover-totp" class={`${fieldInput} font-mono`} inputmode="numeric" autocomplete="one-time-code" value={totp()} onInput={(e) => setTotp(e.currentTarget.value)} />
          </div>
        </Show>
        <Show when={second() === "oidc" && providers().length > 1}>
          <div>
            <label for="recover-provider" class={fieldLabel}>{t("auth.recover.provider")}</label>
            <select id="recover-provider" class={fieldInput} value={provider()} onChange={(e) => setProvider(e.currentTarget.value)}>
              <For each={providers()}>{(p) => <option value={p.slug}>{p.display_name}</option>}</For>
            </select>
          </div>
        </Show>
        <label class="flex items-center gap-3 min-h-12 text-base text-text-primary">
          <input type="checkbox" checked={lostPhone()} onChange={(e) => setLostPhone(e.currentTarget.checked)} />
          {t("auth.recover.lostPhone")}
        </label>
        <Show when={error()}>
          <p role="alert" class="p-3 rounded-md text-sm" style={errorStyle}>{error()}</p>
        </Show>
        <button type="submit" class="btn-primary w-full min-h-12 text-base" disabled={busy()}>
          {t("auth.recover.submit")}
        </button>
      </form>
      <p class="text-base text-text-secondary">{t("auth.recover.noCodes")}</p>
      <A href="/login" class="text-primary auth-link text-base">{t("auth.flow.back")}</A>
    </AuthPageShell>
  );
};

export default RecoverAccount;
