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

/**
 * Format a timestamp as a compact relative time ("just now", "5m ago", "3h ago",
 * "2d ago"). Used by settings lists (sessions, linked accounts) where the longer
 * `formatRelativeTime` ("5 minutes ago") would be too wide.
 */
export function formatRelativeTimeShort(isoString: string): string {
  const diffMs = Date.now() - new Date(isoString).getTime();
  const diffMins = Math.floor(diffMs / 60000);
  if (diffMins < 1) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
}

/**
 * Format a timestamp for display.
 * Shows time only for today, date and time for older messages.
 */
export function formatTimestamp(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();

  if (isToday) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const isYesterday = date.toDateString() === yesterday.toDateString();

  if (isYesterday) {
    return `Yesterday ${date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  }

  return date.toLocaleDateString([], {
    month: "short",
    day: "numeric",
    year: date.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Format a relative time (e.g., "2 minutes ago").
 */
export function formatRelativeTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSecs < 60) {
    return "just now";
  } else if (diffMins < 60) {
    return `${diffMins} minute${diffMins === 1 ? "" : "s"} ago`;
  } else if (diffHours < 24) {
    return `${diffHours} hour${diffHours === 1 ? "" : "s"} ago`;
  } else if (diffDays < 7) {
    return `${diffDays} day${diffDays === 1 ? "" : "s"} ago`;
  } else {
    return formatTimestamp(isoString);
  }
}

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
