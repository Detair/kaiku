# Encryption Architecture

VoiceChat employs a multi-layered encryption strategy to ensure data privacy and security.

## Layer 1: Transport Security (TLS)

All connections between clients and the server are secured using **TLS 1.3**.

- **Implementation:** Uses `rustls` on the server and client.
- **Scope:** All HTTP REST API calls and WebSocket connections.
- **Features:**
    - Perfect Forward Secrecy.
    - Certificate Pinning (optional/planned for client).

## Layer 2: Voice Encryption (DTLS-SRTP)

Voice data is transmitted peer-to-peer (or via SFU) using standard WebRTC encryption protocols.

- **Protocol:** DTLS-SRTP (Datagram Transport Layer Security - Secure Real-time Transport Protocol).
- **Key Exchange:** DTLS handshake establishes keys for SRTP packets.
- **SFU Role:**
    - **Current (MVP):** The SFU (Selective Forwarding Unit) decrypts SRTP packets to route them, then re-encrypts for the receiver. The SFU is a trusted entity.
    - **Future (Paranoid Mode):** MLS (Messaging Layer Security) for End-to-End Encryption (E2EE) of voice frames, where the SFU only sees ciphertext.

## Layer 3: Text Message E2EE (End-to-End Encryption)

Text messages use the **Olm** and **Megolm** cryptographic ratchets via the `vodozemac` library (Apache 2.0), ensuring that the server cannot read message content.

> **Implementation Status:** ✅ DM messages (1:1 Olm and Group Megolm) are fully E2EE enabled. Guild channel encryption is planned.

### 1:1 Direct Messages (Olm)
- **Algorithm:** Double Ratchet Algorithm (Olm).
- **Library:** `vodozemac` (Rust implementation of Olm/Megolm)
- **Key Agreement:** X3DH (Extended Triple Diffie-Hellman) using pre-keys stored on the server.
- **Properties:**
    - **Confidentiality:** Only the recipient can decrypt.
    - **Forward Secrecy:** Compromising current keys does not compromise past messages.
    - **Post-Compromise Security:** New keys heal the session after a compromise.

### Group DMs (Megolm)
- **Algorithm:** Megolm ratchet for efficient group encryption.
- **Library:** `vodozemac` v0.9 (Rust implementation of Olm/Megolm)
- **Mechanism:**
    - Sender creates an outbound Megolm session per channel.
    - Session key distributed to all group members via 1:1 Olm-encrypted messages.
    - Messages encrypted with the Megolm ratchet (one encrypt → all can decrypt).
    - On receive: Olm decryption detects `__megolm_session_key__` marker, auto-stores inbound session.
- **Message Format:** `MegolmE2EEContent` JSON containing `sender_key`, `room_id`, and `megolm_ciphertext`.
- **Session Rotation:**
    - Automatic rotation every 100 messages (matching Matrix protocol).
    - Immediate rotation when participant list changes.
    - Session state cached in-memory for performance.
- **Efficiency:** Encrypt once, send to many. Key distribution cost amortized over 100 messages.

### Guild Channels (Megolm) - Planned
- Same Megolm mechanism as Group DMs, adapted for larger guild channels.
- Additional considerations: member count scaling, lazy key distribution.

### Key Management
- **Identity Keys:** Long-term Curve25519 keys identifying a user/device.
- **One-Time Keys:** Uploaded to server for initial session establishment (X3DH).
- **Storage:**
    - **Tauri Client:** Encrypted SQLite database (`LocalKeyStore`) with AES-256-GCM encryption via SQLCipher.
    - **Key Derivation:** Recovery key → Argon2id → encryption key for SQLite storage.
    - **Recovery Key:** 128-bit random value displayed as Base58-encoded chunks for user backup.
- **Session Persistence:** Olm and Megolm sessions serialized (pickled) and stored encrypted, survive app restarts.

## Layer 4: Data at Rest

- **Database:** Standard disk encryption (e.g., LUKS) recommended for deployment.
- **Server:** Stores only E2EE ciphertexts for messages.
- **File Uploads:** Encrypted with AES-256-GCM before upload to S3.
- **Backups:** Encrypted with a server-side master key.

## References

- [Double Ratchet Algorithm](https://signal.org/docs/specifications/doubleratchet/)
- [X3DH Key Agreement](https://signal.org/docs/specifications/x3dh/)
- [Megolm](https://gitlab.matrix.org/matrix-org/olm/blob/master/docs/megolm.md)