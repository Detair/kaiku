/**
 * English message catalog (the only locale for v1.0).
 *
 * Key convention: `area.screen.purpose`, whole sentences per key, `{{name}}`
 * placeholders. The Android client mirrors keys in strings.xml with `.` -> `_`.
 */
export const en = {
  "common.cancel": "Cancel",
  "common.close": "Close",
  "common.save": "Save",
  "common.retry": "Retry",
  "common.copy": "Copy",
  "common.copied": "Copied",
  "connection.attempt.one": "Attempt {{count}}",
  "connection.attempt.other": "Attempt {{count}}",
  "connection.reconnecting": "Reconnecting…",
  "connection.offline": "You're offline. New messages will appear when you're back online.",
  "connection.retryNow": "Retry now",
} as const;

export type MessageKey = keyof typeof en;
