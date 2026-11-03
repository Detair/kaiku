/**
 * Total lookups of server enum values in literal text maps (Fable review
 * M2, C5): a value the client predates (an appended C28 event type, action,
 * state or reason) and prototype names like `toString` give the fallback,
 * never `undefined` (which makes the translator throw) or a function.
 */

import type { MessageKey } from "@/i18n";

export function totalKey(map: Readonly<Partial<Record<string, MessageKey>>>, value: unknown, fallback: MessageKey): MessageKey {
  if (typeof value !== "string" || !Object.prototype.hasOwnProperty.call(map, value)) return fallback;
  return map[value] ?? fallback;
}
