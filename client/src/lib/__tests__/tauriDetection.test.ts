/**
 * Tauri detection (contract C6): the webview runs in native mode when the
 * Tauri core says so, independent of the `withGlobalTauri` global.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ESLint } from "eslint";
import path from "path";
import { fileURLToPath } from "url";

const { coreIsTauri } = vi.hoisted(() => ({ coreIsTauri: vi.fn(() => false) }));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: coreIsTauri,
}));
// TauriVoiceAdapter subscribes to native voice events on construction.
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

// Built at runtime so the banned name never appears literally in src/.
const TAURI_GLOBAL = ["", "", "TAURI", "", ""].join("_");

const clientDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

describe("Tauri detection", () => {
  beforeEach(() => {
    // Browser-mode import of common.ts starts a cookie session-restore refresh.
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 401, json: vi.fn() }));
  });

  afterEach(async () => {
    const { resetVoiceAdapter } = await import("../webrtc");
    resetVoiceAdapter();
    coreIsTauri.mockReset();
    coreIsTauri.mockReturnValue(false);
    vi.unstubAllGlobals();
  });

  it("isTauri() is true when the Tauri core reports it, without the withGlobalTauri global", async () => {
    coreIsTauri.mockReturnValue(true);
    const { isTauri } = await import("../tauri/common");

    expect(Object.prototype.hasOwnProperty.call(window, TAURI_GLOBAL)).toBe(false);
    expect(isTauri()).toBe(true);
  });

  it("isTauri() is false in a plain browser", async () => {
    coreIsTauri.mockReturnValue(false);
    const { isTauri } = await import("../tauri/common");

    expect(isTauri()).toBe(false);
  });

  it("createVoiceAdapter returns the Tauri adapter in native mode", async () => {
    coreIsTauri.mockReturnValue(true);
    const { createVoiceAdapter } = await import("../webrtc");
    const { TauriVoiceAdapter } = await import("../webrtc/tauri");

    const adapter = await createVoiceAdapter();

    expect(adapter).toBeInstanceOf(TauriVoiceAdapter);
  });

  it("lint rejects the Tauri global literal", async () => {
    const eslint = new ESLint({
      cwd: clientDir,
      // The virtual file is in no tsconfig; detection rules are syntax-only.
      overrideConfig: { languageOptions: { parserOptions: { project: false } } },
    });
    const results = await eslint.lintText(`const a = "${TAURI_GLOBAL}" in window;`, {
      filePath: "src/lib/x.ts",
    });
    const messages = results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax");

    expect(messages).toHaveLength(1);
    expect(messages[0].message).toBe("Use isTauri() from @/lib/tauri/common (contract C6).");
  }, 30_000);

  it("lint rejects window.<Tauri global> in i18n-guarded files too", async () => {
    const eslint = new ESLint({ cwd: clientDir });
    const results = await eslint.lintText(`export const a = window.${TAURI_GLOBAL};`, {
      filePath: "src/i18n/__fixtures__/x.ts",
    });
    const messages = results[0].messages.filter((m) => m.ruleId === "no-restricted-syntax");

    expect(messages).toHaveLength(1);
  }, 30_000);
});
