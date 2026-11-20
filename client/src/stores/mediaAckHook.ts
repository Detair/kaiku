/**
 * Server answers to a screen share or webcam start (contract C30), handed
 * from the socket store to the publish flow (`stores/mediaPublish.ts`)
 * without loading it. No imports: this module sits in the startup payload.
 */

/** The events a start waits for. */
export type MediaAnswer =
  | { type: "screen_share_started"; channel_id: string; user_id: string; stream_id: string }
  | { type: "webcam_started"; channel_id: string; user_id: string }
  | { type: "voice_error"; code: string; message: string };

let listener: ((answer: MediaAnswer) => void) | null = null;

/** The publish flow registers itself when it is loaded. */
export function setMediaAnswerListener(fn: ((answer: MediaAnswer) => void) | null): void {
  listener = fn;
}

/** Called by the socket store for every event a start could wait for. */
export function noteMediaAnswer(answer: MediaAnswer): void {
  listener?.(answer);
}
