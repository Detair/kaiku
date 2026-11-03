/**
 * Registration tickets (C27, F8): after `POST /auth/register` the new account
 * has 30 minutes to bind a passkey, a phone or an authenticator app. Browser:
 * the `__Secure-kaiku_ticket` cookie; desktop: Rust keeps the ticket and adds
 * `X-Kaiku-Ticket`.
 */

import type { RegistrationStatus, TotpSetup } from "../types/login";
import { httpRequest } from "./common";
import { preSessionRequest } from "./preSession";

export function getRegistration(): Promise<RegistrationStatus> {
  return preSessionRequest<RegistrationStatus>("ticket", "GET", "/auth/registration");
}

export function startRegistrationTotp(): Promise<TotpSetup> {
  return preSessionRequest<TotpSetup>("ticket", "POST", "/auth/registration/totp");
}

/** Verify the first code; the recovery codes come back once. */
export async function verifyRegistrationTotp(code: string): Promise<string[]> {
  const r = await preSessionRequest<{ recovery_codes?: string[] }>(
    "ticket",
    "POST",
    "/auth/registration/totp/verify",
    { code },
  );
  return r?.recovery_codes ?? [];
}

/** Browser only: the ticket's passkey (C29). */
export function startRegistrationPasskey(label: string | null): Promise<{ ceremony_id: string; options: unknown }> {
  return httpRequest("POST", "/auth/registration/passkey/start", { label });
}

export function finishRegistrationPasskey(ceremonyId: string, credential: unknown): Promise<unknown> {
  return httpRequest("POST", "/auth/registration/passkey/finish", { ceremony_id: ceremonyId, credential });
}
