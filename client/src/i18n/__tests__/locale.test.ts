import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

type DeMock = { de?: Record<string, string>; fail?: boolean };

/**
 * Fresh module graph per test: the locale signal is module state, and the
 * mocked `de.json` is resolved once per graph. Solid and the testing library
 * are imported from the same graph so the render effect tracks the signal.
 */
async function load(mock: DeMock = {}) {
  vi.resetModules();
  vi.doMock("../locales/de.json", () => {
    if (mock.fail) throw new Error("de.json unavailable");
    return { default: mock.de ?? {} };
  });
  const locale = await import("../locale");
  const i18n = await import("../index");
  return { ...locale, ...i18n };
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.lang = "";
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  vi.doUnmock("../locales/de.json");
});

describe("locale runtime", () => {
  it("t falls back to English for empty de values", async () => {
    const { t, setLocale, getLocale } = await load({ de: { "common.retry": "" } });
    await setLocale("de");
    expect(getLocale()).toBe("de");
    expect(t("common.retry")).toBe("Retry");
  });

  it("t uses a non-empty de value", async () => {
    const { t, setLocale } = await load({ de: { "common.retry": "Erneut versuchen" } });
    await setLocale("de");
    expect(t("common.retry")).toBe("Erneut versuchen");
    await setLocale("en");
    expect(t("common.retry")).toBe("Retry");
  });

  it("initLocale ignores the stored choice when the switch is disabled", async () => {
    localStorage.setItem("kaiku.locale", "de");
    const { initLocale, getLocale, hasExplicitLocale, LOCALE_STORAGE_KEY } = await load({
      de: { "common.retry": "Erneut versuchen" },
    });
    await initLocale();
    expect(getLocale()).toBe("en");
    expect(hasExplicitLocale()).toBe(false);
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("de");
    expect(document.documentElement.lang).toBe("en");
  });

  it("initLocale uses the stored choice when the switch is enabled", async () => {
    vi.stubEnv("VITE_FEATURE_LOCALE_SWITCH", "true");
    localStorage.setItem("kaiku.locale", "de");
    const { initLocale, getLocale, hasExplicitLocale, t } = await load({
      de: { "common.retry": "Erneut versuchen" },
    });
    await initLocale();
    expect(getLocale()).toBe("de");
    expect(hasExplicitLocale()).toBe(true);
    expect(t("common.retry")).toBe("Erneut versuchen");
  });

  it("initLocale falls back to en for an unknown or unloadable locale", async () => {
    vi.stubEnv("VITE_FEATURE_LOCALE_SWITCH", "true");

    localStorage.setItem("kaiku.locale", "xx");
    let m = await load({ de: {} });
    await m.initLocale();
    expect(m.getLocale()).toBe("en");
    expect(m.hasExplicitLocale()).toBe(false);

    localStorage.setItem("kaiku.locale", "de");
    m = await load({ fail: true });
    await expect(m.initLocale()).resolves.toBeUndefined();
    expect(m.getLocale()).toBe("en");
    expect(m.t("common.retry")).toBe("Retry");
    expect(document.documentElement.lang).toBe("en");
  });

  it("initLocale survives a localStorage that throws", async () => {
    vi.stubEnv("VITE_FEATURE_LOCALE_SWITCH", "true");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    const { initLocale, getLocale } = await load();
    await expect(initLocale()).resolves.toBeUndefined();
    expect(getLocale()).toBe("en");
  });

  it("setLocale rejects a de catalog that is not a flat string map", async () => {
    const { setLocale, getLocale, t } = await load({
      de: { "common.retry": 42 } as unknown as Record<string, string>,
    });
    await setLocale("de");
    expect(getLocale()).toBe("en");
    expect(t("common.retry")).toBe("Retry");
  });

  it("setLocale sets html lang and persists", async () => {
    const { setLocale, LOCALE_STORAGE_KEY, hasExplicitLocale } = await load({ de: {} });
    await setLocale("de");
    expect(document.documentElement.lang).toBe("de");
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("de");
    expect(hasExplicitLocale()).toBe(true);

    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    await expect(setLocale("en")).resolves.toBeUndefined();
    expect(document.documentElement.lang).toBe("en");
  });

  it("plural uses the active locale's categories", async () => {
    const { plural } = await load();
    expect(plural(0, { one: "connection.attempt.one", other: "connection.attempt.other" })).toBe(
      "Attempt 0",
    );
    expect(plural(1, { other: "connection.attempt.other" })).toBe("Attempt 1");
  });
});
