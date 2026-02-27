/**
 * Sound Settings Store
 *
 * Manages notification sound preferences through the unified preferences store.
 * Sound settings are synced across devices through the preferences system.
 */

import {
  preferences,
  updateNestedPreference,
  getChannelNotificationLevel as getChannelNotifLevel,
  setChannelNotificationLevel as setChannelNotifLevel,
  isInQuietHours,
} from "./preferences";
import { currentUser } from "./auth";

// ============================================================================
// Types
// ============================================================================

export type SoundOption = "default" | "subtle" | "ping" | "chime" | "bell";
export type NotificationLevel = "all" | "mentions" | "none";

export interface QuietHoursSettings {
  /** Whether quiet hours are enabled */
  enabled: boolean;
  /** Start time in 24h format (e.g., "22:00") */
  startTime: string;
  /** End time in 24h format (e.g., "08:00") */
  endTime: string;
}

export interface SoundSettings {
  /** Master on/off for notification sounds */
  enabled: boolean;
  /** Volume level 0-100 */
  volume: number;
  /** Selected notification sound */
  selectedSound: SoundOption;
  /** Quiet hours / Do Not Disturb settings */
  quietHours: QuietHoursSettings;
}

export interface ChannelNotificationSettings {
  [channelId: string]: NotificationLevel;
}

// ============================================================================
// Derived Signals
// ============================================================================

/**
 * Get sound settings from preferences.
 * Maps preferences sound structure to the SoundSettings interface.
 */
export const soundSettings = (): SoundSettings => {
  const sound = preferences().sound;
  return {
    enabled: sound.enabled,
    volume: sound.volume,
    selectedSound: sound.soundType,
    quietHours: sound.quietHours,
  };
};

/**
 * Get channel notification settings from preferences.
 * Maps "muted" to "none" for backwards compatibility.
 */
export const channelNotificationSettings = (): ChannelNotificationSettings => {
  const channelNotifs = preferences().channelNotifications;
  const result: ChannelNotificationSettings = {};

  for (const [channelId, level] of Object.entries(channelNotifs)) {
    // Map "muted" from preferences to "none" for this store's interface
    result[channelId] = level === "muted" ? "none" : level;
  }

  return result;
};

// ============================================================================
// Sound Settings Functions
// ============================================================================

export function getSoundEnabled(): boolean {
  return preferences().sound.enabled;
}

export function setSoundEnabled(enabled: boolean): void {
  updateNestedPreference("sound", "enabled", enabled);
}

export function getSoundVolume(): number {
  return preferences().sound.volume;
}

export function setSoundVolume(volume: number): void {
  const clamped = Math.max(0, Math.min(100, volume));
  updateNestedPreference("sound", "volume", clamped);
}

export function getSelectedSound(): SoundOption {
  return preferences().sound.soundType;
}

export function setSelectedSound(sound: SoundOption): void {
  updateNestedPreference("sound", "soundType", sound);
}

// ============================================================================
// Quiet Hours Functions
// ============================================================================

export { isInQuietHours };

// Alias for backwards compatibility
export const isWithinQuietHours = isInQuietHours;

export function getQuietHoursEnabled(): boolean {
  return preferences().sound.quietHours.enabled;
}

export function setQuietHoursEnabled(enabled: boolean): void {
  const currentQuietHours = preferences().sound.quietHours;
  updateNestedPreference("sound", "quietHours", {
    ...currentQuietHours,
    enabled,
  });
}

export function getQuietHoursSchedule(): {
  startTime: string;
  endTime: string;
} {
  const { startTime, endTime } = preferences().sound.quietHours;
  return { startTime, endTime };
}

export function setQuietHoursSchedule(
  startTime: string,
  endTime: string,
): void {
  const currentQuietHours = preferences().sound.quietHours;
  updateNestedPreference("sound", "quietHours", {
    ...currentQuietHours,
    startTime,
    endTime,
  });
}

export function getQuietHours(): QuietHoursSettings {
  return soundSettings().quietHours;
}

export function setQuietHoursTime(startTime: string, endTime: string): void {
  setQuietHoursSchedule(startTime, endTime);
}

/**
 * Check if Do Not Disturb is active.
 * DND is active when:
 * - User status is "dnd"
 * - Quiet hours are currently active
 */
export function isDndActive(): boolean {
  const user = currentUser();
  if (user?.status === "dnd") return true;
  return isInQuietHours();
}

// ============================================================================
// Channel Notification Functions
// ============================================================================

/**
 * Get notification level for a channel.
 * Default is "mentions" for channels, "all" for DMs.
 *
 * Note: This maps "muted" from preferences to "none" for backwards compatibility.
 */
export function getChannelNotificationLevel(
  channelId: string,
  isDm: boolean = false,
): NotificationLevel {
  const level = getChannelNotifLevel(channelId);
  // Map "muted" to "none" for this store's interface
  if (level === "muted") return "none";
  // Handle DM default
  if (level === "mentions" && isDm) {
    // Check if there's actually a stored value or if it's the default
    const stored = preferences().channelNotifications[channelId];
    if (!stored) return "all"; // DM default
  }
  return level;
}

/**
 * Set notification level for a channel.
 *
 * Note: This maps "none" to "muted" for the preferences store.
 */
export function setChannelNotificationLevel(
  channelId: string,
  level: NotificationLevel,
): void {
  // Map "none" to "muted" for the preferences store
  const prefsLevel = level === "none" ? "muted" : level;
  setChannelNotifLevel(channelId, prefsLevel);
}

/**
 * Check if a channel is muted (notification level = "none").
 */
export function isChannelMuted(channelId: string): boolean {
  return getChannelNotificationLevel(channelId) === "none";
}

// ============================================================================
// Sound Playback Helpers (not persisted, just utilities)
// ============================================================================

/**
 * Check if sound should play based on current settings.
 * Considers enabled state, quiet hours, and channel muting.
 */
export function shouldPlaySound(channelId?: string): boolean {
  // Check if sounds are globally enabled
  if (!getSoundEnabled()) return false;

  // Check quiet hours
  if (isInQuietHours()) return false;

  // Check channel-specific muting
  if (channelId && isChannelMuted(channelId)) return false;

  return true;
}
