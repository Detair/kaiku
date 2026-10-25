// Component tests for the locale runtime. They live apart from locale.test.ts
// because that file resets modules per test, which would give the compiled
// JSX here a different Solid instance than the signal it should track.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";
import { t, setLocale } from "../index";
import AppearanceSettings from "@/components/settings/AppearanceSettings";

const deMock = vi.hoisted(() => ({ dict: {} as Record<string, string> }));
vi.mock("../locales/de.json", () => ({ default: deMock.dict }));

afterEach(async () => {
  cleanup();
  vi.unstubAllEnvs();
  await setLocale("en");
  localStorage.clear();
});

describe("locale runtime in components", () => {
  it("t is reactive in components", async () => {
    deMock.dict["common.retry"] = "Erneut versuchen";
    render(() => <span>{t("common.retry")}</span>);
    expect(screen.getByText("Retry")).toBeTruthy();
    await setLocale("de");
    expect(screen.getByText("Erneut versuchen")).toBeTruthy();
  });

  it("LanguageSelect is hidden unless the flag is set", () => {
    render(() => <AppearanceSettings />);
    expect(screen.queryByLabelText("Language")).toBeNull();
    cleanup();

    vi.stubEnv("VITE_FEATURE_LOCALE_SWITCH", "true");
    render(() => <AppearanceSettings />);
    expect(screen.queryByLabelText("Language")).not.toBeNull();
    expect(screen.queryByText("Deutsch")).not.toBeNull();
    expect(screen.queryByText(t("settings.language.partial"))).toBeNull();
  });

  it("LanguageSelect shows the partial-translation hint outside English", async () => {
    vi.stubEnv("VITE_FEATURE_LOCALE_SWITCH", "true");
    render(() => <AppearanceSettings />);
    await setLocale("de");
    expect(
      screen.queryByText("Translations may be incomplete. Missing text is shown in English."),
    ).not.toBeNull();
  });
});
