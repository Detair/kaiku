/**
 * check-tauri-commands.ts
 *
 * Verifies that every Tauri command the webview invokes is registered in the
 * Rust `generate_handler![…]` list, and that every `invoke(` names its command
 * with a string literal (so the check can see it).
 *
 * Also flags `isTauri` references that are not called: `isTauri` is a
 * function, and an uncalled function reference is always truthy.
 *
 * Run: bun run lint:tauri-commands
 * Exit code: 1 if an invoked command is unregistered, an invoke name is not
 * a literal or `isTauri` is used uncalled, 0 otherwise.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

export interface InvokeSite {
  file: string;
  line: number;
  /** Command name, or null when the first argument is not a string literal. */
  name: string | null;
}

export interface UnregisteredInvoke {
  name: string;
  files: string[];
}

function isExcluded(file: string): boolean {
  return /(^|\/)__tests__\//.test(file) || /\.test\.tsx?$/.test(file);
}

function isIdentChar(ch: string | undefined): boolean {
  return ch !== undefined && /[A-Za-z0-9_$]/.test(ch);
}

/**
 * Replace comments and string/template contents with spaces (newlines kept),
 * so identifiers found afterwards are real code. Quote characters stay in
 * place so literal arguments can still be located and read from the source.
 */
function maskNonCode(src: string): string {
  const out = src.split("");
  let i = 0;
  const blank = (from: number, to: number) => {
    for (let k = from; k < to; k++) if (out[k] !== "\n") out[k] = " ";
  };
  while (i < src.length) {
    const ch = src[i];
    const next = src[i + 1];
    if (ch === "/" && next === "/") {
      const end = src.indexOf("\n", i);
      const stop = end === -1 ? src.length : end;
      blank(i, stop);
      i = stop;
    } else if (ch === "/" && next === "*") {
      const end = src.indexOf("*/", i + 2);
      const stop = end === -1 ? src.length : end + 2;
      blank(i, stop);
      i = stop;
    } else if (ch === '"' || ch === "'" || ch === "`") {
      let j = i + 1;
      while (j < src.length && src[j] !== ch) {
        if (src[j] === "\\") j++;
        else if (ch !== "`" && src[j] === "\n") break;
        j++;
      }
      blank(i + 1, j);
      i = j + 1;
    } else {
      i++;
    }
  }
  return out.join("");
}

function lineOf(src: string, index: number): number {
  let line = 1;
  for (let k = 0; k < index; k++) if (src[k] === "\n") line++;
  return line;
}

function skipWs(src: string, i: number): number {
  while (i < src.length && /\s/.test(src[i])) i++;
  return i;
}

/** Skip a balanced `<…>` type argument list starting at `i` (which is `<`). */
function skipTypeArgs(code: string, i: number): number {
  let depth = 0;
  for (; i < code.length; i++) {
    const ch = code[i];
    if (ch === "=" && code[i + 1] === ">") {
      i++;
    } else if (ch === "<") {
      depth++;
    } else if (ch === ">") {
      depth--;
      if (depth === 0) return i + 1;
    } else if (ch === ";" || ch === "{") {
      return -1;
    }
  }
  return -1;
}

/** Read a string literal at `i` from the original source; null if not one. */
function readLiteral(src: string, i: number): string | null {
  const quote = src[i];
  if (quote !== '"' && quote !== "'" && quote !== "`") return null;
  let j = i + 1;
  let value = "";
  while (j < src.length && src[j] !== quote) {
    if (src[j] === "\\") {
      value += src[j + 1];
      j += 2;
      continue;
    }
    if (quote === "`" && src[j] === "$" && src[j + 1] === "{") return null;
    value += src[j];
    j++;
  }
  return j < src.length ? value : null;
}

/** All `invoke(` / `invoke<T>(` call sites in one file. */
export function findInvokeSites(file: string, src: string): InvokeSite[] {
  const code = maskNonCode(src);
  const sites: InvokeSite[] = [];
  const re = /\binvoke\b/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code)) !== null) {
    const start = m.index;
    if (isIdentChar(code[start - 1])) continue;
    const before = code.slice(Math.max(0, start - 12), start);
    if (/\bfunction\s+$/.test(before)) continue;
    let i = skipWs(code, start + "invoke".length);
    if (code[i] === "<") {
      i = skipTypeArgs(code, i);
      if (i === -1) continue;
      i = skipWs(code, i);
    }
    if (code[i] !== "(") continue;
    const argStart = skipWs(code, i + 1);
    sites.push({
      file,
      line: lineOf(src, start),
      name: readLiteral(src, argStart),
    });
  }
  return sites;
}

function allSites(srcFiles: Map<string, string>): InvokeSite[] {
  const sites: InvokeSite[] = [];
  for (const [file, src] of srcFiles) {
    if (isExcluded(file)) continue;
    sites.push(...findInvokeSites(file, src));
  }
  return sites;
}

/** Command names invoked with a string literal across the given sources. */
export function findInvokedCommands(srcFiles: Map<string, string>): Set<string> {
  const names = new Set<string>();
  for (const site of allSites(srcFiles)) {
    if (site.name !== null) names.add(site.name);
  }
  return names;
}

/** `invoke(` calls whose first argument is not a string literal. */
export function findNonLiteralInvokes(
  srcFiles: Map<string, string>,
): InvokeSite[] {
  return allSites(srcFiles).filter((s) => s.name === null);
}

/** Identifiers listed inside `generate_handler![…]` (last path segment). */
export function findRegisteredCommands(libRs: string): Set<string> {
  const names = new Set<string>();
  const start = libRs.indexOf("generate_handler![");
  if (start === -1) return names;
  const open = start + "generate_handler!".length;
  let depth = 0;
  let end = -1;
  for (let i = open; i < libRs.length; i++) {
    if (libRs[i] === "[") depth++;
    else if (libRs[i] === "]") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end === -1) return names;
  const body = libRs
    .slice(open + 1, end)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "")
    .replace(/#\[[^\]]*\]/g, "");
  for (const entry of body.split(",")) {
    const path = entry.trim();
    if (!path) continue;
    const segment = path.split("::").pop()?.trim();
    if (segment && /^[A-Za-z_][A-Za-z0-9_]*$/.test(segment)) names.add(segment);
  }
  return names;
}

/** Invoked command names missing from `generate_handler!`, sorted by name. */
export function findUnregisteredInvokes(
  srcFiles: Map<string, string>,
  libRs: string,
): UnregisteredInvoke[] {
  const registered = findRegisteredCommands(libRs);
  const byName = new Map<string, Set<string>>();
  for (const site of allSites(srcFiles)) {
    if (site.name === null || registered.has(site.name)) continue;
    const files = byName.get(site.name) ?? new Set<string>();
    files.add(site.file);
    byName.set(site.name, files);
  }
  return [...byName.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, files]) => ({ name, files: [...files].sort() }));
}

/** `import {`, `import type {`, `import X, {`, `export {` left open on this line. */
const OPENS_SPECIFIER_LIST =
  /^\s*(import|export)(\s+type)?(\s+[A-Za-z_$][\w$]*\s*,)?\s*\{[^}]*$/;

/**
 * `isTauri` references not followed by `(` outside import/export statements
 * (including multi-line specifier lists), as `file:line`.
 */
export function findUncalledIsTauri(srcFiles: Map<string, string>): string[] {
  const hits: string[] = [];
  for (const [file, src] of srcFiles) {
    if (isExcluded(file)) continue;
    const code = maskNonCode(src);
    let inSpecifierList = false;
    code.split("\n").forEach((line, idx) => {
      // Specifier lists of multi-line `import {` / `export {` statements.
      if (inSpecifierList) {
        if (line.includes("}")) inSpecifierList = false;
        return;
      }
      if (/^\s*(import|export)\b/.test(line)) {
        inSpecifierList = OPENS_SPECIFIER_LIST.test(line);
        return;
      }
      const re = /\bisTauri\b/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(line)) !== null) {
        if (isIdentChar(line[m.index - 1]) || line[m.index - 1] === ".") {
          continue;
        }
        const after = skipWs(line, m.index + "isTauri".length);
        if (line[after] !== "(") {
          hits.push(`${file}:${idx + 1}`);
          break;
        }
      }
    });
  }
  return hits;
}

function collectSources(dir: string, root: string, out: Map<string, string>) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "__tests__" || entry === "node_modules") continue;
      collectSources(full, root, out);
    } else if (/\.tsx?$/.test(entry) && !/\.d\.ts$/.test(entry)) {
      out.set(relative(root, full), readFileSync(full, "utf8"));
    }
  }
}

/** All check failures for the given sources, as printable messages. */
export function runChecks(srcFiles: Map<string, string>, libRs: string): string[] {
  const errors: string[] = [];
  for (const { name, files } of findUnregisteredInvokes(srcFiles, libRs)) {
    errors.push(`unregistered Tauri command "${name}": ${files.join(", ")}`);
  }
  for (const site of findNonLiteralInvokes(srcFiles)) {
    errors.push(
      `${site.file}:${site.line}: invoke() must name its command with a string literal`,
    );
  }
  for (const hit of findUncalledIsTauri(srcFiles)) {
    errors.push(`${hit}: isTauri must be called: isTauri()`);
  }
  return errors;
}

function main(): number {
  const clientDir = resolve(import.meta.dirname ?? __dirname, "..");
  const srcFiles = new Map<string, string>();
  collectSources(join(clientDir, "src"), clientDir, srcFiles);
  const libRs = readFileSync(join(clientDir, "src-tauri/src/lib.rs"), "utf8");

  const errors = runChecks(srcFiles, libRs);
  for (const error of errors) console.error(error);
  if (errors.length > 0) return 1;
  console.log(
    `Tauri invoke check passed (${findInvokedCommands(srcFiles).size} commands, all registered).`,
  );
  return 0;
}

if ((import.meta as ImportMeta & { main?: boolean }).main) {
  process.exit(main());
}
