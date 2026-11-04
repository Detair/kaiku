/**
 * Notice and push texts for C28 security events. Literal maps (i18n:check
 * sees every key): web `security.notice.<eventType>` / `security.push.<eventType>`
 * with the camelCase event type, which maps to C28's Android names
 * (`security_notice_sign_in`). An unknown type uses `…generic`.
 */

import { t, type MessageKey } from "@/i18n";
import { totalKey } from "@/lib/totalKey";
import type { SecurityEvent, SecurityEventType, SecurityNotice } from "@/lib/types";

export const NOTICE_KEYS: Record<SecurityEventType, MessageKey> = {
  sign_in: "security.notice.signIn",
  sign_in_not_finished: "security.notice.signInNotFinished",
  second_factor_failed: "security.notice.secondFactorFailed",
  guessing: "security.notice.guessing",
  approval_reported: "security.notice.approvalReported",
  authenticator_added: "security.notice.authenticatorAdded",
  authenticator_removed: "security.notice.authenticatorRemoved",
  key_invalidated: "security.notice.keyInvalidated",
  recovery_codes_created: "security.notice.recoveryCodesCreated",
  password_set: "security.notice.passwordSet",
  password_changed: "security.notice.passwordChanged",
  password_removed: "security.notice.passwordRemoved",
  password_throttled: "security.notice.passwordThrottled",
  password_compromised: "security.notice.passwordCompromised",
  totp_throttled: "security.notice.totpThrottled",
  totp_compromised: "security.notice.totpCompromised",
  step_up_action: "security.notice.stepUpAction",
  repudiated: "security.notice.repudiated",
  request_started: "security.notice.requestStarted",
  request_completed: "security.notice.requestCompleted",
  request_cancelled: "security.notice.requestCancelled",
  request_withdrawn: "security.notice.requestWithdrawn",
  request_ready_soon: "security.notice.requestReadySoon",
  request_ready: "security.notice.requestReady",
  request_expired: "security.notice.requestExpired",
  request_void: "security.notice.requestVoid",
  admin_setup: "security.notice.adminSetup",
  admin_role_granted: "security.notice.adminRoleGranted",
  admin_role_removed: "security.notice.adminRoleRemoved",
  recovery_contact_named: "security.notice.recoveryContactNamed",
  recovery_contact_confirmed: "security.notice.recoveryContactConfirmed",
  recovery_contact_changed: "security.notice.recoveryContactChanged",
  session_ended_reuse: "security.notice.sessionEndedReuse",
  session_ended_limit: "security.notice.sessionEndedLimit",
  bootstrap_reminder: "security.notice.bootstrapReminder",
  sessions_ended: "security.notice.sessionsEnded",
  factor_restored: "security.notice.factorRestored",
  throttle_reset: "security.notice.throttleReset",
  may_approve_changed: "security.notice.mayApproveChanged",
  unknown: "security.notice.generic",
};

export const PUSH_KEYS: Record<SecurityEventType, MessageKey> = {
  sign_in: "security.push.signIn",
  sign_in_not_finished: "security.push.signInNotFinished",
  second_factor_failed: "security.push.secondFactorFailed",
  guessing: "security.push.guessing",
  approval_reported: "security.push.approvalReported",
  authenticator_added: "security.push.authenticatorAdded",
  authenticator_removed: "security.push.authenticatorRemoved",
  key_invalidated: "security.push.keyInvalidated",
  recovery_codes_created: "security.push.recoveryCodesCreated",
  password_set: "security.push.passwordSet",
  password_changed: "security.push.passwordChanged",
  password_removed: "security.push.passwordRemoved",
  password_throttled: "security.push.passwordThrottled",
  password_compromised: "security.push.passwordCompromised",
  totp_throttled: "security.push.totpThrottled",
  totp_compromised: "security.push.totpCompromised",
  step_up_action: "security.push.stepUpAction",
  repudiated: "security.push.repudiated",
  request_started: "security.push.requestStarted",
  request_completed: "security.push.requestCompleted",
  request_cancelled: "security.push.requestCancelled",
  request_withdrawn: "security.push.requestWithdrawn",
  request_ready_soon: "security.push.requestReadySoon",
  request_ready: "security.push.requestReady",
  request_expired: "security.push.requestExpired",
  request_void: "security.push.requestVoid",
  admin_setup: "security.push.adminSetup",
  admin_role_granted: "security.push.adminRoleGranted",
  admin_role_removed: "security.push.adminRoleRemoved",
  recovery_contact_named: "security.push.recoveryContactNamed",
  recovery_contact_confirmed: "security.push.recoveryContactConfirmed",
  recovery_contact_changed: "security.push.recoveryContactChanged",
  session_ended_reuse: "security.push.sessionEndedReuse",
  session_ended_limit: "security.push.sessionEndedLimit",
  bootstrap_reminder: "security.push.bootstrapReminder",
  sessions_ended: "security.push.sessionsEnded",
  factor_restored: "security.push.factorRestored",
  throttle_reset: "security.push.throttleReset",
  may_approve_changed: "security.push.mayApproveChanged",
  unknown: "security.push.generic",
};

interface Subject {
  user_id: string;
  display_name: string;
}

/** "your account" for the reader's own notices, else "Oma's account". */
function subjectText(subject: Subject, meId: string): string {
  return subject.user_id === meId
    ? t("security.notice.yourAccount")
    : t("security.notice.accountOf", { name: subject.display_name });
}

function params(ev: SecurityEvent | undefined, subject: Subject, meId: string, count: number) {
  return {
    subject: subjectText(subject, meId),
    actor: ev?.actor?.username ?? t("security.notice.someone"),
    device: ev?.authenticator?.label || ev?.client?.label || t("security.notice.aDevice"),
    count,
  };
}

/** The notice sentence: placeholders filled, "(N times)" when coalesced. */
export function noticeText(n: SecurityNotice, meId: string): string {
  const text = t(noticeKey(n.event_type), params(n.events[0], n.subject, meId, n.count));
  return n.count > 1
    ? t("security.notice.repeated", { text, count: n.count })
    : t("security.notice.single", { text });
}

/** One event of the security log, worded like its notice. */
export function eventText(ev: SecurityEvent, subject: Subject, meId: string): string {
  return t("security.notice.single", { text: t(noticeKey(ev.event_type), params(ev, subject, meId, 1)) });
}

/** The OS notification text for a new security message. */
export function pushText(n: SecurityNotice, meId: string): string {
  return t(totalKey(PUSH_KEYS, n.event_type, PUSH_KEYS.unknown), params(n.events[0], n.subject, meId, n.count));
}

/** The notice key of an event type; a type the client predates is generic. */
export function noticeKey(type: unknown): MessageKey {
  return totalKey(NOTICE_KEYS, type, NOTICE_KEYS.unknown);
}
