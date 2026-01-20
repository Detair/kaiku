-- E2EE Keys Migration: Device-based key management with backups
-- Migration: 20260119000000_e2ee_keys
--
-- This migration adds support for:
-- - Multi-device per user (each device has its own identity keys)
-- - One-time prekeys for Olm session establishment
-- - Encrypted key backups (password-protected)
-- - Device-to-device key transfer with short TTL
--
-- NOTE: This migration adds tables for the new E2EE encryption system.
-- The existing user_keys table from initial_schema.sql is a legacy placeholder
-- that will be deprecated once E2EE is fully rolled out. The new system uses:
-- - user_devices: Per-device identity keys (users can have multiple devices)
-- - prekeys: One-time prekeys for Olm session establishment
-- - key_backups: Encrypted backup of identity keys
-- - device_transfers: Temporary key transfer between devices

-- ============================================================================
-- User Devices with Identity Keys
-- ============================================================================
-- Each user can have multiple devices. Each device has its own Ed25519 signing
-- key and Curve25519 key exchange key (Olm identity keys).

CREATE TABLE user_devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_name TEXT,
    identity_key_ed25519 TEXT NOT NULL,
    identity_key_curve25519 TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(user_id, identity_key_curve25519),
    CONSTRAINT device_name_length CHECK (device_name IS NULL OR length(device_name) <= 128),
    CONSTRAINT identity_ed25519_length CHECK (length(identity_key_ed25519) <= 64),
    CONSTRAINT identity_curve25519_length CHECK (length(identity_key_curve25519) <= 64)
);

CREATE INDEX idx_user_devices_user_id ON user_devices(user_id);
CREATE INDEX idx_user_devices_identity_curve25519 ON user_devices(identity_key_curve25519);

-- ============================================================================
-- One-Time Prekeys
-- ============================================================================
-- Prekeys are uploaded by devices and claimed atomically by other users
-- when establishing Olm sessions. Once claimed, they cannot be reused.

CREATE TABLE prekeys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    key_id TEXT NOT NULL,
    public_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    claimed_by UUID REFERENCES users(id),
    UNIQUE(device_id, key_id),
    CONSTRAINT key_id_length CHECK (length(key_id) <= 64),
    CONSTRAINT public_key_length CHECK (length(public_key) <= 64)
);

-- Index for efficiently finding unclaimed prekeys for a device
CREATE INDEX idx_prekeys_device_unclaimed ON prekeys(device_id) WHERE claimed_at IS NULL;

-- ============================================================================
-- Encrypted Key Backups
-- ============================================================================
-- Users can backup their Olm account (identity keys + session data) encrypted
-- with a password-derived key. Only one backup per user (UPSERT pattern).

CREATE TABLE key_backups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    salt BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    ciphertext BYTEA NOT NULL,
    version INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id),
    CONSTRAINT salt_size CHECK (octet_length(salt) = 16),
    CONSTRAINT nonce_size CHECK (octet_length(nonce) = 12),
    CONSTRAINT ciphertext_max_size CHECK (octet_length(ciphertext) <= 1048576)  -- 1MB max
);

-- ============================================================================
-- Device Transfer Requests
-- ============================================================================
-- Temporary storage for device-to-device key transfer. New devices can request
-- keys from existing verified devices. Short TTL (5 minutes) for security.

CREATE TABLE device_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    encrypted_keys BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '5 minutes',
    CONSTRAINT transfer_nonce_size CHECK (octet_length(nonce) = 12),
    CONSTRAINT encrypted_keys_max_size CHECK (octet_length(encrypted_keys) <= 65536)  -- 64KB max
);

-- Index for looking up transfers by target device
CREATE INDEX idx_device_transfers_target ON device_transfers(target_device_id);

-- Index for cleanup job to efficiently find expired transfers
CREATE INDEX idx_device_transfers_expires ON device_transfers(expires_at);
