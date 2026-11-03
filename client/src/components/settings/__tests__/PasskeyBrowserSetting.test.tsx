import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const settings = vi.hoisted(() => ({ getSettings: vi.fn(), updateSettings: vi.fn() }));
const env = vi.hoisted(() => ({ tauri: true, mac: false }));

vi.mock("@/lib/tauri/settings", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/settings")),
  ...settings,
}));
vi.mock("@/lib/tauri/common", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/common")),
  isTauri: () => env.tauri,
}));

import PasskeyBrowserSetting from "../PasskeyBrowserSetting";

const BASE = { theme: "dark", notifications_enabled: true, audio: {}, voice: {}, passkey_browser: "system" };

beforeEach(() => {
  env.tauri = true;
  env.mac = false;
  vi.spyOn(navigator, "userAgent", "get").mockImplementation(() =>
    env.mac ? "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)" : "Mozilla/5.0 (X11; Linux x86_64)",
  );
  settings.getSettings.mockReset().mockResolvedValue({ ...BASE });
  settings.updateSettings.mockReset().mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("PasskeyBrowserSetting", () => {
  it("saves the chosen browser", async () => {
    render(() => <PasskeyBrowserSetting />);
    const select = (await screen.findByLabelText("Browser for passkey sign-in")) as HTMLSelectElement;
    await waitFor(() => expect(select.value).toBe("system"));
    fireEvent.change(select, { target: { value: "chromium" } });
    await waitFor(() =>
      expect(settings.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ passkey_browser: "chromium", theme: "dark" })),
    );
    // On Linux the help names the Firefox limit.
    expect(screen.getByText(/Firefox cannot use your phone/)).toBeTruthy();
  });

  it("Safari is listed only on macOS", async () => {
    render(() => <PasskeyBrowserSetting />);
    await screen.findByLabelText("Browser for passkey sign-in");
    expect(screen.queryByRole("option", { name: "Safari" })).toBeNull();
    cleanup();
    env.mac = true;
    render(() => <PasskeyBrowserSetting />);
    await screen.findByLabelText("Browser for passkey sign-in");
    expect(screen.getByRole("option", { name: "Safari" })).toBeTruthy();
  });

  it("hidden outside the desktop app", () => {
    env.tauri = false;
    render(() => <PasskeyBrowserSetting />);
    expect(screen.queryByLabelText("Browser for passkey sign-in")).toBeNull();
  });
});
