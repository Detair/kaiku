/**
 * English message catalog, aggregated from one file per area (`en/<area>.ts`).
 *
 * Key convention (contract C15): `area.screen.purpose`, whole sentences per
 * key, `{{name}}` placeholders, plural forms as sibling `….one`/`….other`
 * keys. The first segment is the area, and only `en/<area>.ts` may hold keys
 * for it; a new area is a new file plus one spread line here. The Android
 * client mirrors keys in strings.xml with `.` -> `_`, camelCase -> snake_case.
 */
import { common } from "./en/common";
import { connection } from "./en/connection";
import { errors } from "./en/errors";
import { auth } from "./en/auth";
import { qr } from "./en/qr";
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

export const en = {
  ...common,
  ...connection,
  ...errors,
  ...auth,
  ...qr,
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
} as const;

export type MessageKey = keyof typeof en;
