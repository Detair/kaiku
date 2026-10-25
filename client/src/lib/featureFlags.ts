/**
 * Build-time feature flags.
 *
 * No user toggle: a flag is fixed when the client is built.
 */

/**
 * End-to-end encryption (setup prompt, recovery-key backup, DM padlock).
 * Off for v1.0; it returns post-1.0 together with Android support.
 * Enabled only by building with `VITE_FEATURE_E2EE=true`.
 */
export function isE2EEEnabled(): boolean {
  return import.meta.env.VITE_FEATURE_E2EE === "true";
}

/**
 * Language picker in Appearance settings, and the stored language choice.
 * Off for v1.0 until a translation is complete (contract C17).
 * Enabled only by building with `VITE_FEATURE_LOCALE_SWITCH=true`.
 */
export function isLocaleSwitchEnabled(): boolean {
  return import.meta.env.VITE_FEATURE_LOCALE_SWITCH === "true";
}
