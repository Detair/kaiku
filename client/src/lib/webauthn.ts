/**
 * WebAuthn helpers for passkeys (C29): the L3 JSON form ↔ the browser's
 * ArrayBuffer options, error mapping and the credential signals.
 *
 * The server sends `options` = `{publicKey: …}` in the WebAuthn L3 JSON form
 * (base64url without padding) and takes `credential` =
 * `PublicKeyCredential.toJSON()`. The native `parse…FromJSON` / `toJSON()`
 * are used where the browser has them.
 */

type Json = Record<string, unknown>;

interface CredentialStatics {
  parseRequestOptionsFromJSON?: (o: unknown) => PublicKeyCredentialRequestOptions;
  parseCreationOptionsFromJSON?: (o: unknown) => PublicKeyCredentialCreationOptions;
  isConditionalMediationAvailable?: () => Promise<boolean>;
  signalUnknownCredential?: (o: { rpId: string; credentialId: string }) => Promise<void>;
  signalAllAcceptedCredentials?: (o: {
    rpId: string;
    userId: string;
    allAcceptedCredentialIds: string[];
  }) => Promise<void>;
}

function statics(): CredentialStatics | undefined {
  return (globalThis as { PublicKeyCredential?: CredentialStatics }).PublicKeyCredential;
}

function b64uToBuf(s: string): ArrayBuffer {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
  const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
  const bin = atob(padded);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out.buffer;
}

function bufToB64u(buf: ArrayBuffer | ArrayBufferView): string {
  const bytes =
    buf instanceof ArrayBuffer
      ? new Uint8Array(buf)
      : new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function decodeDescriptors(list: unknown): PublicKeyCredentialDescriptor[] {
  return ((list as Json[] | undefined) ?? []).map((c) => ({
    ...(c as object),
    id: b64uToBuf(c.id as string),
  })) as PublicKeyCredentialDescriptor[];
}

/** Whether this browser can run passkey ceremonies. */
export function isWebAuthnAvailable(): boolean {
  return (
    statics() !== undefined &&
    typeof navigator !== "undefined" &&
    typeof navigator.credentials?.get === "function"
  );
}

/** Whether passkey autofill (conditional mediation) is available. */
export async function isConditionalMediationAvailable(): Promise<boolean> {
  try {
    return (await statics()?.isConditionalMediationAvailable?.()) === true;
  } catch {
    return false;
  }
}

/** `publicKey` request options from the server's JSON. */
export function requestOptionsFromJSON(json: Json): PublicKeyCredentialRequestOptions {
  const native = statics()?.parseRequestOptionsFromJSON;
  if (native) return native(json);
  return {
    ...(json as object),
    challenge: b64uToBuf(json.challenge as string),
    allowCredentials: decodeDescriptors(json.allowCredentials),
  } as PublicKeyCredentialRequestOptions;
}

/** `publicKey` creation options from the server's JSON. */
export function creationOptionsFromJSON(json: Json): PublicKeyCredentialCreationOptions {
  const native = statics()?.parseCreationOptionsFromJSON;
  if (native) return native(json);
  const user = json.user as Json;
  return {
    ...(json as object),
    challenge: b64uToBuf(json.challenge as string),
    user: { ...(user as object), id: b64uToBuf(user.id as string) },
    excludeCredentials: decodeDescriptors(json.excludeCredentials),
  } as PublicKeyCredentialCreationOptions;
}

/** `PublicKeyCredential.toJSON()`, or the same shape built by hand. */
export function credentialToJSON(cred: PublicKeyCredential): Json {
  const withJson = cred as unknown as { toJSON?: () => Json };
  if (typeof withJson.toJSON === "function") return withJson.toJSON();
  const r = cred.response as AuthenticatorResponse & Record<string, unknown>;
  const response: Json = { clientDataJSON: bufToB64u(r.clientDataJSON) };
  if (r.attestationObject) {
    response.attestationObject = bufToB64u(r.attestationObject as ArrayBuffer);
    const transports = (r as { getTransports?: () => string[] }).getTransports?.();
    if (transports) response.transports = transports;
  } else {
    response.authenticatorData = bufToB64u(r.authenticatorData as ArrayBuffer);
    response.signature = bufToB64u(r.signature as ArrayBuffer);
    response.userHandle = r.userHandle ? bufToB64u(r.userHandle as ArrayBuffer) : null;
  }
  return {
    id: cred.id,
    rawId: bufToB64u(cred.rawId),
    type: cred.type,
    authenticatorAttachment: cred.authenticatorAttachment ?? null,
    clientExtensionResults: cred.getClientExtensionResults?.() ?? {},
    response,
  };
}

export type WebAuthnErrorCode =
  | "PASSKEY_CANCELLED"
  | "PASSKEY_ALREADY_REGISTERED"
  | "PASSKEY_NOT_SUPPORTED"
  | "PASSKEY_FAILED"
  | "ABORTED";

/** The client code of a `navigator.credentials` failure. */
export function webAuthnErrorCode(err: unknown): WebAuthnErrorCode {
  const name = (err as { name?: string } | null)?.name;
  switch (name) {
    case "NotAllowedError":
      return "PASSKEY_CANCELLED";
    case "InvalidStateError":
      return "PASSKEY_ALREADY_REGISTERED";
    case "AbortError":
      return "ABORTED";
    case "NotSupportedError":
    case "SecurityError":
      return "PASSKEY_NOT_SUPPORTED";
    default:
      return "PASSKEY_FAILED";
  }
}

/** The server's `DELETE /auth/passkeys/{id}` answer (C29). */
export interface PasskeyRemovedResponse {
  rp_id: string;
  user_handle: string;
  credential_id: string;
  remaining_credential_ids: string[];
}

/** After a delete: tell the browser's passkey manager (feature-detected). */
export async function signalRemoved(r: PasskeyRemovedResponse): Promise<void> {
  const s = statics();
  try {
    await s?.signalUnknownCredential?.({ rpId: r.rp_id, credentialId: r.credential_id });
  } catch {
    // Best effort only.
  }
  try {
    await s?.signalAllAcceptedCredentials?.({
      rpId: r.rp_id,
      userId: r.user_handle,
      allAcceptedCredentialIds: r.remaining_credential_ids,
    });
  } catch {
    // Best effort only.
  }
}

/** A credential the server does not know (`PASSKEY_UNKNOWN`); never for `pending`. */
export async function signalUnknown(rpId: string, credentialId: string): Promise<void> {
  try {
    await statics()?.signalUnknownCredential?.({ rpId, credentialId });
  } catch {
    // Best effort only.
  }
}
