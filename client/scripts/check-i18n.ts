/**
 * check-i18n.ts
 *
 * CI check and sync tool for the message catalogs (contract C15/C17).
 *
 * Checks: key format, Android-name collisions, locale JSON key sets and
 * `{{placeholder}}` parity, unused keys, and the Android `values*` folders
 * (valid names, locale names known, each locale folder empty or complete).
 * Warns (without failing) when the same English text sits under 3+ keys.
 *
 * Run: bun run i18n:check            (exit 1 on any error, one per line)
 *      bun run i18n:sync             (rewrite locales/*.json from en)
 *      bun run i18n:sync -- --add fr (create an empty locales/fr.json)
 */

import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { JSDOM } from "jsdom";
import { en } from "../src/i18n/en";

type Catalog = Record<string, string>;
export interface AndroidStrings {
  strings: string[];
  plurals: string[];
}

const KEY_RE = /^[a-z][a-zA-Z0-9]*(\.[a-zA-Z0-9]+)+$/;
const ANDROID_NAME_RE = /^[a-z][a-z0-9_]*$/;
// C15: segments are lower camelCase, so a segment must not start uppercase.
const SEGMENT_RE = /^[a-z][a-zA-Z0-9]*$/;

export function checkKeyFormat(keys: string[]): string[] {
  return keys
    .filter((key) => !(KEY_RE.test(key) && key.split(".").every((s) => SEGMENT_RE.test(s))))
    .map((key) => `key "${key}" must look like area.screen.purpose (lower camelCase segments)`);
}

export function toAndroidName(webKey: string): string {
  return webKey.replace(/\./g, "_").replace(/([A-Z])/g, "_$1").toLowerCase();
}

export function checkAndroidNameCollisions(keys: string[]): string[] {
  const byName = new Map<string, string[]>();
  for (const key of keys) {
    const name = toAndroidName(key);
    byName.set(name, [...(byName.get(name) ?? []), key]);
  }
  return [...byName]
    .filter(([, group]) => group.length > 1)
    .map(([name, group]) => `Android name collision on ${name}: ${group.join(", ")}`);
}

export function checkLocaleKeySets(
  source: Catalog,
  locales: Record<string, Catalog>,
): string[] {
  const errors: string[] = [];
  for (const [lang, catalog] of Object.entries(locales)) {
    for (const key of Object.keys(source)) {
      if (!(key in catalog)) errors.push(`${lang}: missing key ${key}`);
    }
    for (const key of Object.keys(catalog)) {
      if (!(key in source)) errors.push(`${lang}: unknown key ${key}`);
    }
  }
  return errors;
}

function placeholders(text: string): string[] {
  return [...new Set([...text.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1]))].sort();
}

export function checkPlaceholders(source: Catalog, locales: Record<string, Catalog>): string[] {
  const errors: string[] = [];
  for (const [lang, catalog] of Object.entries(locales)) {
    for (const [key, value] of Object.entries(catalog)) {
      if (value === "" || !(key in source)) continue;
      const want = placeholders(source[key]).join(", ");
      const got = placeholders(value).join(", ");
      if (want !== got) {
        errors.push(`${lang}: ${key} placeholders {${got}} do not match en {${want}}`);
      }
    }
  }
  return errors;
}

/**
 * A key is used when `"key"`, `'key'` or `` `key` `` occurs in a source file.
 * Callers pass only files that count as usage (not the catalogs or tests).
 */
export function findUnusedKeys(keys: string[], sources: Map<string, string>): string[] {
  const counted = [...sources]
    .map(([file, text]) => [file.replaceAll("\\", "/"), text] as const)
    .filter(
      ([file]) =>
        !/(^|\/)src\/i18n\/(en|locales)\//.test(file) && !/(^|\/)__tests__\//.test(file),
    )
    .map(([, text]) => text);
  return keys.filter(
    (key) => !counted.some((text) => ["\"", "'", "`"].some((q) => text.includes(`${q}${key}${q}`))),
  );
}

/**
 * Warning only: English text that appears under `min` or more keys (plural
 * siblings count once). Candidates for a shared `common.*` key.
 */
export function findDuplicateValues(source: Catalog, min = 3): string[] {
  const byValue = new Map<string, Set<string>>();
  for (const [key, value] of Object.entries(source)) {
    const base = key.replace(/\.(zero|one|two|few|many|other)$/, "");
    byValue.set(value, (byValue.get(value) ?? new Set()).add(base));
  }
  return [...byValue]
    .filter(([, bases]) => bases.size >= min)
    .map(([value, bases]) => `duplicate text ${JSON.stringify(value)} under ${bases.size} keys: ${[...bases].join(", ")}`);
}

/** Throws on malformed XML (jsdom returns a parsererror document instead). */
export function parseAndroidStrings(xml: string): AndroidStrings {
  const doc = new (new JSDOM("").window.DOMParser)().parseFromString(xml, "text/xml");
  if (doc.getElementsByTagName("parsererror").length > 0 || doc.documentElement?.nodeName !== "resources") {
    throw new Error("malformed XML (expected a <resources> document)");
  }
  const names = (tag: string) =>
    [...doc.getElementsByTagName(tag)]
      .filter((el) => el.getAttribute("translatable") !== "false")
      .map((el) => el.getAttribute("name") ?? "");
  return { strings: names("string"), plurals: names("plurals") };
}

export function checkAndroidLocales(
  def: AndroidStrings,
  locales: Record<string, AndroidStrings>,
): string[] {
  const errors: string[] = [];
  const badName = (n: string) => !ANDROID_NAME_RE.test(n);
  for (const name of [...def.strings, ...def.plurals].filter(badName)) {
    errors.push(`android values: invalid name "${name}" (use ${ANDROID_NAME_RE})`);
  }
  for (const [lang, loc] of Object.entries(locales)) {
    const known = [...def.strings, ...def.plurals];
    for (const name of [...loc.strings, ...loc.plurals]) {
      if (!known.includes(name)) errors.push(`android ${lang}: unknown name ${name}`);
    }
    if (loc.strings.length === 0 && loc.plurals.length === 0) continue; // empty is fine
    for (const name of def.strings.filter((n) => !loc.strings.includes(n))) {
      errors.push(`android ${lang}: missing string ${name} (locale folders must be empty or complete)`);
    }
    for (const name of def.plurals.filter((n) => !loc.plurals.includes(n))) {
      errors.push(`android ${lang}: missing plurals ${name} (locale folders must be empty or complete)`);
    }
  }
  return errors;
}

/**
 * Keys that are only exercised by the i18n runtime tests until a later batch
 * migrates a screen that uses them (the plural runtime tests need a real
 * one/other pair). Delete an entry as soon as a component references it.
 */
export const UNUSED_ALLOWLIST: readonly string[] = [
  "connection.attempt.one",
  "connection.attempt.other",
];

/** The allowlist must shrink: entries that are now used or no longer exist are errors. */
export function checkUnusedAllowlist(
  keys: string[],
  sources: Map<string, string>,
  allowlist: readonly string[],
): string[] {
  const unused = new Set(findUnusedKeys(keys, sources));
  return allowlist.flatMap((key) => {
    if (!keys.includes(key)) return [`UNUSED_ALLOWLIST entry ${key} no longer exists in en`];
    if (!unused.has(key)) return [`UNUSED_ALLOWLIST entry ${key} is now used; remove it`];
    return [];
  });
}

/** Per locale: sorted en keys, existing non-empty values kept, new keys "", removed keys dropped. */
export function syncLocaleFiles(
  source: Catalog,
  locales: Record<string, Catalog>,
): Record<string, Catalog> {
  const keys = Object.keys(source).sort();
  const out: Record<string, Catalog> = {};
  for (const [lang, catalog] of Object.entries(locales)) {
    out[lang] = Object.fromEntries(keys.map((key) => [key, catalog[key] || ""]));
  }
  return out;
}

function collectSources(dir: string, root: string, out: Map<string, string>): void {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      collectSources(full, root, out);
    } else if (/\.tsx?$/.test(entry)) {
      out.set(relative(root, full), readFileSync(full, "utf8"));
    }
  }
}

function readLocales(dir: string): Record<string, Catalog> {
  const locales: Record<string, Catalog> = {};
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
    locales[file.replace(/\.json$/, "")] = JSON.parse(readFileSync(join(dir, file), "utf8"));
  }
  return locales;
}

function readAndroid(resDir: string): {
  def: AndroidStrings;
  locales: Record<string, AndroidStrings>;
  errors: string[];
} {
  const errors: string[] = [];
  const read = (folder: string) => {
    const file = join(resDir, folder, "strings.xml");
    if (!existsSync(file)) return undefined;
    try {
      return parseAndroidStrings(readFileSync(file, "utf8"));
    } catch (err) {
      errors.push(`${folder}/strings.xml: ${(err as Error).message}`);
      return undefined;
    }
  };
  const def = read("values") ?? { strings: [], plurals: [] };
  const locales: Record<string, AndroidStrings> = {};
  if (existsSync(resDir)) {
    for (const folder of readdirSync(resDir)) {
      if (!/^values-[a-z]{2,3}(-r[A-Z]{2})?$/.test(folder)) continue;
      const parsed = read(folder);
      if (parsed) locales[folder.slice("values-".length)] = parsed;
    }
  }
  return { def, locales, errors };
}

function writeLocale(dir: string, lang: string, catalog: Catalog): void {
  writeFileSync(join(dir, `${lang}.json`), `${JSON.stringify(catalog, null, 2)}\n`);
}

function main(argv: string[]): number {
  const args = argv.filter((a) => a !== "--");
  const write = args.includes("--write");
  const addAt = args.indexOf("--add");
  const addLang = addAt >= 0 ? args[addAt + 1] : undefined;
  if (addAt >= 0 && !(write && addLang && /^[a-z]{2,3}$/.test(addLang))) {
    console.error("usage: i18n:sync -- --add <lang>   (e.g. fr)");
    return 1;
  }

  const clientDir = resolve(import.meta.dirname ?? __dirname, "..");
  const localeDir = join(clientDir, "src/i18n/locales");
  const source: Catalog = en;
  let locales = readLocales(localeDir);
  if (addLang) {
    if (locales[addLang]) {
      console.error(`locales/${addLang}.json already exists`);
      return 1;
    }
    locales = { ...locales, [addLang]: {} };
  }
  if (write) {
    const synced = syncLocaleFiles(source, locales);
    for (const [lang, catalog] of Object.entries(synced)) {
      if (JSON.stringify(catalog) !== JSON.stringify(locales[lang]) || addLang === lang) {
        writeLocale(localeDir, lang, catalog);
        console.log(`i18n: wrote locales/${lang}.json`);
      }
    }
    locales = synced;
  }

  const keys = Object.keys(source);
  const sources = new Map<string, string>();
  collectSources(join(clientDir, "src"), clientDir, sources);
  const android = readAndroid(join(clientDir, "..", "mobile/android/app/src/main/res"));

  const errors = [
    ...checkKeyFormat(keys),
    ...checkAndroidNameCollisions(keys),
    ...checkLocaleKeySets(source, locales),
    ...checkPlaceholders(source, locales),
    ...findUnusedKeys(keys, sources)
      .filter((key) => !UNUSED_ALLOWLIST.includes(key))
      .map((key) => `unused key ${key}`),
    ...checkUnusedAllowlist(keys, sources, UNUSED_ALLOWLIST),
    ...android.errors,
    ...checkAndroidLocales(android.def, android.locales),
  ];
  for (const warning of findDuplicateValues(source)) console.warn(`warning: ${warning}`);
  for (const error of errors) console.error(error);
  if (errors.length > 0) return 1;
  console.log(
    `i18n: ${keys.length} web keys, ${android.def.strings.length + android.def.plurals.length} Android strings`,
  );
  return 0;
}

if ((import.meta as ImportMeta & { main?: boolean }).main) {
  process.exit(main(process.argv.slice(2)));
}
