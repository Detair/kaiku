/**
 * The network part of a notice's action (C28 `Action`): "That wasn't me",
 * "Cancel this request", "Remove this device" / "Remove device now" and
 * "I will help". Step-ups are per action and target; part 2 sends a factor.
 */

import { plural, t } from "@/i18n";
import { codeIs } from "@/lib/errorCode";
import { HttpError } from "@/lib/tauri/common";
import {
  cancelAccountRequest,
  confirmContactFor,
  removeKeyDevice,
  repudiateEvent,
  type Part2Factor,
} from "@/lib/tauri/security";
import { adminRemoveAuthenticator } from "@/lib/tauri/adminSecurity";
import { deletePasskey } from "@/lib/tauri/passkeys";
import { isTauri } from "@/lib/tauri/common";
import { signalRemoved } from "@/lib/webauthn";
import { StepUpCancelled, withStepUp } from "@/lib/stepUp";
import { refreshMeSecurity } from "@/stores/security";
import { securityErrorText } from "@/components/settings/security/securityErrors";
import type { NoticeAction, RepudiateResult, SecurityEvent, SecurityNotice } from "@/lib/types";
import { eventText } from "./noticeText";

export type FollowUp = "removeDevice" | "removeDeviceNow" | "openSecurity" | "askAdmin";

export type NoticeResult =
  | { kind: "done"; text: string }
  | { kind: "cancelled" }
  | { kind: "tooLate"; text: string; followUp: FollowUp | null };

function repudiationSummary(r: RepudiateResult, ev: SecurityEvent): string {
  const parts: string[] = [];
  if (r.ended_session_ids.length > 0) {
    parts.push(plural(r.ended_session_ids.length, { one: "security.action.summary.sessions.one", other: "security.action.summary.sessions.other" }));
  }
  if (r.revoked_authenticator_ids.length > 0) {
    parts.push(plural(r.revoked_authenticator_ids.length, { one: "security.action.summary.devices.one", other: "security.action.summary.devices.other" }));
  }
  if (r.compromised.includes("password")) parts.push(t("security.action.summary.password"));
  if (r.compromised.includes("totp")) parts.push(t("security.action.summary.totp"));
  if (r.voided_request_ids.length > 0) parts.push(t("security.action.summary.requests"));
  const methods = Array.isArray(ev.details.auth_methods) ? (ev.details.auth_methods as string[]) : [];
  if (methods.includes("oidc")) parts.push(t("security.action.summary.oidc"));
  return parts.length > 0 ? parts.join(" ") : t("security.action.summary.nothing");
}

/**
 * Who may follow up a cancel that lost against completion (C27). Without
 * the event naming the new device (a self-service request), the owner goes
 * to Security settings and a contact asks an admin (Argon review 2 #2).
 */
function followUpFor(notice: SecurityNotice, meId: string, isAdmin: boolean, hasDevice: boolean): FollowUp | null {
  if (notice.subject.user_id === meId) return hasDevice ? "removeDevice" : "openSecurity";
  if (!isAdmin) return "askAdmin";
  return hasDevice ? "removeDeviceNow" : null;
}

export async function runNoticeAction(
  a: NoticeAction,
  ev: SecurityEvent,
  notice: SecurityNotice,
  meId: string,
  opts: { part2?: Part2Factor | null; isAdmin?: boolean } = {},
): Promise<NoticeResult> {
  const label = eventText(ev, notice.subject, meId);
  const part2 = a.via === "part2" ? (opts.part2 ?? null) : null;
  try {
    switch (a.action) {
      case "repudiate": {
        const run = () => repudiateEvent(ev.id, part2);
        const r = a.via === "part2" ? await run() : await withStepUp({ action: "event.repudiate", target: ev.id, targetLabel: label }, run);
        return { kind: "done", text: repudiationSummary(r, ev) };
      }
      case "cancel_request": {
        // No request named: nothing to cancel, no empty-target step-up (Argon 2 #4).
        if (!ev.request_id) return { kind: "cancelled" };
        const id = ev.request_id;
        const run = () => cancelAccountRequest(id, null, part2);
        if (a.via === "part2") await run();
        else await withStepUp({ action: "request.cancel", target: id, targetLabel: notice.subject.display_name }, run);
        return { kind: "done", text: t("security.action.cancelled") };
      }
      case "remove_device":
        return removeDevice(ev, notice, meId);
      case "confirm_contact":
        await confirmContactFor(notice.subject.user_id);
        return { kind: "done", text: t("security.action.helping") };
      default:
        return { kind: "cancelled" };
    }
  } catch (err) {
    if (err instanceof StepUpCancelled) return { kind: "cancelled" };
    if (err instanceof HttpError && codeIs(err.code, "EVENT_ALREADY_REPUDIATED")) {
      return { kind: "done", text: t("security.action.already") };
    }
    if (err instanceof HttpError && codeIs(err.code, "ACCOUNT_REQUEST_COMPLETED")) {
      return {
        kind: "tooLate",
        text: t("security.action.tooLate"),
        followUp: followUpFor(notice, meId, opts.isAdmin ?? false, ev.authenticator !== null),
      };
    }
    throw err;
  } finally {
    // The overview (open request, devices, prompts) changed: no stale state
    // until the next hint (Argon review 2 #4).
    void refreshMeSecurity();
  }
}

/**
 * Owner: remove the own device by its type (Codex review M4): a passkey
 * through `passkey.remove` and LP's delete (with the credential signals), a
 * key device through its own route. Anyone else reading: "Remove device now"
 * (admin). An authenticator type the client predates is refused.
 */
export async function removeDevice(ev: SecurityEvent, notice: SecurityNotice, meId: string): Promise<NoticeResult> {
  const device = ev.authenticator;
  if (!device) return { kind: "cancelled" };
  const type = device.type;
  if (type !== "passkey" && type !== "key_device") throw new Error("AUTHENTICATOR_TYPE_UNKNOWN");
  if (notice.subject.user_id === meId) {
    if (type === "passkey") {
      const removed = await withStepUp({ action: "passkey.remove", target: device.id, targetLabel: device.label }, () =>
        deletePasskey(device.id),
      );
      if (!isTauri()) await signalRemoved(removed);
    } else {
      await withStepUp({ action: "key_device.remove", target: device.id, targetLabel: device.label }, () =>
        removeKeyDevice(device.id),
      );
    }
  } else {
    await withStepUp({ action: "admin.authenticator.remove", target: device.id, targetLabel: device.label }, () =>
      adminRemoveAuthenticator(notice.subject.user_id, device.id),
    );
  }
  void refreshMeSecurity();
  return { kind: "done", text: t("security.action.removed") };
}

/** Notice codes on top of the shared security map (Fable review m5). */
const NOTICE_ERRORS = {
  REPUDIATE_NOT_ALLOWED: "security.action.notAllowed",
  CANCEL_NOT_ALLOWED: "security.action.notAllowed",
} as const;

export function noticeActionError(err: unknown): string {
  return securityErrorText(err, "security.action.failed", NOTICE_ERRORS) ?? "";
}
