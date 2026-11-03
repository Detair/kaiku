/**
 * English catalog the app loads at startup: every area except the ones
 * `locale.ts` loads on demand (`LAZY_AREAS`, kept off the startup path for
 * the bundle budget). `en.ts` adds those back for the `MessageKey` type, the
 * i18n check and the tests. A new area is a new file plus one spread line
 * here (or in `en.ts` and `LAZY_AREAS` for an on-demand area).
 */
import { common } from "./en/common";
import { connection } from "./en/connection";
import { errors } from "./en/errors";
import { auth } from "./en/auth";
import { channels } from "./en/channels";
import { chat } from "./en/chat";
import { dm } from "./en/dm";
import { home } from "./en/home";
import { time } from "./en/time";
import { admin } from "./en/admin";
import { settings } from "./en/settings";
import { onboarding } from "./en/onboarding";
import { voice } from "./en/voice";
import { notifications } from "./en/notifications";
import { guilds } from "./en/guilds";
import { passkeys } from "./en/passkeys";

export const enCore = {
  ...common,
  ...connection,
  ...errors,
  ...auth,
  ...channels,
  ...chat,
  ...dm,
  ...home,
  ...time,
  ...admin,
  ...settings,
  ...onboarding,
  ...voice,
  ...notifications,
  ...guilds,
  ...passkeys,
} as const;
