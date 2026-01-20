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

Text messages use the **Olm** and **Megolm** cryptographic ratchets, ensuring that the server cannot read message content.

### 1:1 Direct Messages (Olm)
- **Algorithm:** Double Ratchet Algorithm (Olm).
- **Key Agreement:** X3DH (Extended Triple Diffie-Hellman) using pre-keys stored on the server.
- **Properties:**
    - **Confidentiality:** Only the recipient can decrypt.
    - **Forward Secrecy:** Compromising current keys does not compromise past messages.
    - **Post-Compromise Security:** New keys heal the session after a compromise.

### Group Channels (Megolm)
- **Algorithm:** Megolm (Outbound group sessions).
- **Mechanism:**
    - Sender creates an outbound Megolm session.
    - Session keys are distributed to group members via 1:1 Olm channels.
    - Messages are encrypted with the Megolm ratchet.
    - Receivers use their copy of the session key to decrypt.
- **Efficiency:** Encrypt once, send to many.

### Key Management
- **Identity Keys:** Long-term Curve25519 keys identifying a user/device.
- **One-Time Keys:** Uploaded to server for initial session establishment (X3DH).
- **Storage:** Keys are stored in the client's secure storage (OS Keychain).

## Layer 4: Data at Rest

- **Database:** Standard disk encryption (e.g., LUKS) recommended for deployment.
- **Server:** Stores only E2EE ciphertexts for messages.
- **File Uploads:** Encrypted with AES-256-GCM before upload to S3.
- **Backups:** Encrypted with a server-side master key.

## References

- [Double Ratchet Algorithm](https://signal.org/docs/specifications/doubleratchet/)
- [X3DH Key Agreement](https://signal.org/docs/specifications/x3dh/)
- [Megolm](https://gitlab.matrix.org/matrix-org/olm/blob/master/docs/megolm.md)