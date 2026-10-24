process.env.TZ = "UTC";

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

/** Fresh module graph per test (locale signal is module state); see locale.test.ts. */
async function load(de: Record<string, string> = {}) {
  vi.resetModules();
  vi.doMock("../locales/de.json", () => ({ default: de }));
  const locale = await import("../locale");
  const format = await import("../format");
  return { ...locale, ...format };
}

const NOW = new Date("2026-10-23T14:30:00Z");

beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.doUnmock("../locales/de.json");
});

describe("format", () => {
  it("formatTime follows the system region when no language was chosen", async () => {
    const { formatLocales, formatTime, formatShortDate } = await load();
    expect(formatLocales(null, ["de-DE"])).toBeUndefined();
    expect(formatTime(NOW, ["de-DE"])).toBe("14:30");
    expect(formatTime(NOW, ["en-US"])).toBe("02:30 PM");
    expect(formatShortDate(new Date("2026-10-23T10:00:00Z"), NOW, ["de-DE"])).toMatch(/^23\. Okt/);
  });

  it("formatLocales prefers the system region of the chosen language", async () => {
    const { formatLocales } = await load();
    expect(formatLocales("de", ["en-GB", "de-AT"])).toEqual(["de-AT"]);
    expect(formatLocales("de", ["en-US"])).toEqual(["de"]);
  });

  it("relative time words follow the UI language", async () => {
    const { formatRelativeTime, formatRelativeTimeShort, setLocale } = await load();
    expect(formatRelativeTime("2026-10-23T14:25:00Z")).toBe("5 minutes ago");
    expect(formatRelativeTime("2026-10-23T14:29:00Z")).toBe("1 minute ago");
    expect(formatRelativeTime("2026-10-23T14:29:45Z")).toBe("just now");
    expect(formatRelativeTimeShort("2026-10-23T11:30:00Z")).toBe("3h ago");
    expect(formatRelativeTimeShort("2026-10-23T14:29:30Z")).toBe("just now");
    await setLocale("de");
    expect(formatRelativeTime("2026-10-23T14:25:00Z")).toBe("vor 5 Minuten");
  });

  it("formatTimestamp says Yesterday", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
    const { formatTimestamp } = await load();
    const out = formatTimestamp("2026-10-22T09:05:00Z");
    expect(out).toContain("Yesterday");
    expect(out).toContain("09:05");
    expect(formatTimestamp("2026-10-23T09:05:00Z")).toBe("09:05 AM");
  });

  it("formatList joins with the UI language", async () => {
    const { formatList } = await load();
    expect(formatList(["Anna", "Ben"])).toBe("Anna and Ben");
    expect(formatList(["Anna", "Ben"], "disjunction")).toBe("Anna or Ben");
  });

  it("formatNumber uses the given locales", async () => {
    const { formatNumber } = await load();
    expect(formatNumber(1234.5, ["de-DE"])).toBe("1.234,5");
    expect(formatNumber(1234.5, ["en-US"])).toBe("1,234.5");
  });
});
