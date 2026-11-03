/**
 * Seam for LP's passkey ceremonies (C29), which are not on main yet.
 *
 * TODO(LP rebase): delete this module and call LP's functions directly:
 * - `signIn` → `stores/auth.ts::loginWithPasskey(serverUrl, {mediation, signal})`
 *   (browser: `navigator.credentials.get`; desktop: the `passkey_sign_in`
 *   command, which adopts the tokens in Rust and ends the old session).
 *   If LP's `loginWithPasskey` already calls `completeSignIn`, drop the call
 *   in `replaceSessionWithPasskey`.
 * - `stepUp` → `lib/tauri/passkeys.ts::stepUpWithPasskey(action, target, signal)`
 *   (browser ceremony, or the `passkey_step_up` command in the system browser).
 * - `create` → LP's `lib/webauthn.ts`: `navigator.credentials.create({publicKey:
 *   creationOptionsFromJSON(options.publicKey)})` then `credentialToJSON(cred)`
 *   (browser only; registration never runs in the desktop app). Without it,
 *   `createPasskeyFromJSON` uses the WebAuthn L3 JSON methods when present.
 *
 * Until then nothing is registered and every passkey entry point stays hidden
 * (`passkeyCeremonies()` is null). Tests register fakes.
 */

import type { AuthResult } from "@/lib/tauri/common";
import type { StepUpAction, StepUpGrant } from "@/lib/types";

export interface PasskeyCeremonies {
  signIn(serverUrl: string, o: { mediation: "optional"; signal: AbortSignal }): Promise<AuthResult>;
  stepUp(action: StepUpAction, target: string, signal: AbortSignal): Promise<StepUpGrant>;
  /** Create a passkey from C29 `options` JSON; resolves the `credential` JSON. */
  create?(options: unknown, signal: AbortSignal): Promise<unknown>;
}

let ceremonies: PasskeyCeremonies | null = null;

/** LP (or a test) provides the ceremonies; `null` removes them. */
export function registerPasskeyCeremonies(c: PasskeyCeremonies | null): void {
  ceremonies = c;
}

/** The registered ceremonies, or null while LP is not merged. */
export function passkeyCeremonies(): PasskeyCeremonies | null {
  return ceremonies;
}

interface JsonCredentialStatics {
  parseCreationOptionsFromJSON?: (o: unknown) => PublicKeyCredentialCreationOptions;
}

/**
 * Browser passkey creation for setup links, requests and tickets (C29):
 * LP's ceremony when registered, else the WebAuthn L3 JSON helpers. Throws
 * `PASSKEY_NOT_SUPPORTED` when neither exists.
 */
export async function createPasskeyFromJSON(options: unknown, signal: AbortSignal): Promise<unknown> {
  if (ceremonies?.create) return ceremonies.create(options, signal);
  const statics = (globalThis as { PublicKeyCredential?: JsonCredentialStatics }).PublicKeyCredential;
  const publicKey = (options as { publicKey?: unknown } | null)?.publicKey ?? options;
  if (!statics?.parseCreationOptionsFromJSON || !navigator.credentials?.create) {
    throw new Error("PASSKEY_NOT_SUPPORTED");
  }
  const cred = (await navigator.credentials.create({
    publicKey: statics.parseCreationOptionsFromJSON(publicKey),
    signal,
  })) as (Credential & { toJSON?: () => unknown }) | null;
  if (!cred?.toJSON) throw new Error("PASSKEY_FAILED");
  return cred.toJSON();
}
