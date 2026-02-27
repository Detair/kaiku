// Sentry browser wrapper for Tauri webview
// Only active when running inside Tauri (window.__TAURI__ present) and VITE_SENTRY_DSN set.
import * as Sentry from "@sentry/browser";

export function initSentry(): void {
  const dsn = import.meta.env.VITE_SENTRY_DSN as string | undefined;
  if (!dsn || !("__TAURI__" in window)) return;

  Sentry.init({
    dsn,
    release: import.meta.env.VITE_APP_VERSION as string | undefined,
    environment: import.meta.env.VITE_APP_ENV ?? "development",
    tracesSampleRate: 0.05,
    sendDefaultPii: false,
    beforeSend(event) {
      // Strip query params that might contain tokens
      if (event.request?.url) {
        event.request.url = event.request.url.split("?")[0];
      }
      return event;
    },
    ignoreErrors: [
      "ResizeObserver loop limit exceeded",
      "Non-Error promise rejection captured",
    ],
  });
}
