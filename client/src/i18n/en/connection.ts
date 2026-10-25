export const connection = {
  "connection.attempt.one": "Attempt {{count}}",
  "connection.attempt.other": "Attempt {{count}}",
  "connection.reconnecting": "Reconnecting…",
  "connection.offline": "You're offline. New messages will appear when you're back online.",
  "connection.retryNow": "Retry now",
  "connection.realtime.unavailable": "Real-time messaging temporarily unavailable. Reconnecting...",
} as const satisfies Record<`connection.${string}`, string>;
