import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const state = vi.hoisted(() => ({ set: null as null | ((v: Record<string, unknown>) => void) }));

vi.mock("@/stores/preferences", async () => {
  const { createSignal } = await import("solid-js");
  const [prefs, setPrefs] = createSignal<Record<string, unknown>>({
    theme: "focused-hybrid",
  });
  state.set = (v) => setPrefs(v);
  return {
    preferences: prefs,
    updatePreference: (key: string, value: unknown) => setPrefs({ ...prefs(), [key]: value }),
  };
});
vi.mock("@/lib/featureFlags", () => ({ isLocaleSwitchEnabled: () => false }));

import AppearanceSettings from "../AppearanceSettings";

beforeEach(() => state.set!({ theme: "focused-hybrid" }));
afterEach(() => cleanup());

function radio(name: RegExp) {
  return screen.getByRole("radio", { name });
}

describe("AppearanceSettings", () => {
  it("lists the light theme and 'same as my device' as radios", () => {
    render(() => <AppearanceSettings />);
    expect(radio(/Same as my device/)).toBeTruthy();
    expect(radio(/Solarized Light/)).toBeTruthy();
    expect(radio(/^Focused Hybrid/).getAttribute("aria-checked")).toBe("true");
    expect(radio(/Same as my device/).getAttribute("aria-checked")).toBe("false");
  });

  it("choosing the light theme selects it", () => {
    render(() => <AppearanceSettings />);
    fireEvent.click(radio(/Solarized Light/));
    expect(radio(/Solarized Light/).getAttribute("aria-checked")).toBe("true");
    expect(radio(/^Focused Hybrid/).getAttribute("aria-checked")).toBe("false");
  });

  it("'same as my device' replaces the marked theme and says what is shown", () => {
    render(() => <AppearanceSettings />);
    fireEvent.click(radio(/Same as my device/));
    expect(radio(/Same as my device/).getAttribute("aria-checked")).toBe("true");
    expect(radio(/^Focused Hybrid/).getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText(/Right now:/)).toBeTruthy();
    // picking a theme again turns it off
    fireEvent.click(radio(/Solarized Light/));
    expect(radio(/Same as my device/).getAttribute("aria-checked")).toBe("false");
  });

  it("offers three text sizes and applies the choice to the root", () => {
    render(() => <AppearanceSettings />);
    expect(radio(/^Normal$/).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(radio(/^Large$/));
    expect(radio(/^Large$/).getAttribute("aria-checked")).toBe("true");
    expect(document.documentElement.getAttribute("data-font-size")).toBe("large");
    fireEvent.click(radio(/^Very large$/));
    expect(document.documentElement.getAttribute("data-font-size")).toBe("xlarge");
    fireEvent.click(radio(/^Normal$/));
    expect(document.documentElement.getAttribute("data-font-size")).toBeNull();
  });
});
