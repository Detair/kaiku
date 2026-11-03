/**
 * English message catalog, aggregated from one file per area (`en/<area>.ts`).
 *
 * Key convention (contract C15): `area.screen.purpose`, whole sentences per
 * key, `{{name}}` placeholders, plural forms as sibling `….one`/`….other`
 * keys. The first segment is the area, and only `en/<area>.ts` may hold keys
 * for it; a new area is a new file plus one spread line in `enCore.ts`. The
 * Android client mirrors keys in strings.xml with `.` -> `_`, camelCase ->
 * snake_case.
 *
 * The app never imports this at runtime: `locale.ts` starts from `enCore`
 * and loads the remaining areas (`security`) on demand. This full catalog is
 * for the `MessageKey` type, `check-i18n` and the tests.
 */
import { enCore } from "./enCore";
import { security } from "./en/security";

export const en = {
  ...enCore,
  ...security,
} as const;

export type MessageKey = keyof typeof en;
