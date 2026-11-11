/**
 * Voice session epoch (C32 amendment "leave and voice_removed win").
 *
 * One monotonically increasing number for every voice session in this
 * window: channel voice and DM-call voice, browser and native. Each join
 * (and each rejoin attempt) runs under the epoch that was current when it
 * started. Leave, switching channel, a manual join, give-up and
 * `voice_removed` bump the epoch first; a join that finds its epoch stale
 * after an await releases what it acquired and reports `cancelled`.
 *
 * This counter only orders work inside this window (JS). In the desktop
 * app the native process allocates its own session epoch for each join
 * (`begin_voice_session`), so reloads and several windows share one order
 * there; the browser has no surviving native state and needs nothing more.
 */

let epoch = 0;

/** The epoch of the voice session that may currently acquire resources. */
export function currentVoiceEpoch(): number {
  return epoch;
}

/** Start a new epoch: every join running under an older one is cancelled. */
export function bumpVoiceEpoch(): number {
  epoch += 1;
  return epoch;
}

/** Whether a join running under `joinEpoch` must stop (undefined = never). */
export function isVoiceEpochStale(joinEpoch: number | undefined): boolean {
  return joinEpoch !== undefined && joinEpoch !== epoch;
}

/** DM-call joins: the epoch captured when the call was started/accepted. */
const callJoinEpochs = new Map<string, number>();

/**
 * Capture the session epoch for the DM call in `channelId` right now (call
 * started or accepted, before any await). A leave before the native join
 * makes it stale, and the join then does nothing.
 */
export function captureCallJoinEpoch(channelId: string): number {
  callJoinEpochs.set(channelId, epoch);
  return epoch;
}

/** The epoch captured for `channelId` (consumed), if any. */
export function takeCallJoinEpoch(channelId: string): number | undefined {
  const captured = callJoinEpochs.get(channelId);
  callJoinEpochs.delete(channelId);
  return captured;
}

/** Error a join returns when its epoch went stale; nobody shows it. */
export const VOICE_JOIN_CANCELLED = {
  type: "cancelled",
  message: "voice join cancelled",
} as const;

/** Whether a join error is the cancellation above (not a failure). */
export function isJoinCancelled(error: { type: string } | null | undefined): boolean {
  return error?.type === "cancelled";
}

/** Error text of a native `join_voice` whose epoch went stale (Rust). */
export const NATIVE_JOIN_CANCELLED = "VOICE_JOIN_CANCELLED";

/** Whether a native join failed only because its session was cancelled. */
export function isNativeJoinCancelled(err: unknown): boolean {
  return (typeof err === "string" ? err : String(err)).includes(NATIVE_JOIN_CANCELLED);
}
