# <span style="color: #88C0D0;">Security & Privacy</span>

> [!WARNING]  
> **Early Development Note:** Kaiku is heavily in development. Security features are currently being implemented and have *not* yet been audited. Do not rely on Kaiku for sensitive communication in its current pre-release state.

Kaiku was fundamentally built around the concept of **Absolute Data Freedom**. This means we are designing the system in a way where even the server administrator mathematically cannot read your private messages.

---

## <span style="color: #88C0D0;">End-to-End Encryption (E2EE)</span>

All Direct Messages (DMs) and private group text channels in Kaiku utilize the **Olm cryptographic ratchet** (the same foundational protocol that powers Signal and Matrix).

### How it Works
1. **Key Generation**: When you install Kaiku, your local client generates a unique Ed25519 identity key and a set of one-time Curve25519 pre-keys.
2. **The Handshake**: When you start a DM, your client requests the recipient's pre-keys from the Server, establishes a shared secret, and immediately begins encrypting traffic.
3. **The Ratchet**: With every single message sent, the encryption keys automatically rotate (ratchet). If a hacker somehow steals a key from memory, it is practically useless for reading past or future messages.

The Server *only* handles the encrypted ciphertext. It never holds the private keys necessary to decrypt your conversations.

---

## <span style="color: #88C0D0;">Voice & Video Encryption</span>

Voice and Video traffic routed through WebRTC is automatically secured by **DTLS-SRTP** (Datagram Transport Layer Security — Secure Real-Time Transport Protocol). 

- **Peer-to-Peer Calls**: In a direct 1-1 call, the DTLS handshake occurs directly between the two clients. The media stream is encrypted and unreadable by anyone else, including the networking relay (TURN) server if one is used.
- **Server Channels (SFU)**: *Implementation Details for scalable SFU E2EE (like Insertable Streams) are currently under final technical evaluation.*

---

## <span style="color: #88C0D0;">Data Sovereignty Defaults</span>

- **No Telemetry**: The Kaiku client ships with exactly zero analytics trackers, telemetry daemons, or "opt-out" data harvesting.
- **Self-Destructing Caches**: Features built to automatically purge temporary media caches on client exit. 
- **Verifiable Builds**: As an open-source tool, our build pipelines and dependencies will be strictly verifiable to ensure the binary you download directly matches the public Rust source code.
