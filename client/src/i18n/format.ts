/**
 * Date, time, number and list formatting via `Intl`.
 *
 * Numbers (clock time, day/month order) follow the system region unless the
 * user explicitly picked a language; words (relative time, "Yesterday", list
 * joins) follow the UI language. Never hardcode a locale such as "en-US".
 */
import { t } from "./index";
import { getLocale, hasExplicitLocale, type Locale } from "./locale";

type Locales = readonly string[] | undefined;

/**
 * Locales for number-like formatting. `undefined` (runtime default = system
 * region) when no language was chosen; otherwise the system entry that matches
 * the chosen language (e.g. "de-AT"), else the bare language.
 */
export function formatLocales(
  explicit: Locale | null = hasExplicitLocale() ? getLocale() : null,
  system: readonly string[] = typeof navigator === "undefined" ? [] : navigator.languages,
): string[] | undefined {
  if (explicit === null) return undefined;
  const match = system.find((l) => l.split("-")[0].toLowerCase() === explicit);
  return [match ?? explicit];
}

const dateTimeCache = new Map<string, Intl.DateTimeFormat>();
const relativeCache = new Map<string, Intl.RelativeTimeFormat>();
const listCache = new Map<string, Intl.ListFormat>();
const numberCache = new Map<string, Intl.NumberFormat>();

function cached<T>(cache: Map<string, T>, locales: Locales, options: object, make: () => T): T {
  const key = JSON.stringify([locales ?? null, options]);
  let value = cache.get(key);
  if (!value) {
    value = make();
    cache.set(key, value);
  }
  return value;
}

function dateTime(
  date: Date | string,
  locales: Locales,
  options: Intl.DateTimeFormatOptions,
): string {
  const fmt = cached(
    dateTimeCache,
    locales,
    options,
    () => new Intl.DateTimeFormat(locales as string[] | undefined, options),
  );
  return fmt.format(typeof date === "string" ? new Date(date) : date);
}

const TIME_OPTIONS: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };

/** Clock time, e.g. "14:30" (German PC) or "02:30 PM" (US). */
export function formatTime(date: Date | string, locales: Locales = formatLocales()): string {
  return dateTime(date, locales, TIME_OPTIONS);
}

/** "Oct 23" / "23. Okt."; the year is added when it differs from `now`'s. */
export function formatShortDate(
  date: Date | string,
  now: Date = new Date(),
  locales: Locales = formatLocales(),
): string {
  const d = typeof date === "string" ? new Date(date) : date;
  return dateTime(d, locales, {
    month: "short",
    day: "numeric",
    year: d.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
  });
}

/** Time for today, "Yesterday {time}", otherwise short date plus time. */
export function formatTimestamp(iso: string, now: Date = new Date()): string {
  const date = new Date(iso);
  if (date.toDateString() === now.toDateString()) return formatTime(date);

  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) {
    return t("time.yesterdayAt", { time: formatTime(date) });
  }

  return dateTime(date, formatLocales(), {
    month: "short",
    day: "numeric",
    year: date.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
    ...TIME_OPTIONS,
  });
}

function relative(n: number, unit: Intl.RelativeTimeFormatUnit, style: "long" | "narrow"): string {
  const locale = getLocale();
  const options = { numeric: "always", style } as const;
  const fmt = cached(
    relativeCache,
    [locale],
    options,
    () => new Intl.RelativeTimeFormat(locale, options),
  );
  return fmt.format(-n, unit);
}

function relativeParts(iso: string, now: Date) {
  const diffSecs = Math.floor((now.getTime() - new Date(iso).getTime()) / 1000);
  const mins = Math.floor(diffSecs / 60);
  const hours = Math.floor(mins / 60);
  return { secs: diffSecs, mins, hours, days: Math.floor(hours / 24) };
}

/** "5 minutes ago" up to 6 days, then `formatTimestamp`. */
export function formatRelativeTime(iso: string, now: Date = new Date()): string {
  const { secs, mins, hours, days } = relativeParts(iso, now);
  if (secs < 60) return t("time.justNow");
  if (mins < 60) return relative(mins, "minute", "long");
  if (hours < 24) return relative(hours, "hour", "long");
  if (days < 7) return relative(days, "day", "long");
  return formatTimestamp(iso, now);
}

/** Compact "5m ago" / "3h ago" / "2d ago", no 7-day cap. */
export function formatRelativeTimeShort(iso: string, now: Date = new Date()): string {
  const { mins, hours, days } = relativeParts(iso, now);
  if (mins < 1) return t("time.justNow");
  if (mins < 60) return relative(mins, "minute", "narrow");
  if (hours < 24) return relative(hours, "hour", "narrow");
  return relative(days, "day", "narrow");
}

/** "A, B, and C" / "A, B und C" in the UI language. */
export function formatList(
  items: string[],
  type: "conjunction" | "disjunction" = "conjunction",
): string {
  const locale = getLocale();
  const options = { type } as const;
  const fmt = cached(listCache, [locale], options, () => new Intl.ListFormat(locale, options));
  return fmt.format(items);
}

/** Locale-aware number, e.g. "1.234,5" on a German PC. */
export function formatNumber(n: number, locales: Locales = formatLocales()): string {
  const fmt = cached(
    numberCache,
    locales,
    {},
    () => new Intl.NumberFormat(locales as string[] | undefined),
  );
  return fmt.format(n);
}
