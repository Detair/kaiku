/**
 * Passkeys (C29): sign-in, step-up, registration and management.
 *
 * Browser: the JSON routes with `navigator.credentials`. Desktop (Tauri):
 * sign-in and step-up run in the system browser through the Rust commands
 * `passkey_sign_in` / `passkey_step_up` (tokens stay in Rust); the desktop
 * app never registers a passkey (B.1).
 */

import { invoke } from "@tauri-apps/api/core";

import {
  browserState,
  getAuthContext,
  httpRequest,
  HttpError,
  isTauri,
  scheduleTokenRefresh,
  setSessionRestoreBlocked,
  type AuthResult,
} from "./common";
import type { User } from "../types";
import {
  credentialToJSON,
  creationOptionsFromJSON,
  requestOptionsFromJSON,
  webAuthnErrorCode,
  type PasskeyRemovedResponse,
} from "../webauthn";

export type { PasskeyRemovedResponse } from "../webauthn";

/** The step-up registry (C27 common rules); passkeys consume two of them. */
export type StepUpAction =
  | "passkey.add"
  | "passkey.remove"
  | "key_device.remove"
  | "enrollment.issue"
  | "password.set"
  | "password.remove"
  | "totp.set"
  | "totp.remove"
  | "recovery_codes.create"
  | "identity.link"
  | "identity.unlink"
  | "sessions.end"
  | "recovery_contact.set"
  | "request.cancel"
  | "event.repudiate"
  | "account.delete"
  | "admin.elevate"
  | "admin.enrollment.issue"
  | "admin.authenticator.remove"
  | "admin.may_approve"
  | "admin.factor.set_state"
  | "admin.recovery_contact.set";

export interface StepUpGrant {
  action: StepUpAction;
  /** Canonical target: lowercase UUID, `others` or `""`. */
  target: string;
  via: "passkey" | "device";
  expires_at: string;
}

/** C29 `Passkey`. */
export interface Passkey {
  id: string;
  label: string;
  state: "pending" | "active" | "revoked" | "unknown";
  created_at: string;
  last_used_at: string | null;
  ready_at: string | null;
  backup_eligible: boolean;
  backup_state: boolean;
  transports: string[];
  aaguid: string | null;
}

export interface PasskeyListResponse {
  rp_id: string;
  user_handle: string;
  passkeys: Passkey[];
}

interface CeremonyStart {
  ceremony_id: string;
  options: { publicKey: Record<string, unknown> };
}

/**
 * A passkey failure: a server code (`PASSKEY_UNKNOWN`, `PASSKEY_PENDING`, …)
 * or a ceremony code (`PASSKEY_CANCELLED`, `ABORTED`, …). `credentialId` and
 * `rpId` let the caller signal an unknown credential; `readyAt` comes with
 * `PASSKEY_PENDING`.
 */
export class PasskeyError extends Error {
  constructor(
    public readonly code: string,
    public readonly credentialId?: string,
    public readonly rpId?: string,
    public readonly readyAt?: string,
  ) {
    super(code);
    this.name = "PasskeyError";
  }
}

function attemptId(): string {
  return crypto.randomUUID();
}

/** Run a Rust passkey command; an abort cancels it in Rust. */
async function invokeCancellable<T>(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
): Promise<T> {
  const id = attemptId();
  const onAbort = () => {
    void invoke("passkey_cancel", { attemptId: id }).catch(() => undefined);
  };
  if (signal.aborted) throw new PasskeyError("ABORTED");
  signal.addEventListener("abort", onAbort, { once: true });
  try {
    return await invoke<T>(command, { ...args, attemptId: id });
  } catch (err) {
    throw new PasskeyError(signal.aborted ? "ABORTED" : String(err));
  } finally {
    signal.removeEventListener("abort", onAbort);
  }
}

async function getCredential(
  options: CeremonyStart["options"],
  signal: AbortSignal,
  mediation?: CredentialMediationRequirement,
): Promise<PublicKeyCredential> {
  try {
    const cred = await navigator.credentials.get({
      publicKey: requestOptionsFromJSON(options.publicKey),
      signal,
      ...(mediation ? { mediation } : {}),
    });
    if (!cred) throw new PasskeyError("PASSKEY_FAILED");
    return cred as PublicKeyCredential;
  } catch (err) {
    if (err instanceof PasskeyError) throw err;
    throw new PasskeyError(webAuthnErrorCode(err));
  }
}

/** Map a server refusal to a `PasskeyError` carrying what the UI needs. */
function serverError(err: unknown, cred?: PublicKeyCredential, rpId?: string): PasskeyError {
  if (err instanceof HttpError && err.code) {
    const readyAt =
      typeof err.details?.ready_at === "string" ? (err.details.ready_at as string) : undefined;
    return new PasskeyError(err.code.toUpperCase(), cred?.id, rpId, readyAt);
  }
  throw err;
}

/**
 * Sign in with a passkey. Browser: a discoverable ceremony
 * (`mediation: "conditional"` for autofill). Desktop: the system browser.
 */
export async function passkeySignIn(
  serverUrl: string,
  o: { mediation: "conditional" | "optional"; signal: AbortSignal; username?: string },
): Promise<AuthResult> {
  if (isTauri()) {
    return invokeCancellable<AuthResult>("passkey_sign_in", { serverUrl }, o.signal);
  }
  // The base URL for the calls below; the restore block and the stored
  // server are only touched after a completed sign-in (autofill runs on
  // every login page and must not re-enable a logged-out session).
  browserState.serverUrl = serverUrl;
  const start = await httpRequest<CeremonyStart>("POST", "/auth/passkeys/login/start", {});
  const rpId = start.options.publicKey.rpId as string | undefined;
  const cred = await getCredential(start.options, o.signal, o.mediation);
  let response: {
    access_token: string;
    expires_in: number;
    setup_required: boolean;
    password_change_required?: boolean;
  };
  try {
    response = await httpRequest("POST", "/auth/passkeys/login/finish", {
      ceremony_id: start.ceremony_id,
      credential: credentialToJSON(cred),
      username: o.username?.trim() || null,
    });
  } catch (err) {
    throw serverError(err, cred, rpId);
  }
  localStorage.setItem("serverUrl", serverUrl);
  setSessionRestoreBlocked(false);
  browserState.accessToken = response.access_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;
  scheduleTokenRefresh();
  const user = await httpRequest<User>("GET", "/auth/me");
  return {
    user,
    setup_required: response.setup_required,
    password_change_required: response.password_change_required ?? false,
  };
}

/**
 * Confirm `action` on `target` with a passkey (F7): a single-use grant for
 * this session. Desktop runs it in the system browser; the request always
 * goes to the session's own server.
 */
export async function stepUpWithPasskey(
  action: StepUpAction,
  target: string | null,
  signal: AbortSignal,
): Promise<StepUpGrant> {
  if (isTauri()) {
    const { baseUrl } = await getAuthContext();
    return invokeCancellable<StepUpGrant>(
      "passkey_step_up",
      { serverUrl: baseUrl, action, target: target ?? "" },
      signal,
    );
  }
  const start = await httpRequest<CeremonyStart>("POST", "/auth/passkeys/step-up/start", {
    action,
    target: target ?? "",
  });
  const cred = await getCredential(start.options, signal);
  try {
    return await httpRequest<StepUpGrant>("POST", "/auth/passkeys/step-up/finish", {
      ceremony_id: start.ceremony_id,
      credential: credentialToJSON(cred),
    });
  } catch (err) {
    throw serverError(err, cred, start.options.publicKey.rpId as string | undefined);
  }
}

/** Add a passkey (browser only; needs a `passkey.add` grant first). */
export async function registerPasskey(o: { label: string; signal: AbortSignal }): Promise<Passkey> {
  if (isTauri()) throw new PasskeyError("PASSKEY_NOT_SUPPORTED");
  const start = await httpRequest<CeremonyStart>("POST", "/auth/passkeys/register/start", {
    label: o.label,
  });
  let cred: PublicKeyCredential;
  try {
    const created = await navigator.credentials.create({
      publicKey: creationOptionsFromJSON(start.options.publicKey),
      signal: o.signal,
    });
    if (!created) throw new PasskeyError("PASSKEY_FAILED");
    cred = created as PublicKeyCredential;
  } catch (err) {
    if (err instanceof PasskeyError) throw err;
    throw new PasskeyError(webAuthnErrorCode(err));
  }
  try {
    return await httpRequest<Passkey>("POST", "/auth/passkeys/register/finish", {
      ceremony_id: start.ceremony_id,
      credential: credentialToJSON(cred),
    });
  } catch (err) {
    throw serverError(err);
  }
}

export function listPasskeys(): Promise<PasskeyListResponse> {
  return httpRequest<PasskeyListResponse>("GET", "/auth/passkeys");
}

export function renamePasskey(id: string, label: string): Promise<Passkey> {
  return httpRequest<Passkey>("PATCH", `/auth/passkeys/${encodeURIComponent(id)}`, { label });
}

export function deletePasskey(id: string): Promise<PasskeyRemovedResponse> {
  return httpRequest<PasskeyRemovedResponse>("DELETE", `/auth/passkeys/${encodeURIComponent(id)}`);
}
