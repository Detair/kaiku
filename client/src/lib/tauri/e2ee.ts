/**
 * End-to-end encryption commands (Olm + Megolm).
 *
 * Most E2EE commands require Tauri; browser mode falls back to errors
 * or disabled state.
 */

import type {
  ClaimedPrekeyInput,
  ClaimedPrekeyResponse,
  E2EEContent,
  E2EEStatus,
  InitE2EEResponse,
  PrekeyData,
  UserKeysResponse,
} from "../types";
import { httpRequest, isTauri } from "./common";

/**
 * Get the current E2EE status (initialization state, device ID, etc.).
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function getE2EEStatus(): Promise<E2EEStatus> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<E2EEStatus>("get_e2ee_status");
  }

  // Browser mode - E2EE not available
  return {
    initialized: false,
    device_id: null,
    has_identity_keys: false,
  };
}

/**
 * Initialize E2EE with the given encryption key (derived from user password).
 * This generates identity keys and prekeys for the device.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function initE2EE(
  encryptionKey: string,
): Promise<InitE2EEResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<InitE2EEResponse>("init_e2ee", { encryptionKey });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Encrypt a message for the given recipients.
 * Recipients must include their claimed prekeys from the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function encryptMessage(
  plaintext: string,
  recipients: ClaimedPrekeyInput[],
): Promise<E2EEContent> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<E2EEContent>("encrypt_message", { plaintext, recipients });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Decrypt a message from another user.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function decryptMessage(
  senderUserId: string,
  senderKey: string,
  messageType: number,
  ciphertext: string,
): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("decrypt_message", {
      senderUserId,
      senderKey,
      messageType,
      ciphertext,
    });
  }

  throw new Error("E2EE requires the native Tauri app");
}

// =============================================================================
// Megolm Group E2EE Commands
// =============================================================================

/**
 * Create a new Megolm outbound session for a group/channel.
 * Returns the exportable session key (base64) that should be shared with other members.
 */
export async function createMegolmSession(roomId: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("create_megolm_session", { roomId });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Encrypt a message for a group using Megolm.
 */
export async function encryptGroupMessage(roomId: string, plaintext: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("encrypt_group_message", { roomId, plaintext });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Store an inbound Megolm session key received from another user.
 */
export async function addInboundGroupSession(
  roomId: string,
  senderKey: string,
  sessionKey: string
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("add_inbound_group_session", { roomId, senderKey, sessionKey });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Decrypt a Megolm group message.
 */
export async function decryptGroupMessage(
  roomId: string,
  senderKey: string,
  ciphertext: string
): Promise<string> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("decrypt_group_message", { roomId, senderKey, ciphertext });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Mark prekeys as published after uploading them to the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function markPrekeysPublished(): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<void>("mark_prekeys_published");
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Generate additional prekeys (one-time keys).
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function generatePrekeys(count: number): Promise<PrekeyData[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PrekeyData[]>("generate_prekeys", { count });
  }

  throw new Error("E2EE requires the native Tauri app");
}

/**
 * Check if the device needs to upload more prekeys to the server.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function needsPrekeyUpload(): Promise<boolean> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<boolean>("needs_prekey_upload");
  }

  // Browser mode - always return false
  return false;
}

/**
 * Get our Curve25519 public key (base64).
 * This is needed for looking up our ciphertext in encrypted messages.
 * Note: E2EE commands require Tauri - they are not available in browser mode.
 */
export async function getOurCurve25519Key(): Promise<string | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_our_curve25519_key");
  }

  // Browser mode - not available
  return null;
}

// ============================================================================
// E2EE Key API Endpoints
// ============================================================================

/**
 * Get another user's device keys for establishing encrypted sessions.
 * Returns all devices and their public identity keys.
 */
export async function getUserKeys(userId: string): Promise<UserKeysResponse> {
  return httpRequest<UserKeysResponse>("GET", `/api/users/${userId}/keys`);
}

/**
 * Claim a prekey from a specific device to establish an encrypted session.
 * The prekey is consumed and cannot be reused.
 */
export async function claimPrekey(
  userId: string,
  deviceId: string,
): Promise<ClaimedPrekeyResponse> {
  return httpRequest<ClaimedPrekeyResponse>(
    "POST",
    `/api/users/${userId}/keys/claim`,
    { device_id: deviceId },
  );
}

/**
 * Upload identity keys and prekeys to the server.
 * Creates or updates the device record.
 */
export async function uploadKeys(
  deviceName: string | null,
  identityKeyEd25519: string,
  identityKeyCurve25519: string,
  oneTimePrekeys: PrekeyData[],
): Promise<{
  device_id: string;
  prekeys_uploaded: number;
  prekeys_skipped: number;
}> {
  return httpRequest<{
    device_id: string;
    prekeys_uploaded: number;
    prekeys_skipped: number;
  }>("POST", "/api/keys/upload", {
    device_name: deviceName,
    identity_key_ed25519: identityKeyEd25519,
    identity_key_curve25519: identityKeyCurve25519,
    one_time_prekeys: oneTimePrekeys.map((pk) => ({
      key_id: pk.key_id,
      public_key: pk.public_key,
    })),
  });
}

