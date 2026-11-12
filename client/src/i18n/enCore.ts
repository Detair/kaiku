/**
 * English catalog in the startup chunk: every area except the ones in
 * `enLazy.ts`, which `locale.ts` loads before the first render (one chunk,
 * kept out of the initial payload for the bundle budget). `en.ts` joins both
 * for the `MessageKey` type, the i18n check and the tests. A new area is a
 * new file plus one spread line here or in `enLazy.ts`.
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
import { guilds } from "./en/guilds";
import { passkeys } from "./en/passkeys";
import { moderation } from "./en/moderation";

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
  ...guilds,
  ...passkeys,
  ...moderation,
} as const;
