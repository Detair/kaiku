/**
 * Enrolment of a phone or passkey (C26 SM2, C29 passkey claims).
 *
 * Issuer side, three bases (C26 "Enrolment core"): the signed-in account
 * (`own`, bearer), an account request in `ready` (request secret) and a
 * registration ticket. The new-device side here is only the browser passkey
 * claim of a setup link and its status long-poll; the setup token travels in
 * request bodies only, never in a URL.
 */

import {
  decodeClaimStatus,
  decodeEnrollment,
  type ClaimStatus,
  type Enrollment,
  type EnrollmentIssued,
  type PasskeyClaimed,
  type PasskeyClaimStart,
} from "../types/login";
import { httpRequest } from "./common";
import { preSessionRequest, type PreSessionKind } from "./preSession";

export type EnrollmentIssuer = { kind: "own" } | { kind: "request" } | { kind: "registration" };

const ISSUER_BASE: Record<EnrollmentIssuer["kind"], string> = {
  own: "/auth/enrollments",
  request: "/auth/account-requests/current/enrollments",
  registration: "/auth/registration/enrollments",
};

const ISSUER_SECRET: Record<EnrollmentIssuer["kind"], PreSessionKind | null> = {
  own: null,
  request: "request",
  registration: "ticket",
};

function issuerCall<T>(issuer: EnrollmentIssuer, method: string, suffix: string, body?: unknown): Promise<T> {
  const path = `${ISSUER_BASE[issuer.kind]}${suffix}`;
  const secret = ISSUER_SECRET[issuer.kind];
  return secret ? preSessionRequest<T>(secret, method, path, body) : httpRequest<T>(method, path, body);
}

/** Issue a setup code. `own` needs the grant `enrollment.issue`; 409 `ENROLLMENT_LIMIT`. */
export function issueEnrollment(issuer: EnrollmentIssuer): Promise<EnrollmentIssued> {
  return issuerCall<EnrollmentIssued>(issuer, "POST", "", {});
}

export async function getEnrollment(issuer: EnrollmentIssuer, id: string): Promise<Enrollment> {
  return decodeEnrollment(await issuerCall<unknown>(issuer, "GET", `/${encodeURIComponent(id)}`));
}

/** Confirm the 6 digits the claiming device shows; 400 `ENROLL_CODE_WRONG {tries_left}`, 410 `ENROLLMENT_FAILED`. */
export async function confirmEnrollment(issuer: EnrollmentIssuer, id: string, code: string): Promise<Enrollment> {
  return decodeEnrollment(
    await issuerCall<unknown>(issuer, "POST", `/${encodeURIComponent(id)}/confirm`, { code }),
  );
}

export async function cancelEnrollment(issuer: EnrollmentIssuer, id: string): Promise<void> {
  await issuerCall<void>(issuer, "DELETE", `/${encodeURIComponent(id)}`);
}

/** Browser only: start the passkey claim of a setup link (C29). */
export function startPasskeyClaim(setupToken: string): Promise<PasskeyClaimStart> {
  return httpRequest<PasskeyClaimStart>("POST", "/auth/enrollments/passkey/start", {
    setup_token: setupToken,
  });
}

/** Browser only: finish it; the claim-secret cookie is set with the answer. */
export function finishPasskeyClaim(
  setupToken: string,
  ceremonyId: string,
  credential: unknown,
  label: string | null,
): Promise<PasskeyClaimed> {
  return httpRequest<PasskeyClaimed>("POST", "/auth/enrollments/passkey/finish", {
    setup_token: setupToken,
    ceremony_id: ceremonyId,
    credential,
    label,
  });
}

/** Long-poll the claim (browser: the claim cookie authenticates it). */
export async function claimStatus(enrollmentId: string, wait: number): Promise<ClaimStatus> {
  const w = Math.max(0, Math.min(25, Math.round(wait)));
  return decodeClaimStatus(
    await httpRequest<unknown>("POST", "/auth/enrollments/status", { enrollment_id: enrollmentId, wait: w }),
  );
}
