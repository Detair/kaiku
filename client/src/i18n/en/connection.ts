export const connection = {
  "connection.attempt.one": "Attempt {{count}}",
  "connection.attempt.other": "Attempt {{count}}",
  "connection.reconnecting": "Reconnecting…",
  "connection.offline": "You're offline. New messages will appear when you're back online.",
  "connection.retryNow": "Retry now",
  "connection.session.expiredTitle": "Session Expired",
  "connection.session.expired": "Your session has expired. Please log in again to continue.",
  "connection.session.endedTitle": "Signed out",
  "connection.session.revoked": "Your session was ended on another device or by an admin. Please log in again to continue.",
  "connection.session.banned": "This account has been suspended, so you were signed out.",
  "connection.session.logIn": "Log in",
} as const satisfies Record<`connection.${string}`, string>;
