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
 * The counter starts at the wall clock (ms) so a reloaded window starts
 * above every epoch the native side saw before the reload.
 */

let epoch = Date.now();

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

/** Error a join returns when its epoch went stale; nobody shows it. */
export const VOICE_JOIN_CANCELLED = {
  type: "cancelled",
  message: "voice join cancelled",
} as const;

/** Whether a join error is the cancellation above (not a failure). */
export function isJoinCancelled(error: { type: string } | null | undefined): boolean {
  return error?.type === "cancelled";
}
