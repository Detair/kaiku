/**
 * Passkeys in the security settings (C29): list, add (browser only, after a
 * passkey step-up), rename and remove (after a step-up for that passkey).
 * The first passkey of an account comes from a key device (DC1), which L2
 * adds here; without a passkey there is no Add button.
 */

import { Component, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { t } from "@/i18n";
import { formatRelativeTime, formatTimestamp } from "@/i18n/format";
import { formatPasskeyError } from "@/lib/authErrors";
import { isTauri } from "@/lib/tauri/common";
import {
  deletePasskey,
  listPasskeys,
  PasskeyError,
  registerPasskey,
  renamePasskey,
  stepUpWithPasskey,
  type Passkey,
} from "@/lib/tauri/passkeys";
import { signalRemoved } from "@/lib/webauthn";
import { errorText } from "@/i18n/errors";

/** "Firefox on Linux" for a new passkey's default name. */
function defaultLabel(): string {
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Safari\//.test(ua)
          ? "Safari"
          : "Browser";
  const os = /Android/.test(ua)
    ? "Android"
    : /Windows/.test(ua)
      ? "Windows"
      : /Mac OS X|Macintosh/.test(ua)
        ? "macOS"
        : /Linux/.test(ua)
          ? "Linux"
          : "";
  return os ? t("passkeys.settings.defaultLabel", { browser, os }) : browser;
}

function failureText(err: unknown): string {
  if (err instanceof PasskeyError) return formatPasskeyError(err);
  return errorText(err, "passkeys.error.failed");
}

const quiet = (err: unknown) =>
  err instanceof PasskeyError && ["PASSKEY_CANCELLED", "ABORTED"].includes(err.code);

const btn =
  "min-h-12 px-3 rounded-lg bg-white/10 hover:bg-white/20 text-text-primary text-sm state-disabled";

const PasskeysSection: Component = () => {
  const desktop = isTauri();
  const [data, { refetch }] = createResource(listPasskeys);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [editing, setEditing] = createSignal<string | null>(null);
  const [name, setName] = createSignal("");
  const [confirming, setConfirming] = createSignal<string | null>(null);
  let ctl: AbortController | undefined;
  onCleanup(() => ctl?.abort());

  const passkeys = () => data()?.passkeys.filter((p) => p.state !== "revoked") ?? [];
  const hasActive = () => passkeys().some((p) => p.state === "active");

  const run = async (action: (signal: AbortSignal) => Promise<void>) => {
    ctl?.abort();
    const c = new AbortController();
    ctl = c;
    setBusy(true);
    setError("");
    try {
      await action(c.signal);
      await refetch();
    } catch (err) {
      if (!c.signal.aborted && !quiet(err)) setError(failureText(err));
    } finally {
      if (ctl === c) setBusy(false);
    }
  };

  const add = () =>
    run(async (signal) => {
      await stepUpWithPasskey("passkey.add", null, signal);
      await registerPasskey({ label: defaultLabel(), signal });
    });

  const remove = (p: Passkey) =>
    run(async (signal) => {
      await stepUpWithPasskey("passkey.remove", p.id, signal);
      const removed = await deletePasskey(p.id);
      setConfirming(null);
      if (!desktop) await signalRemoved(removed);
    });

  const save = (p: Passkey) => {
    const label = name().trim();
    if (label.length < 1 || [...label].length > 64) {
      setError(t("passkeys.settings.labelInvalid"));
      return;
    }
    void run(async () => {
      await renamePasskey(p.id, label);
      setEditing(null);
    });
  };

  return (
    <section class="space-y-3" aria-labelledby="passkeys-title">
      <h3 id="passkeys-title" class="text-base font-semibold text-text-primary">
        {t("passkeys.settings.title")}
      </h3>
      <p class="text-sm text-text-secondary">{t("passkeys.settings.intro")}</p>

      <Show when={!data.loading} fallback={null}>
        <Show when={!data.error} fallback={<p role="alert" class="text-sm text-text-primary">{t("passkeys.settings.loadFailed")}</p>}>
          <Show when={passkeys().length > 0} fallback={<p class="text-sm text-text-secondary">{t("passkeys.settings.empty")}</p>}>
            <ul class="space-y-2">
              <For each={passkeys()}>
                {(p) => (
                  <li class="p-3 rounded-lg bg-white/5 border border-white/10 space-y-2">
                    <Show
                      when={editing() === p.id}
                      fallback={
                        <div class="flex flex-wrap items-center gap-2">
                          <span class="font-medium text-text-primary">{p.label}</span>
                          <Show when={p.backup_state}>
                            <span class="px-2 py-0.5 rounded bg-accent-primary/20 text-xs text-text-primary">
                              {t("passkeys.settings.synced")}
                            </span>
                          </Show>
                        </div>
                      }
                    >
                      <label class="block text-sm text-text-secondary" for={`passkey-name-${p.id}`}>
                        {t("passkeys.settings.nameLabel")}
                      </label>
                      <input
                        id={`passkey-name-${p.id}`}
                        class="input-field"
                        value={name()}
                        onInput={(e) => setName(e.currentTarget.value)}
                        maxLength={128}
                      />
                    </Show>
                    <p class="text-sm text-text-secondary">
                      <Show
                        when={p.state === "pending"}
                        fallback={
                          p.last_used_at
                            ? t("passkeys.settings.lastUsed", { time: formatRelativeTime(p.last_used_at) })
                            : t("passkeys.settings.neverUsed")
                        }
                      >
                        {t("passkeys.settings.pending", { time: p.ready_at ? formatTimestamp(p.ready_at) : "" })}
                      </Show>
                    </p>
                    <Show when={p.state === "active"}>
                      <Show
                        when={confirming() === p.id}
                        fallback={
                          <div class="flex flex-wrap gap-2">
                            <Show
                              when={editing() === p.id}
                              fallback={
                                <button type="button" class={btn} disabled={busy()} onClick={() => { setName(p.label); setEditing(p.id); }}>
                                  {t("passkeys.settings.rename")}
                                </button>
                              }
                            >
                              <button type="button" class={btn} disabled={busy()} onClick={() => save(p)}>
                                {t("passkeys.settings.save")}
                              </button>
                            </Show>
                            <button type="button" class={btn} disabled={busy()} onClick={() => { setError(""); setConfirming(p.id); }}>
                              {t("passkeys.settings.remove")}
                            </button>
                          </div>
                        }
                      >
                        <div class="space-y-2">
                          <p class="text-sm text-text-primary">{t("passkeys.settings.removeConfirm", { label: p.label })}</p>
                          <Show when={desktop}>
                            <p class="text-sm text-text-secondary">{t("passkeys.settings.removeDesktopHint")}</p>
                          </Show>
                          <div class="flex flex-wrap gap-2">
                            <button type="button" class={btn} disabled={busy()} onClick={() => void remove(p)}>
                              {t("passkeys.settings.removeConfirmButton")}
                            </button>
                            <button type="button" class={btn} onClick={() => { ctl?.abort(); setBusy(false); setConfirming(null); }}>
                              {t("passkeys.login.cancel")}
                            </button>
                          </div>
                        </div>
                      </Show>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <Show
            when={!desktop}
            fallback={<p class="text-sm text-text-secondary">{t("passkeys.settings.desktopNotice")}</p>}
          >
            <Show when={hasActive()}>
              <button type="button" class="btn-primary min-h-12 px-4" disabled={busy()} onClick={() => void add()}>
                {t("passkeys.settings.add")}
              </button>
            </Show>
          </Show>
        </Show>
      </Show>

      <Show when={error()}>
        <p role="alert" class="text-sm text-text-primary">{error()}</p>
      </Show>
    </section>
  );
};

export default PasskeysSection;
