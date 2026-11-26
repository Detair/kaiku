/**
 * Links between the start-up path and the voice code.
 *
 * The voice owner (and with it adapters, recovery and stores) is loaded
 * when voice is first needed. Code on the start-up path must still be able
 * to end voice synchronously (before the signed-in user changes) and to
 * tell the owner that a frame arrived. It calls these hooks; the owner
 * fills them in when it is loaded. No voice code loaded means there is no
 * voice session, so there is nothing to end.
 *
 * Keep this module free of imports.
 */

interface VoiceHooks {
  /** End voice now: intent dropped, media released before this returns. */
  shutdown: () => void;
  /** A frame from the server arrived on the signalling socket. */
  inbound: () => void;
}

let hooks: VoiceHooks | null = null;
let identity: string | null = null;

/** Called by the voice owner when it is loaded. */
export function registerVoiceHooks(registered: VoiceHooks): void {
  hooks = registered;
}

/**
 * End voice synchronously. Called before every change of the signed-in
 * user and when the session ends.
 */
export function shutdownVoice(): void {
  hooks?.shutdown();
}

/** Any frame from the server shows that the socket is alive right now. */
export function noteVoiceInbound(): void {
  hooks?.inbound();
}

/** Who is signed in now (told by the auth store whenever it changes). */
export function setVoiceIdentity(userId: string | null): void {
  identity = userId;
}

/** The signed-in user's id, or null. */
export function voiceIdentity(): string | null {
  return identity;
}
