import { describe, expect, it } from "vitest";
import {
  checkAndroidLocales,
  checkAndroidNameCollisions,
  checkKeyFormat,
  checkLocaleKeySets,
  checkPlaceholders,
  findUnusedKeys,
  syncLocaleFiles,
  toAndroidName,
} from "../check-i18n";

describe("check-i18n", () => {
  it("reports missing and unknown locale keys", () => {
    const en = { "a.b": "B", "a.c": "C" };
    const de = { de: { "a.b": "", "a.x": "" } };
    expect(checkLocaleKeySets(en, de)).toEqual(["de: missing key a.c", "de: unknown key a.x"]);
  });

  it("placeholders must match in non-empty translations", () => {
    const en = { "a.b": "Hi {{name}}" };
    expect(checkPlaceholders(en, { de: { "a.b": "Hallo {{nam}}" } })).toHaveLength(1);
    expect(checkPlaceholders(en, { de: { "a.b": "" } })).toHaveLength(0);
    expect(checkPlaceholders(en, { de: { "a.b": "Hallo {{name}}" } })).toHaveLength(0);
  });

  it("finds unused keys but counts literal references", () => {
    const sources = new Map([
      ["src/x.tsx", 't("a.b")'],
      ["src/i18n/en/a.ts", '"a.c": "C"'],
    ]);
    expect(findUnusedKeys(["a.b", "a.c"], sources)).toEqual(["a.c"]);
  });

  it("rejects bad key formats and Android collisions", () => {
    expect(checkKeyFormat(["Chat.send", "chat", "chat.sendFailed"])).toHaveLength(2);
    const errors = checkAndroidNameCollisions(["chat.sendFailed", "chat.send.failed"]);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain("chat_send_failed");
  });

  it("toAndroidName mirrors keys", () => {
    expect(toAndroidName("connection.retryNow")).toBe("connection_retry_now");
  });

  it("android locale folder must be empty or complete", () => {
    const def = { strings: ["a", "b"], plurals: ["p"] };
    expect(checkAndroidLocales(def, { de: { strings: [], plurals: [] } })).toHaveLength(0);
    const partial = checkAndroidLocales(def, { de: { strings: ["a"], plurals: [] } }).join("\n");
    expect(partial).toContain("b");
    expect(partial).toContain("p");
    const unknown = checkAndroidLocales(def, { de: { strings: ["a", "b", "z"], plurals: ["p"] } });
    expect(unknown.join("\n")).toContain("z");
  });

  it("sync keeps translations and orders keys", () => {
    const en = { "b.x": "X", "a.y": "Y" };
    const out = syncLocaleFiles(en, { de: { "b.x": "B", "old.k": "O" } });
    expect(out.de).toEqual({ "a.y": "", "b.x": "B" });
    expect(Object.keys(out.de)).toEqual(["a.y", "b.x"]);
  });
});
