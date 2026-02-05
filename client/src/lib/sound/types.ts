/**
 * Sound Service Types
 *
 * Type definitions for the notification sound system.
 */

// Re-export settings types for convenience
export type { SoundOption, NotificationLevel } from "@/stores/sound";

/**
 * Sound event categories that can trigger notifications.
 */
export type SoundEventType =
  | "message_dm"      // Direct message received
  | "message_mention" // Message with @mention
  | "message_channel" // Regular channel message (when enabled)
  | "message_thread"  // Thread reply received
  | "call_incoming"   // Incoming call (future)
  | "user_join"       // User joined voice (future)
  | "user_leave";     // User left voice (future)

/**
 * Mention types that can appear in messages.
 */
export type MentionType = "direct" | "everyone" | "here" | null;

/**
 * Sound event to be processed by the SoundService.
 */
export interface SoundEvent {
  /** Type of event */
  type: SoundEventType;
  /** Channel ID where the event occurred */
  channelId: string;
  /** Whether this is a DM channel */
  isDm: boolean;
  /** Mention type if applicable */
  mentionType?: MentionType;
  /** User ID of the message author (to skip own messages) */
  authorId?: string;
}

/**
 * Sound metadata for UI display.
 */
export interface SoundInfo {
  id: string;
  name: string;
  description: string;
  filename: string;
}

/**
 * Available notification sounds.
 */
export const AVAILABLE_SOUNDS: SoundInfo[] = [
  {
    id: "default",
    name: "Default",
    description: "Clean, neutral notification chime",
    filename: "default.wav",
  },
  {
    id: "subtle",
    name: "Subtle",
    description: "Soft, minimal tone",
    filename: "subtle.wav",
  },
  {
    id: "ping",
    name: "Ping",
    description: "Classic ping sound",
    filename: "ping.wav",
  },
  {
    id: "chime",
    name: "Chime",
    description: "Melodic chime",
    filename: "chime.wav",
  },
  {
    id: "bell",
    name: "Bell",
    description: "Soft bell notification",
    filename: "bell.wav",
  },
];

/**
 * Get sound info by ID.
 */
export function getSoundInfo(id: string): SoundInfo | undefined {
  return AVAILABLE_SOUNDS.find((s) => s.id === id);
}

/**
 * Get sound file path by ID.
 */
export function getSoundPath(id: string): string {
  const sound = getSoundInfo(id);
  const filename = sound ? sound.filename : "default.wav";
  return "/sounds/" + filename;
}
