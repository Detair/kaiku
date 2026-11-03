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
import type { StepUpAction, StepUpGrant } from "../types/login";
import {
  credentialToJSON,
  creationOptionsFromJSON,
  isWebAuthnAvailable,
  requestOptionsFromJSON,
  signalUnknown,
  webAuthnErrorCode,
  type PasskeyRemovedResponse,
} from "../webauthn";

export type { PasskeyRemovedResponse } from "../webauthn";

/**
 * The step-up action table and grant (C26–C29 common rules), defined once
 * in `lib/types/login.ts` next to `STEP_UP_ACTIONS` (the server's
 * `StepUpAction::ALL`); passkeys consume two of them.
 */
export type { StepUpAction, StepUpGrant };

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
  run: (attemptId: string) => Promise<T>,
  signal: AbortSignal,
): Promise<T> {
  const id = attemptId();
  const onAbort = () => {
    void invoke("passkey_cancel", { attemptId: id }).catch(() => undefined);
  };
  if (signal.aborted) throw new PasskeyError("ABORTED");
  signal.addEventListener("abort", onAbort, { once: true });
  try {
    return await run(id);
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
  if (!isWebAuthnAvailable()) throw new PasskeyError("PASSKEY_NOT_SUPPORTED");
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
    const code = err.code.toUpperCase();
    const readyAt =
      typeof err.details?.ready_at === "string" ? (err.details.ready_at as string) : undefined;
    // C29: an unknown credential is signalled (never a pending one).
    if (code === "PASSKEY_UNKNOWN" && rpId && cred?.id) void signalUnknown(rpId, cred.id);
    return new PasskeyError(code, cred?.id, rpId, readyAt);
  }
  throw err;
}

/**
 * Sign in with a passkey. Browser: a discoverable ceremony
 * (`mediation: "conditional"` for autofill). Desktop: the system browser.
 */
/** A user name now, or read when the credential was chosen. */
export type UsernameHint = string | (() => string | undefined);

interface FinishedSignIn {
  access_token: string;
  expires_in: number;
  setup_required: boolean;
  password_change_required?: boolean;
}

/**
 * POST to `serverUrl` directly (browser): the client's current server and
 * tokens stay untouched until a sign-in completed. Errors are `HttpError`s
 * with the parsed body as `details`.
 */
async function passkeyPost<T>(
  serverUrl: string,
  path: string,
  body: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const base = serverUrl.replace(/\/+$/, "");
  const response = await fetch(`${base}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    credentials: "include",
    signal,
  });
  const text = await response.text();
  const parsed = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  if (!response.ok) {
    const code = typeof parsed.error === "string" ? parsed.error : undefined;
    const message = typeof parsed.message === "string" ? parsed.message : `HTTP ${response.status}`;
    throw new HttpError(response.status, message, code, parsed);
  }
  return parsed as T;
}

/** `GET /auth/me` on `serverUrl` with a session not installed yet. */
async function fetchMe(serverUrl: string, accessToken: string): Promise<User> {
  const response = await fetch(`${serverUrl.replace(/\/+$/, "")}/auth/me`, {
    headers: { Authorization: `Bearer ${accessToken}` },
    credentials: "omit",
  });
  const text = await response.text();
  const parsed = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  if (!response.ok) {
    const code = typeof parsed.error === "string" ? parsed.error : undefined;
    const message = typeof parsed.message === "string" ? parsed.message : `HTTP ${response.status}`;
    throw new HttpError(response.status, message, code, parsed);
  }
  return parsed as unknown as User;
}

/** End a session minted by a stale attempt (best effort, its own bearer only). */
async function endStaleSession(serverUrl: string, accessToken: string): Promise<void> {
  try {
    await fetch(`${serverUrl.replace(/\/+$/, "")}/auth/logout`, {
      method: "POST",
      headers: { Authorization: `Bearer ${accessToken}` },
      credentials: "omit",
    });
  } catch {
    // The session lapses on its own.
  }
}

/**
 * Sign in with a passkey. Browser: a discoverable ceremony
 * (`mediation: "conditional"` for autofill). Desktop: the system browser.
 * `isCurrent` is checked after every await: a stale attempt (aborted, or
 * replaced by another sign-in or a logout) never installs its session.
 */
export async function passkeySignIn(
  serverUrl: string,
  o: {
    mediation: "conditional" | "optional";
    signal: AbortSignal;
    username?: UsernameHint;
    isCurrent?: () => boolean;
  },
): Promise<AuthResult> {
  const hint = () => {
    const raw = typeof o.username === "function" ? o.username() : o.username;
    return raw?.trim() || null;
  };
  if (isTauri()) {
    return invokeCancellable(
      (attemptId) =>
        invoke<AuthResult>("passkey_sign_in", { serverUrl, username: hint(), attemptId }),
      o.signal,
    );
  }
  const live = () => !o.signal.aborted && (o.isCurrent?.() ?? true);
  const start = await passkeyPost<CeremonyStart>(
    serverUrl,
    "/auth/passkeys/login/start",
    {},
    o.signal,
  );
  const rpId = start.options.publicKey.rpId as string | undefined;
  const cred = await getCredential(start.options, o.signal, o.mediation);
  if (!live()) throw new PasskeyError("ABORTED");
  let response: FinishedSignIn;
  try {
    // Not cancelled by the attempt's signal: once sent, finish may mint a
    // session, so it must land to be ended if the attempt went stale.
    response = await passkeyPost<FinishedSignIn>(serverUrl, "/auth/passkeys/login/finish", {
      ceremony_id: start.ceremony_id,
      credential: credentialToJSON(cred),
      username: hint(),
    });
  } catch (err) {
    if (!live()) throw new PasskeyError("ABORTED");
    throw serverError(err, cred, rpId);
  }
  if (!live()) {
    await endStaleSession(serverUrl, response.access_token);
    throw new PasskeyError("ABORTED");
  }
  // The profile with the new token, before any global auth state changes;
  // a session that cannot be completed (or went stale meanwhile) is ended.
  let user: User;
  try {
    user = await fetchMe(serverUrl, response.access_token);
  } catch (err) {
    await endStaleSession(serverUrl, response.access_token);
    if (!live()) throw new PasskeyError("ABORTED");
    throw err;
  }
  if (!live()) {
    await endStaleSession(serverUrl, response.access_token);
    throw new PasskeyError("ABORTED");
  }
  browserState.serverUrl = serverUrl;
  localStorage.setItem("serverUrl", serverUrl);
  setSessionRestoreBlocked(false);
  browserState.accessToken = response.access_token;
  browserState.tokenExpiresAt = Date.now() + response.expires_in * 1000;
  scheduleTokenRefresh();
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
    return invokeCancellable(
      (attemptId) =>
        invoke<StepUpGrant>("passkey_step_up", {
          serverUrl: baseUrl,
          action,
          target: target ?? "",
          attemptId,
        }),
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

/** Whether this client can run a passkey ceremony (desktop: the system browser). */
export function canUsePasskeys(): boolean {
  return isTauri() || isWebAuthnAvailable();
}

/** `navigator.credentials.create` for C29 registration options (browser only). */
async function createCredential(
  options: CeremonyStart["options"],
  signal: AbortSignal,
): Promise<PublicKeyCredential> {
  if (isTauri() || !isWebAuthnAvailable()) throw new PasskeyError("PASSKEY_NOT_SUPPORTED");
  if (signal.aborted) throw new PasskeyError("ABORTED");
  try {
    const created = await navigator.credentials.create({
      publicKey: creationOptionsFromJSON(options.publicKey),
      signal,
    });
    if (!created) throw new PasskeyError("PASSKEY_FAILED");
    return created as PublicKeyCredential;
  } catch (err) {
    if (err instanceof PasskeyError) throw err;
    throw new PasskeyError(webAuthnErrorCode(err));
  }
}

/**
 * Create a passkey from C29 registration `options` (`{publicKey}`) and answer
 * its `credential` JSON, for the routes that finish elsewhere: setup links,
 * account requests and registration tickets (browser only).
 */
export async function createPasskeyCredential(options: unknown, signal: AbortSignal): Promise<unknown> {
  if (typeof options !== "object" || options === null) throw new PasskeyError("PASSKEY_FAILED");
  const o = options as Partial<CeremonyStart["options"]>;
  const publicKey = o.publicKey ?? (options as Record<string, unknown>);
  return credentialToJSON(await createCredential({ publicKey }, signal));
}

/** Add a passkey (browser only; needs a `passkey.add` grant first). */
export async function registerPasskey(o: { label: string; signal: AbortSignal }): Promise<Passkey> {
  if (isTauri() || !isWebAuthnAvailable()) throw new PasskeyError("PASSKEY_NOT_SUPPORTED");
  // Already cancelled: no ceremony is started on the server (Argon 5 #8).
  if (o.signal.aborted) throw new PasskeyError("ABORTED");
  const start = await httpRequest<CeremonyStart>("POST", "/auth/passkeys/register/start", {
    label: o.label,
  });
  const cred = await createCredential(start.options, o.signal);
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
