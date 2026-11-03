/**
 * UserSecurityPanel — an admin's view of one account's sign-in security
 * (C27 admin user view, F8 "Admins see per user"): phones and passkeys,
 * password and TOTP state, recovery codes, the recovery contact,
 * `may_approve`, an open request, sessions and the account's security log.
 * Every change steps up with the account or the authenticator as target.
 * Immediate actions (removing devices, `may_approve`, factor states, the
 * recovery contact) are refused on another admin's account (F6 rev 4); an
 * admin account never gets a recovery contact (C28). Phones are set up from
 * the admin's own phone only (coordinator call 1).
 */

import { Component, For, Show, createSignal } from "solid-js";
import { t, type MessageKey } from "@/i18n";
import { formatTimestamp } from "@/i18n/format";
import {
  adminRemoveAuthenticator,
  adminSetFactors,
  adminSetMayApprove,
  adminSetRecoveryContact,
  listAdminSecurityEvents,
} from "@/lib/tauri/adminSecurity";
import { cancelAccountRequest } from "@/lib/tauri/security";
import { HttpError } from "@/lib/tauri/common";
import { codeIs } from "@/lib/errorCode";
import { withStepUp } from "@/lib/stepUp";
import type { UserDetailsResponse } from "@/lib/types";
import { adminState, loadUserDetails } from "@/stores/admin";
import Toggle from "@/components/ui/Toggle";
import { showToast } from "@/components/ui/Toast";
import SecurityLogSection from "@/components/settings/security/SecurityLogSection";
import { securityErrorText } from "@/components/settings/security/securityErrors";
import { contactCandidates } from "./recoveryContacts";
import { totalKey } from "@/lib/totalKey";

const STATE: Record<string, MessageKey> = {
  active: "security.overview.state.active",
  throttled: "security.overview.state.throttled",
  compromised: "security.overview.state.compromised",
};

/** Key device and passkey states (C26/C29); anything else is "Unknown state". */
const AUTHENTICATOR_STATE: Record<string, MessageKey> = {
  active: "security.keyDevices.state.active",
  pending: "security.keyDevices.state.pending",
  revoked: "security.keyDevices.state.revoked",
};

type Factor = "password" | "totp";
type FactorState = "active" | "compromised";

const FACTOR_BUTTON: Record<Factor, Record<FactorState, MessageKey>> = {
  password: {
    compromised: "admin.userSecurity.markPasswordUnsafe",
    active: "admin.userSecurity.allowPasswordAgain",
  },
  totp: {
    compromised: "admin.userSecurity.markTotpUnsafe",
    active: "admin.userSecurity.allowTotpAgain",
  },
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
  const [factors, setFactors] = createSignal<Record<Factor, string | null>>({
    password: props.user.password_state ?? null,
    totp: props.user.totp_state ?? null,
  });
  const [contactName, setContactName] = createSignal(props.user.recovery_contact?.display_name ?? null);
  const [contactPick, setContactPick] = createSignal("");
  const userId = () => props.user.id;
  const isAdmin = () => props.user.is_admin === true;
  /** Another admin's account: immediate actions are that admin's or the CLI's (F6 rev 4). */
  const isAdminTarget = () => isAdmin() && props.user.id !== props.meId;
  const name = () => props.user.display_name || props.user.username;
  const candidates = () => contactCandidates(adminState.users, [userId(), props.meId]);

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

  const setFactorState = (factor: Factor, state: FactorState) =>
    run(async () => {
      await withStepUp({ action: "admin.factor.set_state", target: userId(), targetLabel: name() }, () =>
        adminSetFactors(userId(), { [factor]: state }),
      );
      setFactors((f) => ({ ...f, [factor]: state }));
    });

  const saveContact = () => {
    const pick = candidates().find((u) => u.id === contactPick());
    if (!pick) return;
    return run(async () => {
      await withStepUp({ action: "admin.recovery_contact.set", target: userId(), targetLabel: name() }, () =>
        adminSetRecoveryContact(userId(), pick.id),
      );
      setContactName(pick.display_name || pick.username);
      setContactPick("");
    });
  };

  const cancelRequest = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await withStepUp({ action: "request.cancel", target: id, targetLabel: name() }, () =>
        cancelAccountRequest(id, null, null),
      );
      setDone((s) => new Set(s).add(id));
    } catch (err) {
      if (err instanceof HttpError && codeIs(err.code, "ACCOUNT_REQUEST_COMPLETED")) {
        // Lost against completion (C27): reload, so the new device shows with
        // "Remove device now" (the panel is rebuilt; the toast stays).
        showToast({ type: "warning", title: t("admin.userSecurity.tooLate") });
        void loadUserDetails(userId());
      } else {
        setError(securityErrorText(err, "admin.userSecurity.failed"));
      }
    } finally {
      setBusy(false);
    }
  };

  const authenticators = () => [...(props.user.key_devices ?? []), ...(props.user.passkeys ?? [])];
  const authenticatorState = (a: { state: string; ready_at?: string | null }) =>
    a.state === "pending" && a.ready_at
      ? t("security.keyDevices.readyAt", { time: formatTimestamp(a.ready_at) })
      : t(totalKey(AUTHENTICATOR_STATE, a.state, "security.keyDevices.state.unknown"));

  const factorText = (state: string | null) =>
    state ? t(totalKey(STATE, state, "security.overview.state.unknown")) : t("security.overview.none");

  /** "Mark as unsafe" / "Allow again" for one factor, if it is set. */
  const FactorButton = (p: { factor: Factor }) => (
    <Show when={factors()[p.factor]}>
      {(state) => {
        const next = (): FactorState => (state() === "compromised" ? "active" : "compromised");
        return (
          <button type="button" class={small} disabled={busy()} onClick={() => void setFactorState(p.factor, next())}>
            {t(FACTOR_BUTTON[p.factor][next()])}
          </button>
        );
      }}
    </Show>
  );

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
                    <span class="text-text-secondary"> · {authenticatorState(a)}</span>
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
        <dd class="text-text-primary">{factorText(factors().password)}</dd>
        <dt class="text-text-secondary">{t("security.overview.totp")}</dt>
        <dd class="text-text-primary">{factorText(factors().totp)}</dd>
        <dt class="text-text-secondary">{t("security.overview.recoveryCodes")}</dt>
        <dd class="text-text-primary">{props.user.recovery_codes_left ?? "-"}</dd>
        <dt class="text-text-secondary">{t("security.overview.contact")}</dt>
        <dd class="text-text-primary">{contactName() ?? t("security.overview.none")}</dd>
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

      <Show when={!isAdminTarget() && (factors().password || factors().totp)}>
        <div class="flex flex-wrap gap-2">
          <FactorButton factor="password" />
          <FactorButton factor="totp" />
        </div>
      </Show>

      <Show when={!isAdmin()}>
        <div class="flex flex-wrap items-center gap-2">
          <select
            class="min-h-11 flex-1 rounded-lg border border-white/15 bg-surface-base px-2 text-sm text-text-primary"
            aria-label={t("security.overview.contact")}
            value={contactPick()}
            disabled={busy()}
            onChange={(e) => setContactPick(e.currentTarget.value)}
          >
            <option value="" disabled>
              {t("admin.userSecurity.contactChoose")}
            </option>
            <For each={candidates()}>{(u) => <option value={u.id}>{u.display_name || u.username}</option>}</For>
          </select>
          <button type="button" class={small} disabled={busy() || !contactPick()} onClick={() => void saveContact()}>
            {t("admin.userSecurity.contactSave")}
          </button>
        </div>
      </Show>

      <div class="space-y-1">
        <div class="flex items-center justify-between gap-3">
          <span class="text-sm text-text-primary">{t("admin.userSecurity.mayApprove")}</span>
          <Toggle
            checked={mayApprove()}
            onChange={(next) => void toggleMayApprove(next)}
            label={t("admin.userSecurity.mayApprove")}
            disabled={busy() || isAdminTarget()}
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
