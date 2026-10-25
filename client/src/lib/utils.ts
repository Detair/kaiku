/**
 * Utility Functions
 */

/**
 * Get the display name for a voice participant with fallback chain.
 */
export function getParticipantDisplayName(participant: {
  user_id: string;
  display_name?: string | null;
  username?: string | null;
}): string {
  return participant.display_name || participant.username || participant.user_id.slice(0, 8);
}

export { formatRelativeTime, formatRelativeTimeShort, formatTimestamp } from "@/i18n/format";

/**
 * Get initials from a display name.
 */
export function getInitials(name: string): string {
  return name
    .split(" ")
    .map((n) => n[0])
    .join("")
    .toUpperCase()
    .slice(0, 2);
}

/**
 * Truncate text to a maximum length.
 */
export function truncate(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text;
  return text.slice(0, maxLength - 3) + "...";
}

/**
 * Check if two messages are from the same author and close in time (for grouping).
 */
export function shouldGroupWithPrevious(
  currentTimestamp: string,
  previousTimestamp: string,
  currentAuthorId: string,
  previousAuthorId: string,
): boolean {
  if (currentAuthorId !== previousAuthorId) return false;

  const current = new Date(currentTimestamp);
  const previous = new Date(previousTimestamp);
  const diffMs = current.getTime() - previous.getTime();
  const diffMins = diffMs / (1000 * 60);

  // Group if within 5 minutes
  return diffMins < 5;
}

/**
 * Format elapsed time in MM:SS format.
 * Used for voice connection duration timers.
 *
 * @param startTime - Unix timestamp in milliseconds
 * @returns Formatted time string (e.g., "03:45")
 *
 * @example
 * const start = Date.now();
 * // After 125 seconds...
 * formatElapsedTime(start); // "02:05"
 */
export function formatElapsedTime(startTime: number): string {
  const elapsed = Math.floor((Date.now() - startTime) / 1000);
  const minutes = Math.floor(elapsed / 60);
  const seconds = elapsed % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}
