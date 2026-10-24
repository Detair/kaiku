// @vitest-environment node
import { describe, it, expect } from "vitest";
import { ESLint } from "eslint";
import path from "path";
import { fileURLToPath } from "url";
import { readFileSync, readdirSync } from "fs";
import { t, plural } from "../index";
import { en } from "../en";

const clientDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

// Loading the ESLint config is slow under full-suite load; share one instance
// and allow it more than the default 5 s.
const LINT_TIMEOUT_MS = 30_000;
let eslintInstance: ESLint | undefined;
const eslint = () => (eslintInstance ??= new ESLint({ cwd: clientDir }));

describe("i18n", () => {
  it("t returns the English text for a key", () => {
    expect(t("common.retry")).toBe("Retry");
  });

  it("t fills placeholders", () => {
    expect(t("connection.attempt.other", { count: 3 })).toBe("Attempt 3");
  });

  it("plural picks one/other", () => {
    const forms = { one: "connection.attempt.one", other: "connection.attempt.other" } as const;
    expect(plural(1, forms)).toBe("Attempt 1");
    expect(plural(3, forms)).toBe("Attempt 3");
  });

  it("plural falls back to other when a category is missing", () => {
    expect(plural(1, { other: "connection.attempt.other" })).toBe("Attempt 1");
  });

  it("every area file only holds keys of its own area", async () => {
    const areaDir = path.join(clientDir, "src/i18n/en");
    const files = readdirSync(areaDir).filter((f) => f.endsWith(".ts"));
    expect(files.length).toBeGreaterThan(0);
    let total = 0;
    for (const file of files) {
      const area = file.replace(/\.ts$/, "");
      const mod = (await import(`../en/${area}.ts`)) as Record<string, Record<string, string>>;
      const keys = Object.keys(mod[area]);
      expect(keys.every((k) => k.startsWith(`${area}.`))).toBe(true);
      total += keys.length;
    }
    // No key is lost or duplicated by the aggregator.
    expect(Object.keys(en)).toHaveLength(total);
  });

  it("de.json lists every en key, sorted", () => {
    const de = JSON.parse(
      readFileSync(path.join(clientDir, "src/i18n/locales/de.json"), "utf8"),
    ) as Record<string, string>;
    expect(Object.keys(de)).toEqual(Object.keys(en).sort());
  });

  it("the language picker is lint-guarded", async () => {
    const results = await eslint().lintText("export const X = () => <p>Language</p>;", {
      filePath: "src/components/settings/LanguageSelect.tsx",
    });
    const messages = results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax");
    expect(messages).toHaveLength(1);
  }, LINT_TIMEOUT_MS);

  it("lint rejects JSX text literals in guarded files", async () => {
    const results = await eslint().lintText("export const X = () => <p>Hello</p>;", {
      filePath: "src/i18n/__fixtures__/x.tsx",
    });
    const messages = results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax");
    expect(messages).toHaveLength(1);
  }, LINT_TIMEOUT_MS);

  const lintCount = async (code: string) => {
    const results = await eslint().lintText(code, { filePath: "src/i18n/__fixtures__/x.tsx" });
    return results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax").length;
  };

  it("flags literal ternaries in children and toast titles", async () => {
    expect(await lintCount('export const X = () => <p>{ok ? "Saved" : "Failed"}</p>;')).toBe(2);
    expect(await lintCount('showToast({ type: "error", title: "Oops" });')).toBe(1);
    expect(await lintCount("export const X = () => <b title={`Hi ${n}`} />;")).toBe(1);
    expect(await lintCount('export const X = () => <p>{a}{" "}{b}</p>;')).toBe(1);
  }, LINT_TIMEOUT_MS);

  it("flags setError text but not message keys", async () => {
    expect(await lintCount('setError("Could not save.");')).toBe(1);
    expect(await lintCount('setError("auth.forcedChange.failed");')).toBe(0);
  }, LINT_TIMEOUT_MS);

  it("flags aria-description literals", async () => {
    expect(await lintCount('export const X = () => <p aria-description="Help text" />;')).toBe(1);
  }, LINT_TIMEOUT_MS);

  it("does not flag class ternaries or t() args", async () => {
    expect(
      await lintCount(
        'export const X = () => <p class={x ? "a b" : "c"} title={t("common.retry")}>{t("common.save")}</p>;',
      ),
    ).toBe(0);
  }, LINT_TIMEOUT_MS);
});
