import { resolveTemplate, translator } from "@solid-primitives/i18n";
import type { MessageKey } from "./en";
import { activeDict, getLocale, type Locale } from "./locale";

export type { MessageKey } from "./en";
export {
  DEFAULT_LOCALE,
  LOCALE_NAMES,
  LOCALES,
  getLocale,
  hasExplicitLocale,
  initLocale,
  setLocale,
  type Locale,
} from "./locale";

type Params = Record<string, string | number>;

// Reads the locale's dictionary signal, so `t()` in JSX re-renders on setLocale.
const translate = translator(activeDict, resolveTemplate) as unknown as (
  key: string,
  params?: Params,
) => string;

/** Look up a message in the active locale and fill `{{name}}` placeholders. */
export function t(key: MessageKey, params?: Params): string {
  return translate(key, params);
}

/** Plural keys per CLDR category; `other` is required, the rest fall back to it. */
export type PluralForms = { other: MessageKey } & Partial<
  Record<Exclude<Intl.LDMLPluralRule, "other">, MessageKey>
>;

const pluralRulesCache = new Map<Locale, Intl.PluralRules>();

function pluralRules(locale: Locale): Intl.PluralRules {
  let rules = pluralRulesCache.get(locale);
  if (!rules) {
    rules = new Intl.PluralRules(locale);
    pluralRulesCache.set(locale, rules);
  }
  return rules;
}

/** Pick the key for `count`'s plural category in the active locale; `count` is passed as a param. */
export function plural(count: number, forms: PluralForms, params?: Params): string {
  const category = pluralRules(getLocale()).select(count);
  return t(forms[category] ?? forms.other, { ...params, count });
}
