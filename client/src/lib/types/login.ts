/**
 * v1.0 login wire types (contracts C26, C27, C28) as the desktop and web
 * clients use them. Field names are the wire names (snake_case). Clients
 * ignore unknown fields and map unknown enum values to `"unknown"` (C5); the
 * `decode*` helpers below do that mapping for every enum the UI branches on.
 */

// ---------------------------------------------------------------------------
// Enum helper
// ---------------------------------------------------------------------------

/** `value` when it is one of `allowed`, else `fallback` (C5 unknown tolerance). */
export function oneOf<T extends string, F extends string>(
  value: unknown,
  allowed: readonly T[],
  fallback: F,
): T | F {
  return typeof value === "string" && (allowed as readonly string[]).includes(value)
    ? (value as T)
    : fallback;
}

// ---------------------------------------------------------------------------
// Sessions and assurance (C27 "Sessions and assurance")
// ---------------------------------------------------------------------------

export type Assurance = "full" | "probation";
export const CLIENT_CLASSES = ["web", "desktop", "android"] as const;
export type ClientClass = (typeof CLIENT_CLASSES)[number] | "unknown";

/** `GET /auth/me` `session` (named `AuthSession`: `SessionInfo` is the session-list row). */
export interface AuthSession {
  id: string;
  assurance: Assurance;
  auth_methods: string[];
  auth_time: string;
  client_class: ClientClass;
  absolute_expires_at: string;
  key_device_id: string | null;
}

/** A missing or unknown assurance counts as probation (C27). */
export function decodeAuthSession(raw: unknown): AuthSession | null {
  if (!raw || typeof raw !== "object") return null;
  const s = raw as Record<string, unknown>;
  if (typeof s.id !== "string") return null;
  return {
    id: s.id,
    assurance: s.assurance === "full" ? "full" : "probation",
    auth_methods: Array.isArray(s.auth_methods) ? (s.auth_methods as string[]) : [],
    auth_time: typeof s.auth_time === "string" ? s.auth_time : "",
    client_class: oneOf(s.client_class, CLIENT_CLASSES, "unknown"),
    absolute_expires_at: typeof s.absolute_expires_at === "string" ? s.absolute_expires_at : "",
    key_device_id: typeof s.key_device_id === "string" ? s.key_device_id : null,
  };
}

// ---------------------------------------------------------------------------
// Login attempts (C27, SM1)
// ---------------------------------------------------------------------------

export const LOGIN_ATTEMPT_STATES = [
  "open",
  "claimed",
  "approved",
  "consumed",
  "denied",
  "reported",
  "cancelled",
  "expired",
] as const;
export type LoginAttemptState = (typeof LOGIN_ATTEMPT_STATES)[number] | "unknown";
export const LOGIN_ATTEMPT_KINDS = ["qr", "password", "oidc"] as const;
export type LoginAttemptKind = (typeof LOGIN_ATTEMPT_KINDS)[number] | "unknown";

export interface LoginAttempt {
  attempt_id: string;
  kind: LoginAttemptKind;
  state: LoginAttemptState;
  expires_at: string;
  /** Present only in a response that minted a new token (create, rotation). */
  qr: { payload: string; expires_at: string } | null;
  totp_available: boolean;
}

export interface LoginStep {
  status: "confirmation_required" | "totp_required";
  attempt: LoginAttempt;
}

export function decodeLoginAttempt(raw: unknown): LoginAttempt {
  const a = (raw ?? {}) as Record<string, unknown>;
  const qr = a.qr as { payload?: unknown; expires_at?: unknown } | null | undefined;
  return {
    attempt_id: String(a.attempt_id ?? ""),
    kind: oneOf(a.kind, LOGIN_ATTEMPT_KINDS, "unknown"),
    state: oneOf(a.state, LOGIN_ATTEMPT_STATES, "unknown"),
    expires_at: String(a.expires_at ?? ""),
    qr:
      qr && typeof qr.payload === "string"
        ? { payload: qr.payload, expires_at: String(qr.expires_at ?? "") }
        : null,
    totp_available: a.totp_available === true,
  };
}

export function decodeLoginStep(raw: unknown): LoginStep {
  const s = (raw ?? {}) as Record<string, unknown>;
  return {
    // Anything but totp_required waits for a phone (the safer reading).
    status: s.status === "totp_required" ? "totp_required" : "confirmation_required",
    attempt: decodeLoginAttempt(s.attempt),
  };
}

// ---------------------------------------------------------------------------
// Step-up (C26–C29 common rules: the action table)
// ---------------------------------------------------------------------------

export const STEP_UP_ACTIONS = [
  "passkey.add",
  "passkey.remove",
  "key_device.remove",
  "enrollment.issue",
  "password.set",
  "password.remove",
  "totp.set",
  "totp.remove",
  "recovery_codes.create",
  "identity.link",
  "identity.unlink",
  "sessions.end",
  "recovery_contact.set",
  "request.cancel",
  "event.repudiate",
  "account.delete",
  "admin.elevate",
  "admin.enrollment.issue",
  "admin.authenticator.remove",
  "admin.may_approve",
  "admin.factor.set_state",
  "admin.recovery_contact.set",
] as const;
export type StepUpAction = (typeof STEP_UP_ACTIONS)[number];

export interface StepUpGrant {
  action: StepUpAction;
  /** Canonical target: lowercase UUID, `others` or `""`. */
  target: string;
  via: "passkey" | "device";
  expires_at: string;
}

// ---------------------------------------------------------------------------
// Key devices and enrolments (C26)
// ---------------------------------------------------------------------------

export const KEY_DEVICE_STATES = ["pending", "active", "revoked"] as const;
export type KeyDeviceState = (typeof KEY_DEVICE_STATES)[number] | "unknown";

export interface KeyDevice {
  id: string;
  device_id: string;
  label: string;
  platform: string;
  state: KeyDeviceState;
  enrolled_via: string;
  uv_class: string;
  created_at: string;
  activated_at: string | null;
  ready_at: string | null;
  last_used_at: string | null;
  revoked_at: string | null;
  revoke_reason: string | null;
  is_current: boolean;
}

export function decodeKeyDevice(raw: unknown): KeyDevice {
  const d = (raw ?? {}) as KeyDevice;
  return { ...d, state: oneOf(d.state, KEY_DEVICE_STATES, "unknown"), is_current: d.is_current === true };
}

export interface EnrollmentIssued {
  enrollment_id: string;
  setup_token: string;
  setup_url: string;
  expires_at: string;
}

export const ENROLLMENT_STATES = ["issued", "claimed", "done", "cancelled", "expired", "failed"] as const;
export type EnrollmentState = (typeof ENROLLMENT_STATES)[number] | "unknown";

export interface EnrollmentSubject {
  user_id: string;
  username: string;
  display_name: string;
}

export interface Enrollment {
  id: string;
  variant: string;
  state: EnrollmentState;
  subject: EnrollmentSubject;
  claimed: { authenticator_type: "key_device" | "passkey" | "unknown"; label: string; key_hash: string } | null;
  tries_left: number;
  code_expires_at: string | null;
  expires_at: string;
  request_id: string | null;
}

export function decodeEnrollment(raw: unknown): Enrollment {
  const e = (raw ?? {}) as Enrollment;
  return {
    ...e,
    state: oneOf(e.state, ENROLLMENT_STATES, "unknown"),
    claimed: e.claimed
      ? {
          ...e.claimed,
          authenticator_type: oneOf(e.claimed.authenticator_type, ["key_device", "passkey"] as const, "unknown"),
        }
      : null,
  };
}

/** `POST /auth/enrollments/passkey/start` (C29, setup link in a browser). */
export interface PasskeyClaimStart {
  ceremony_id: string;
  options: unknown;
  server_name: string;
  subject: { username: string; display_name: string };
}

/** `POST /auth/enrollments/passkey/finish` → the code goes to the issuer. */
export interface PasskeyClaimed {
  enrollment_id: string;
  state: "claimed";
  code: string;
  code_expires_at: string;
}

export const AUTHENTICATOR_STATES = ["active", "pending"] as const;

/** `POST /auth/enrollments/status` long-poll answer. */
export interface ClaimStatus {
  state: EnrollmentState;
  authenticator_state: (typeof AUTHENTICATOR_STATES)[number] | "unknown" | null;
  ready_at: string | null;
}

export function decodeClaimStatus(raw: unknown): ClaimStatus {
  const s = (raw ?? {}) as Record<string, unknown>;
  return {
    state: oneOf(s.state, ENROLLMENT_STATES, "unknown"),
    authenticator_state:
      s.authenticator_state == null ? null : oneOf(s.authenticator_state, AUTHENTICATOR_STATES, "unknown"),
    ready_at: typeof s.ready_at === "string" ? s.ready_at : null,
  };
}

// ---------------------------------------------------------------------------
// Account requests (C27, SM3)
// ---------------------------------------------------------------------------

export const ACCOUNT_REQUEST_STATES = [
  "waiting",
  "ready",
  "completed",
  "cancelled",
  "withdrawn",
  "expired",
  "void",
] as const;
export type AccountRequestState = (typeof ACCOUNT_REQUEST_STATES)[number] | "unknown";
export const ACCOUNT_REQUEST_PURPOSES = ["recovery", "bootstrap", "admin_setup", "admin_recovery"] as const;
export type AccountRequestPurpose = (typeof ACCOUNT_REQUEST_PURPOSES)[number] | "unknown";

export interface AccountRequest {
  id: string;
  kind: "self" | "admin" | "unknown";
  purpose: AccountRequestPurpose;
  state: AccountRequestState;
  created_at: string;
  ready_at: string;
  /** Finish deadline once `ready`. */
  expires_at: string | null;
  factors: string[];
  lost_key_devices: number;
  staged: {
    authenticator: { type: string; label: string } | null;
    password: boolean;
    totp: boolean;
    recovery_contact: string | null;
  };
}

export interface AccountRequestSummary {
  id: string;
  subject: EnrollmentSubject;
  kind: "self" | "admin" | "unknown";
  purpose: AccountRequestPurpose;
  state: AccountRequestState;
  created_at: string;
  ready_at: string;
  started_by: { user_id: string; username: string } | null;
  lost_devices: { id: string; label: string }[];
  new_authenticator: { type: string; label: string } | null;
  cancel: { via: "step_up" | "part2" | "unknown" } | null;
}

export function decodeAccountRequest(raw: unknown): AccountRequest {
  const r = (raw ?? {}) as AccountRequest;
  return {
    ...r,
    kind: oneOf(r.kind, ["self", "admin"] as const, "unknown"),
    purpose: oneOf(r.purpose, ACCOUNT_REQUEST_PURPOSES, "unknown"),
    state: oneOf(r.state, ACCOUNT_REQUEST_STATES, "unknown"),
    factors: Array.isArray(r.factors) ? r.factors : [],
    lost_key_devices: typeof r.lost_key_devices === "number" ? r.lost_key_devices : 0,
    staged: r.staged ?? { authenticator: null, password: false, totp: false, recovery_contact: null },
  };
}

export function decodeAccountRequestSummary(raw: unknown): AccountRequestSummary {
  const r = (raw ?? {}) as AccountRequestSummary;
  return {
    ...r,
    kind: oneOf(r.kind, ["self", "admin"] as const, "unknown"),
    purpose: oneOf(r.purpose, ACCOUNT_REQUEST_PURPOSES, "unknown"),
    state: oneOf(r.state, ACCOUNT_REQUEST_STATES, "unknown"),
    lost_devices: Array.isArray(r.lost_devices) ? r.lost_devices : [],
    new_authenticator: r.new_authenticator ?? null,
    started_by: r.started_by ?? null,
    cancel: r.cancel ? { via: oneOf(r.cancel.via, ["step_up", "part2"] as const, "unknown") } : null,
  };
}

/** Recovery start (F6). `second` is the second factor next to the recovery code. */
export interface RecoveryStart {
  username: string;
  purpose: "recovery";
  recovery_code: string;
  second:
    | { type: "password"; password: string }
    | { type: "totp"; code: string }
    | { type: "oidc"; oidc_code: string };
  lost: "none" | "all_key_devices";
}

/** Bootstrap start ("Secure my account", F8). */
export interface BootstrapStart {
  username: string;
  purpose: "bootstrap";
  first: { type: "password"; password: string } | { type: "oidc"; oidc_code: string };
  totp_code: string;
}

export interface TotpSetup {
  secret: string;
  otpauth_uri: string;
}

// ---------------------------------------------------------------------------
// Registration tickets (C27, F8)
// ---------------------------------------------------------------------------

/** `POST /auth/register` (never a session). */
export interface RegistrationStarted {
  user_id: string;
  username: string;
  ticket_expires_at: string;
}

export interface RegistrationStatus {
  expires_at: string;
  bound: boolean;
}

// ---------------------------------------------------------------------------
// Security events, notices, recovery contact (C28)
// ---------------------------------------------------------------------------

export const SECURITY_EVENT_TYPES = [
  "sign_in",
  "sign_in_not_finished",
  "second_factor_failed",
  "guessing",
  "approval_reported",
  "authenticator_added",
  "authenticator_removed",
  "key_invalidated",
  "recovery_codes_created",
  "password_set",
  "password_changed",
  "password_removed",
  "password_throttled",
  "password_compromised",
  "totp_throttled",
  "totp_compromised",
  "step_up_action",
  "repudiated",
  "request_started",
  "request_completed",
  "request_cancelled",
  "request_withdrawn",
  "request_ready_soon",
  "request_ready",
  "request_expired",
  "request_void",
  "admin_setup",
  "admin_role_granted",
  "admin_role_removed",
  "recovery_contact_named",
  "recovery_contact_confirmed",
  "recovery_contact_changed",
  "session_ended_reuse",
  "session_ended_limit",
  "bootstrap_reminder",
] as const;
export type SecurityEventType = (typeof SECURITY_EVENT_TYPES)[number] | "unknown";

export const NOTICE_ACTION_KINDS = ["repudiate", "cancel_request", "remove_device", "confirm_contact"] as const;

export interface NoticeAction {
  action: (typeof NOTICE_ACTION_KINDS)[number] | "unknown";
  via: "step_up" | "part2" | null;
  warning: "voids_own_request" | "voids_own_bootstrap" | null;
}

export interface SecurityEvent {
  id: string;
  event_type: SecurityEventType;
  severity: "high" | "normal";
  created_at: string;
  subject_id: string;
  actor: { kind: string; user_id: string | null; username: string | null } | null;
  client: { class: string; label: string } | null;
  place: string | null;
  ip: string | null;
  session_id: string | null;
  authenticator: { id: string; type: string; label: string } | null;
  request_id: string | null;
  details: Record<string, unknown>;
  actions: NoticeAction[];
  repudiated_at: string | null;
}

export function decodeSecurityEvent(raw: unknown): SecurityEvent {
  const e = (raw ?? {}) as SecurityEvent;
  return {
    ...e,
    event_type: oneOf(e.event_type, SECURITY_EVENT_TYPES, "unknown"),
    severity: e.severity === "high" ? "high" : "normal",
    actor: e.actor ?? null,
    client: e.client ?? null,
    place: e.place ?? null,
    ip: e.ip ?? null,
    authenticator: e.authenticator ?? null,
    request_id: e.request_id ?? null,
    details: e.details ?? {},
    repudiated_at: e.repudiated_at ?? null,
    actions: (Array.isArray(e.actions) ? e.actions : []).map((a) => ({
      action: oneOf(a?.action, NOTICE_ACTION_KINDS, "unknown"),
      via: a?.via === "step_up" || a?.via === "part2" ? a.via : null,
      warning: a?.warning === "voids_own_request" || a?.warning === "voids_own_bootstrap" ? a.warning : null,
    })),
  };
}

export interface SecurityNotice {
  notice_id: string;
  subject: EnrollmentSubject;
  event_type: SecurityEventType;
  severity: "high" | "normal";
  count: number;
  first_at: string;
  last_at: string;
  /** Newest first, at most 50. */
  events: SecurityEvent[];
  events_truncated: boolean;
}

export function decodeSecurityNotice(raw: unknown): SecurityNotice | null {
  if (!raw || typeof raw !== "object") return null;
  const n = raw as SecurityNotice;
  return {
    ...n,
    event_type: oneOf(n.event_type, SECURITY_EVENT_TYPES, "unknown"),
    severity: n.severity === "high" ? "high" : "normal",
    count: typeof n.count === "number" ? n.count : 1,
    events: (Array.isArray(n.events) ? n.events : []).map(decodeSecurityEvent),
    events_truncated: n.events_truncated === true,
  };
}

/** `POST /api/me/security-events/{id}/repudiate` answer. */
export interface RepudiateResult {
  event: SecurityEvent;
  ended_session_ids: string[];
  revoked_authenticator_ids: string[];
  compromised: string[];
  voided_request_ids: string[];
}

export interface RecoveryContact {
  user_id: string;
  username: string;
  display_name: string;
  named_at: string;
  confirmed_at: string | null;
  counts_from: string | null;
}

/** `GET /api/me/recovery-contact-for` item: someone who chose the caller. */
export interface ContactForEntry {
  user_id: string;
  username: string;
  display_name: string;
  named_at: string;
  confirmed_at: string | null;
}

export const SECURITY_PROMPTS = [
  "secure_my_account",
  "set_up_phone",
  "add_passkey",
  "sign_in_with_passkey",
  "name_recovery_contact",
  "renew_totp",
  "renew_recovery_codes",
  "open_request",
] as const;
export type SecurityPrompt = (typeof SECURITY_PROMPTS)[number];

export const CREDENTIAL_STATES = ["active", "throttled", "compromised"] as const;
export type CredentialState = (typeof CREDENTIAL_STATES)[number] | "unknown";

/** `GET /api/me/security` (C27 "Security overview and migration prompts"). */
export interface MeSecurity {
  session: { assurance: Assurance; auth_methods: string[] };
  password: { state: CredentialState } | null;
  totp: { state: CredentialState } | null;
  recovery_codes_left: number;
  key_devices: number;
  passkeys: number;
  may_approve: boolean;
  bootstrap_open: boolean;
  recovery_contact: RecoveryContact | null;
  open_request: AccountRequestSummary | null;
  /** Unknown prompts are dropped (C27). */
  prompts: SecurityPrompt[];
}

export function decodeMeSecurity(raw: unknown): MeSecurity {
  const s = (raw ?? {}) as Record<string, unknown>;
  const cred = (v: unknown): { state: CredentialState } | null =>
    v && typeof v === "object"
      ? { state: oneOf((v as { state?: unknown }).state, CREDENTIAL_STATES, "unknown") }
      : null;
  const session = (s.session ?? {}) as { assurance?: unknown; auth_methods?: unknown };
  return {
    session: {
      assurance: session.assurance === "full" ? "full" : "probation",
      auth_methods: Array.isArray(session.auth_methods) ? (session.auth_methods as string[]) : [],
    },
    password: cred(s.password),
    totp: cred(s.totp),
    recovery_codes_left: typeof s.recovery_codes_left === "number" ? s.recovery_codes_left : 0,
    key_devices: typeof s.key_devices === "number" ? s.key_devices : 0,
    passkeys: typeof s.passkeys === "number" ? s.passkeys : 0,
    may_approve: s.may_approve === true,
    bootstrap_open: s.bootstrap_open === true,
    recovery_contact: (s.recovery_contact as RecoveryContact | null) ?? null,
    open_request: s.open_request ? decodeAccountRequestSummary(s.open_request) : null,
    prompts: (Array.isArray(s.prompts) ? s.prompts : []).filter((p): p is SecurityPrompt =>
      (SECURITY_PROMPTS as readonly string[]).includes(p as string),
    ),
  };
}

/** `{items, has_more, next_cursor}` (C28 security log). */
export interface CursorPaginatedResponse<T> {
  items: T[];
  has_more: boolean;
  next_cursor: string | null;
}
