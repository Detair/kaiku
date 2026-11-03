/**
 * /secure-account — "Secure my account" (F8 bootstrap): an account with only
 * a password (or SSO) and an authenticator app starts a request that adds a
 * phone or passkey after the delay.
 */

import { Component, For, Show, createResource, createSignal } from "solid-js";
import { A, useNavigate } from "@solidjs/router";
import { t } from "@/i18n";
import { fetchServerSettings } from "@/lib/tauri/auth";
import { getServerUrl } from "@/lib/tauri/common";
import PasswordInput from "@/components/ui/PasswordInput";
import AuthPageShell, { errorStyle, fieldInput, fieldLabel } from "@/components/auth/AuthPageShell";
import { startBootstrap } from "@/stores/accountRequest";
import { requestOidcCode } from "@/lib/requestOidc";
import type { BootstrapStart } from "@/lib/types";
import { requestStartError } from "./requestErrors";

const SecureAccount: Component = () => {
  document.title = `${t("auth.secure.title")} | Kaiku`;
  const navigate = useNavigate();
  const server = () => getServerUrl() || window.location.origin;
  const [settings] = createResource(async () => {
    try {
      return await fetchServerSettings(server());
    } catch {
      return null;
    }
  });
  const [username, setUsername] = createSignal("");
  const [first, setFirst] = createSignal<"password" | "oidc">("password");
  const [password, setPassword] = createSignal("");
  const [totp, setTotp] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const providers = () => settings()?.oidc_providers ?? [];

  const submit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    const code = totp().replace(/\D+/g, "");
    if (!username().trim()) {
      setError(t("auth.recover.missing"));
      return;
    }
    if ((first() === "password" && !password()) || code.length < 6) {
      setError(t("auth.secure.needBoth"));
      return;
    }
    setBusy(true);
    try {
      const factor: BootstrapStart["first"] =
        first() === "password"
          ? { type: "password", password: password() }
          : { type: "oidc", oidc_code: await requestOidcCode(server(), providers()[0]?.slug ?? "") };
      await startBootstrap({ username: username().trim(), purpose: "bootstrap", first: factor, totp_code: code });
      navigate("/account-request", { replace: true });
    } catch (err) {
      setError(requestStartError(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AuthPageShell title={t("auth.secure.title")}>
      <p class="text-base text-text-primary">{t("auth.secure.intro")}</p>
      <form class="space-y-4" onSubmit={submit} noValidate>
        <div>
          <label for="secure-username" class={fieldLabel}>{t("common.username")}</label>
          <input id="secure-username" class={fieldInput} autocomplete="username" value={username()} onInput={(e) => setUsername(e.currentTarget.value)} />
        </div>
        <Show when={providers().length > 0}>
          <fieldset class="space-y-2">
            <legend class={fieldLabel}>{t("auth.secure.first")}</legend>
            <For each={[["password", "auth.recover.second.password"], ["oidc", "auth.recover.second.oidc"]] as const}>
              {([value, key]) => (
                <label class="flex items-center gap-3 min-h-12 text-base text-text-primary">
                  <input type="radio" name="secure-first" checked={first() === value} onClick={() => setFirst(value)} />
                  {t(key)}
                </label>
              )}
            </For>
          </fieldset>
        </Show>
        <Show when={first() === "password"}>
          <div>
            <label for="secure-password" class={fieldLabel}>{t("auth.login.password")}</label>
            <PasswordInput id="secure-password" class={fieldInput} autocomplete="current-password" value={password()} onInput={(e) => setPassword(e.currentTarget.value)} />
          </div>
        </Show>
        <div>
          <label for="secure-totp" class={fieldLabel}>{t("auth.totp.label")}</label>
          <input id="secure-totp" class={`${fieldInput} font-mono`} inputmode="numeric" autocomplete="one-time-code" value={totp()} onInput={(e) => setTotp(e.currentTarget.value)} />
          <p class="text-sm text-text-secondary mt-1">{t("auth.totp.hint")}</p>
        </div>
        <Show when={error()}>
          <p role="alert" class="p-3 rounded-md text-sm" style={errorStyle}>{error()}</p>
        </Show>
        <button type="submit" class="btn-primary w-full min-h-12 text-base" disabled={busy()}>
          {t("auth.secure.submit")}
        </button>
      </form>
      <A href="/login" class="text-primary auth-link text-base">{t("auth.flow.back")}</A>
    </AuthPageShell>
  );
};

export default SecureAccount;
