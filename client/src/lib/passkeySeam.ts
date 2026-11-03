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
 *
 * Until then nothing is registered and every passkey entry point stays hidden
 * (`passkeyCeremonies()` is null). Tests register fakes.
 */

import type { AuthResult } from "@/lib/tauri/common";
import type { StepUpAction, StepUpGrant } from "@/lib/types";

export interface PasskeyCeremonies {
  signIn(serverUrl: string, o: { mediation: "optional"; signal: AbortSignal }): Promise<AuthResult>;
  stepUp(action: StepUpAction, target: string, signal: AbortSignal): Promise<StepUpGrant>;
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
