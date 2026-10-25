/**
 * Locale runtime: the active locale and the dictionary `t()` reads.
 *
 * English is the source (`en/<area>.ts`); other locales are flat JSON files in
 * `locales/`, loaded lazily. Empty or missing values fall back to English per
 * key, so a partial translation never shows raw keys (contract C17). The
 * picker and the stored choice only count when `VITE_FEATURE_LOCALE_SWITCH`
 * is on; otherwise the app always runs in English.
 */
import { createSignal } from "solid-js";
import { isLocaleSwitchEnabled } from "@/lib/featureFlags";
import { en } from "./en";

export type Locale = "en" | "de";
export const LOCALES: readonly Locale[] = ["en", "de"];
export const DEFAULT_LOCALE: Locale = "en";
/** Endonyms: each language is named in itself, never translated. */
export const LOCALE_NAMES: Record<Locale, string> = { en: "English", de: "Deutsch" };
export const LOCALE_STORAGE_KEY = "kaiku.locale";

type Dict = Record<string, string>;

const enDict: Dict = { ...en };

// `en` has no JSON: the TS modules are its source.
const loaders = import.meta.glob<{ default: unknown }>("./locales/*.json");

const [locale, setLocaleSignal] = createSignal<Locale>(DEFAULT_LOCALE);
const [dict, setDict] = createSignal<Dict>(enDict);
const [explicit, setExplicit] = createSignal(false);

// Guards against an older, slower load overwriting a newer choice.
let requestId = 0;

/** The active locale (reactive). */
export function getLocale(): Locale {
  return locale();
}

/** True when the active locale comes from the user's own choice (reactive). */
export function hasExplicitLocale(): boolean {
  return explicit();
}

/** The dictionary `t()` resolves against (reactive). Internal to `@/i18n`. */
export function activeDict(): Dict {
  return dict();
}

function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (LOCALES as readonly string[]).includes(value);
}

function readStored(): string | null {
  try {
    return localStorage.getItem(LOCALE_STORAGE_KEY);
  } catch {
    return null;
  }
}

function writeStored(value: Locale): void {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, value);
  } catch {
    // Storage blocked or full: the choice still applies for this session.
  }
}

function setHtmlLang(value: Locale): void {
  if (typeof document !== "undefined") document.documentElement.lang = value;
}

/** Load `locales/<locale>.json` and merge its non-empty, known keys over English. */
async function loadDict(target: Exclude<Locale, "en">): Promise<Dict> {
  const loader = loaders[`./locales/${target}.json`];
  if (!loader) throw new Error(`no catalog for locale "${target}"`);
  const data = (await loader()).default;
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw new Error(`catalog for "${target}" is not an object`);
  }
  const merged: Dict = { ...enDict };
  for (const [key, value] of Object.entries(data)) {
    if (typeof value !== "string") {
      throw new Error(`catalog for "${target}" has a non-string value at "${key}"`);
    }
    if (value !== "" && key in enDict) merged[key] = value;
  }
  return merged;
}

function apply(target: Locale, next: Dict, isExplicit: boolean): void {
  setDict(next);
  setLocaleSignal(target);
  setExplicit(isExplicit);
  setHtmlLang(target);
}

/**
 * Switch to `target`, or stay on English if its catalog can't be loaded.
 * Returns whether `target` became active.
 */
async function activate(target: Locale, isExplicit: boolean): Promise<boolean> {
  const id = ++requestId;
  if (target === "en") {
    apply("en", enDict, isExplicit);
    return true;
  }
  try {
    const next = await loadDict(target);
    if (id !== requestId) return false;
    apply(target, next, isExplicit);
    return true;
  } catch (err) {
    console.warn(`[i18n] could not load locale "${target}", using English`, err);
    if (id === requestId) apply("en", enDict, false);
    return false;
  }
}

/** Switch the UI language and remember the choice. Never rejects. */
export async function setLocale(target: Locale): Promise<void> {
  if (!isLocale(target)) {
    console.warn(`[i18n] unknown locale "${String(target)}", using English`);
    await activate("en", false);
    return;
  }
  if (await activate(target, true)) writeStored(target);
}

/**
 * Pick the startup locale. Flag off: English, and the stored value is left
 * alone. Flag on: the stored value if it is a known locale, else English.
 * The browser language is not auto-detected in v1.0. Never rejects.
 */
export async function initLocale(): Promise<void> {
  const stored = isLocaleSwitchEnabled() ? readStored() : null;
  if (isLocale(stored)) {
    await activate(stored, true);
  } else {
    await activate(DEFAULT_LOCALE, false);
  }
}
