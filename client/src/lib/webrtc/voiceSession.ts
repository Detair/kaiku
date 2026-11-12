/**
 * Voice session handles.
 *
 * Every join creates exactly one voice session inside the adapter, named by
 * a handle. Whoever started the join holds the handle and is the only one
 * who can end that session: `leave(handle)` / `teardown(handle)` release
 * what THAT join acquired (its microphone stream, its peer connections,
 * its native epoch) and nothing else. There is no "current session" to
 * look up: a join that finds its own session ended after an await releases
 * what it just got and reports `cancelled`; adapter events carry the handle
 * of the session they belong to.
 *
 * Tokens only need to be unique inside this window. In the desktop app the
 * adapter maps each handle to the epoch the native process allocated for
 * it (`begin_voice_session`), which orders sessions across reloads and
 * windows.
 */

/** One voice session inside an adapter: created by exactly one join. */
export interface VoiceSessionHandle {
  /** Unique per join in this window; never reused. */
  readonly token: number;
  /** Channel (or DM channel) the session is in. */
  readonly channelId: string;
}

let lastToken = 0;

/** A token no other user action or join in this window has. */
export function nextVoiceToken(): number {
  lastToken += 1;
  return lastToken;
}

/** Whether an adapter event for `session` belongs to the session `held`. */
export function isSameSession(
  session: VoiceSessionHandle | null | undefined,
  held: VoiceSessionHandle | null | undefined,
): boolean {
  return !!session && !!held && session.token === held.token;
}

/** Error a join returns when its session was ended meanwhile; nobody shows it. */
export const VOICE_JOIN_CANCELLED = {
  type: "cancelled",
  message: "voice join cancelled",
} as const;

/** Whether a join error is the cancellation above (not a failure). */
export function isJoinCancelled(error: { type: string } | null | undefined): boolean {
  return error?.type === "cancelled";
}

/** Error text of a native `join_voice` whose session was ended (Rust). */
export const NATIVE_JOIN_CANCELLED = "VOICE_JOIN_CANCELLED";

/** Whether a native join failed only because its session was cancelled. */
export function isNativeJoinCancelled(err: unknown): boolean {
  return (typeof err === "string" ? err : String(err)).includes(NATIVE_JOIN_CANCELLED);
}
