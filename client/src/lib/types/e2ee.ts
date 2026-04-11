/**
 * End-to-end encryption (Olm/Megolm) types.
 */

export interface E2EEStatus {
  initialized: boolean;
  device_id: string | null;
  has_identity_keys: boolean;
}

export interface InitE2EEResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  prekeys: PrekeyData[];
}

export interface PrekeyData {
  key_id: string;
  public_key: string;
}

export interface DeviceKeys {
  device_id: string;
  device_name: string | null;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
}

export interface UserKeysResponse {
  devices: DeviceKeys[];
}

export interface ClaimedPrekeyResponse {
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
}

export interface E2EEContent {
  sender_key: string;
  recipients: Record<string, Record<string, EncryptedMessage>>;
}

export interface EncryptedMessage {
  message_type: number;
  ciphertext: string;
}

/** Megolm group-encrypted message content (distinguished from Olm by `megolm_ciphertext` key). */
export interface MegolmE2EEContent {
  /** Sender's Curve25519 public key (base64) */
  sender_key: string;
  /** Room/channel this was encrypted for */
  room_id: string;
  /** Megolm ciphertext (base64) */
  megolm_ciphertext: string;
}

export interface ClaimedPrekeyInput {
  user_id: string;
  device_id: string;
  identity_key_ed25519: string;
  identity_key_curve25519: string;
  one_time_prekey: {
    key_id: string;
    public_key: string;
  } | null;
}
