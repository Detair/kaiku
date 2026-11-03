/**
 * UserSecurityPanel — an admin's view of one account's sign-in security
 * (C27 admin user view, F8 "Admins see per user"): phones and passkeys,
 * password and TOTP state, recovery codes, the recovery contact,
 * `may_approve`, an open request, sessions and the account's security log.
 * Every change steps up with the account or the authenticator as target.
 * Immediate actions on another admin's account are hidden (F6 rev 4).
 * Phones are set up from the admin's own phone only (coordinator call 1).
 */

import { Component, For, Show, createSignal } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import {
  adminRemoveAuthenticator,
  adminSetFactors,
  adminSetMayApprove,
  listAdminSecurityEvents,
} from "@/lib/tauri/adminSecurity";
import { cancelAccountRequest } from "@/lib/tauri/security";
import { withStepUp } from "@/lib/stepUp";
import type { UserDetailsResponse } from "@/lib/types";
import Toggle from "@/components/ui/Toggle";
import SecurityLogSection from "@/components/settings/security/SecurityLogSection";
import { securityErrorText } from "@/components/settings/security/securityErrors";

const STATE: Record<string, MessageKey> = {
  active: "security.overview.state.active",
  throttled: "security.overview.state.throttled",
  compromised: "security.overview.state.compromised",
};

const small = "min-h-11 px-3 rounded-lg border border-white/15 bg-white/5 hover:bg-white/10 text-sm text-text-primary state-disabled";

export interface UserSecurityPanelProps {
  user: UserDetailsResponse;
  meId: string;
}

const UserSecurityPanel: Component<UserSecurityPanelProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [done, setDone] = createSignal<Set<string>>(new Set());
  const [mayApprove, setMayApprove] = createSignal(props.user.may_approve === true);
  const [password, setPassword] = createSignal(props.user.password_state ?? null);
  const isAdminTarget = () => props.user.is_admin === true;
  const userId = () => props.user.id;
  const name = () => props.user.display_name || props.user.username;

  const run = async (job: () => Promise<unknown>, mark?: string) => {
    setBusy(true);
    setError(null);
    try {
      await job();
      if (mark) setDone((s) => new Set(s).add(mark));
    } catch (err) {
      setError(securityErrorText(err, "admin.userSecurity.failed"));
    } finally {
      setBusy(false);
    }
  };

  const removeNow = (id: string, label: string) =>
    run(
      () =>
        withStepUp({ action: "admin.authenticator.remove", target: id, targetLabel: label }, () =>
          adminRemoveAuthenticator(userId(), id),
        ),
      id,
    );

  const toggleMayApprove = (next: boolean) =>
    run(async () => {
      await withStepUp({ action: "admin.may_approve", target: userId(), targetLabel: name() }, () =>
        adminSetMayApprove(userId(), next),
      );
      setMayApprove(next);
    });

  const setPasswordState = (state: "active" | "compromised") =>
    run(async () => {
      await withStepUp({ action: "admin.factor.set_state", target: userId(), targetLabel: name() }, () =>
        adminSetFactors(userId(), { password: state }),
      );
      setPassword(state);
    });

  const cancelRequest = (id: string) =>
    run(
      () =>
        withStepUp({ action: "request.cancel", target: id, targetLabel: name() }, () =>
          cancelAccountRequest(id, null, null),
        ),
      id,
    );

  const authenticators = () => [...(props.user.key_devices ?? []), ...(props.user.passkeys ?? [])];

  return (
    <section class="space-y-4 pt-4 border-t border-white/10" data-testid="admin-user-security">
      <h4 class="text-sm font-semibold text-text-primary">{t("admin.userSecurity.title")}</h4>
      <Show when={isAdminTarget()}>
        <p class="text-sm text-text-secondary">{t("security.error.adminTarget")}</p>
      </Show>

      <div class="space-y-2">
        <p class="text-sm text-text-secondary">{t("admin.userSecurity.authenticators")}</p>
        <Show when={authenticators().length > 0} fallback={<p class="text-sm text-text-primary">{t("security.overview.none")}</p>}>
          <ul class="space-y-1">
            <For each={authenticators()}>
              {(a) => (
                <li class="flex flex-wrap items-center gap-2 p-2 rounded-lg bg-white/5">
                  <span class="flex-1 text-sm text-text-primary">
                    {a.label}
                    <span class="text-text-secondary"> · {a.state === "pending" && a.ready_at ? t("security.keyDevices.readyAt", { time: formatTimestamp(a.ready_at) }) : a.state}</span>
                  </span>
                  <Show when={!isAdminTarget() && a.state !== "revoked" && !done().has(a.id)}>
                    <button
                      type="button"
                      class={small}
                      disabled={busy()}
                      aria-label={t("admin.userSecurity.removeNowNamed", { label: a.label })}
                      onClick={() => void removeNow(a.id, a.label)}
                    >
                      {t("security.action.removeDeviceNow")}
                    </button>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <p class="text-sm text-text-secondary">{t("admin.userSecurity.setupPhone", { name: name() })}</p>
      </div>

      <dl class="grid grid-cols-2 gap-2 text-sm">
        <dt class="text-text-secondary">{t("security.overview.password")}</dt>
        <dd class="text-text-primary">{password() ? t(STATE[password()!] ?? "security.overview.state.unknown") : t("security.overview.none")}</dd>
        <dt class="text-text-secondary">{t("security.overview.totp")}</dt>
        <dd class="text-text-primary">{props.user.totp_state ? t(STATE[props.user.totp_state] ?? "security.overview.state.unknown") : t("security.overview.none")}</dd>
        <dt class="text-text-secondary">{t("security.overview.recoveryCodes")}</dt>
        <dd class="text-text-primary">{props.user.recovery_codes_left ?? "-"}</dd>
        <dt class="text-text-secondary">{t("security.overview.contact")}</dt>
        <dd class="text-text-primary">{props.user.recovery_contact?.display_name ?? t("security.overview.none")}</dd>
        <Show when={props.user.open_sessions}>
          {(s) => (
            <>
              <dt class="text-text-secondary">{t("admin.userSecurity.sessions")}</dt>
              <dd class="text-text-primary">{t("admin.userSecurity.sessionsCount", { full: s().full, probation: s().probation })}</dd>
            </>
          )}
        </Show>
      </dl>
      <Show when={props.user.bootstrap_open}>
        <span class="inline-block px-2 py-0.5 text-xs rounded-md border border-white/15 text-text-primary">
          {t("admin.userSecurity.bootstrapOpen")}
        </span>
      </Show>

      <Show when={!isAdminTarget() && password()}>
        <div class="flex flex-wrap gap-2">
          <Show
            when={password() === "compromised"}
            fallback={
              <button type="button" class={small} disabled={busy()} onClick={() => void setPasswordState("compromised")}>
                {t("admin.userSecurity.markUnsafe")}
              </button>
            }
          >
            <button type="button" class={small} disabled={busy()} onClick={() => void setPasswordState("active")}>
              {t("admin.userSecurity.allowAgain")}
            </button>
          </Show>
        </div>
      </Show>

      <div class="space-y-1">
        <div class="flex items-center justify-between gap-3">
          <span class="text-sm text-text-primary">{t("admin.userSecurity.mayApprove")}</span>
          <Toggle
            checked={mayApprove()}
            onChange={(next) => void toggleMayApprove(next)}
            label={t("admin.userSecurity.mayApprove")}
            disabled={busy()}
          />
        </div>
        <p class="text-xs text-text-secondary">{t("admin.userSecurity.mayApproveHint")}</p>
      </div>

      <Show when={props.user.open_request}>
        {(r) => (
          <div class="p-3 rounded-lg border border-status-warning/50 space-y-2">
            <p class="text-sm text-text-primary">
              {t("admin.userSecurity.openRequest", { time: formatTimestamp(r().ready_at) })}
            </p>
            <Show when={!done().has(r().id)} fallback={<p class="text-sm text-text-primary">{t("security.action.cancelled")}</p>}>
              <button type="button" class={small} disabled={busy()} onClick={() => void cancelRequest(r().id)}>
                {t("security.request.cancel")}
              </button>
            </Show>
          </div>
        )}
      </Show>

      <Show when={error()}>
        <p role="alert" class="text-sm text-text-primary">{error()}</p>
      </Show>

      <SecurityLogSection
        load={(before, limit) => listAdminSecurityEvents(userId(), before, limit)}
        subject={{ user_id: userId(), display_name: name() }}
      />
    </section>
  );
};

export default UserSecurityPanel;
