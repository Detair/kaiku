// @vitest-environment node
import { describe, it, expect } from "vitest";
import { ESLint } from "eslint";
import path from "path";
import { fileURLToPath } from "url";
import { t, plural } from "../index";

const clientDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

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

  it("lint rejects JSX text literals in guarded files", async () => {
    const eslint = new ESLint({ cwd: clientDir });
    const results = await eslint.lintText("export const X = () => <p>Hello</p>;", {
      filePath: "src/i18n/__fixtures__/x.tsx",
    });
    const messages = results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax");
    expect(messages).toHaveLength(1);
  });
});
