import { flatten, resolveTemplate, translator } from "@solid-primitives/i18n";
import { en, type MessageKey } from "./en";

export type { MessageKey } from "./en";

type Params = Record<string, string | number>;

// Locale is fixed to "en" for v1.0; the dict accessor is where a locale signal goes later.
const dict = flatten(en) as Record<string, string>;
const translate = translator(() => dict, resolveTemplate) as unknown as (
  key: string,
  params?: Params,
) => string;

/** Look up a message and fill `{{name}}` placeholders. */
export function t(key: MessageKey, params?: Params): string {
  return translate(key, params);
}

const pluralRules = new Intl.PluralRules("en");

/** Pick the `one`/`other` key for `count`; `count` is passed as a param. */
export function plural(
  count: number,
  forms: { one: MessageKey; other: MessageKey },
  params?: Params,
): string {
  const category = pluralRules.select(count);
  return t(category === "one" ? forms.one : forms.other, { ...params, count });
}
