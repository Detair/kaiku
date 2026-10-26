export const errors = {
  "errors.rateLimited": "Too many attempts. Please wait a moment and try again.",
  "errors.server": "Something went wrong on the server. Please try again in a moment.",
  "errors.featureDisabled": "This feature is turned off on this server.",
  "errors.network": "Can't reach the server. Check your connection.",
} as const satisfies Record<`errors.${string}`, string>;
