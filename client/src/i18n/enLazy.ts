/**
 * English areas loaded before the first render but outside the startup
 * chunk (bundle budget): `locale.ts` imports this one module (one request),
 * `StartupGate` shows a reload if it cannot. `en.ts` adds them to the full
 * catalog for the `MessageKey` type, the i18n check and the tests.
 */
import { admin } from "./en/admin";
import { notifications } from "./en/notifications";
import { onboarding } from "./en/onboarding";
import { security } from "./en/security";
import { settings } from "./en/settings";
import { voice } from "./en/voice";

export const enLazy = {
  ...admin,
  ...settings,
  ...onboarding,
  ...voice,
  ...notifications,
  ...security,
} as const;
