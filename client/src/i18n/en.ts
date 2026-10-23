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
  "qr.title": "Link Mobile Device",
  "qr.instructions": "Scan this code with the Kaiku mobile app to sign in",
  "qr.alt": "QR login code",
  "qr.expiresIn": "Expires in {{seconds}}s",
  "qr.expired": "Code expired",
  "qr.generateNew": "Generate new code",
  "qr.tryAgain": "Try again",
  "qr.noServer": "Can't create a login code because the server address is unknown. Sign out and sign in again.",
  "qr.failedTitle": "QR Code Failed",
  "qr.failedMessage": "Could not generate QR login code.",
  "chat.encryptedPlaceholder": "Encrypted message — it can't be shown on this device.",
  "dm.header.selectConversation": "Select a conversation",
  "dm.header.unknownUser": "Unknown",
  "dm.header.changeIcon": "Change icon (Max {{limit}})",
  "dm.header.iconAlt": "DM Icon",
  "dm.header.uploadIconFailed": "Failed to upload icon",
  "dm.header.clickToRename": "Click to rename",
  "dm.header.renameFailed": "Could not rename conversation. Please try again.",
  "dm.header.members": "{{count}} members",
  "dm.header.startCall": "Start voice call",
  "dm.header.callInProgress": "Call in progress",
} as const;

export type MessageKey = keyof typeof en;
