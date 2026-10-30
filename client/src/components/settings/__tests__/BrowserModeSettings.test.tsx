import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const native = vi.hoisted(() => ({ on: false }));
vi.mock("@/lib/tauri/common", async (orig) => ({
  ...(await orig<typeof import("@/lib/tauri/common")>()),
  isTauri: () => native.on,
}));
vi.mock("@/lib/sound", () => ({ testSound: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }));

import NotificationSettings from "../NotificationSettings";
import PrivacySettings from "../PrivacySettings";
import FocusSettings from "../FocusSettings";
import {
  initNotifications,
  cleanupNotifications,
  sendOsNotification,
} from "@/lib/notifications";

type Perm = "default" | "granted" | "denied";
const requestPermission = vi.fn();

function stubNotification(permission: Perm | null) {
  if (permission === null) {
    vi.stubGlobal("Notification", undefined);
    return;
  }
  const ctor = vi.fn() as unknown as { permission: Perm; requestPermission: typeof requestPermission };
  ctor.permission = permission;
  ctor.requestPermission = requestPermission;
  vi.stubGlobal("Notification", ctor);
}

beforeEach(() => {
  native.on = false;
  requestPermission.mockReset();
  cleanupNotifications();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("browser notification permission", () => {
  it("does not request permission on init", async () => {
    stubNotification("default");
    await initNotifications();
    expect(requestPermission).not.toHaveBeenCalled();
  });

  it("shows the Allow button when not yet allowed and requests on click", async () => {
    stubNotification("default");
    requestPermission.mockResolvedValue("granted");
    render(() => <NotificationSettings />);
    fireEvent.click(screen.getByRole("button", { name: "Allow notifications" }));
    expect(requestPermission).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.getByText("Notifications are allowed in this browser.")).toBeTruthy(),
    );
  });

  it("shows enabled state without a button", () => {
    stubNotification("granted");
    render(() => <NotificationSettings />);
    expect(screen.getByText("Notifications are allowed in this browser.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Allow notifications" })).toBeNull();
  });

  it("shows blocked hint and the test button explains it", () => {
    stubNotification("denied");
    render(() => <NotificationSettings />);
    expect(screen.getByText(/Notifications are blocked/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Send test notification/ }));
    expect(screen.getAllByText(/Notifications are blocked/).length).toBe(2);
  });

  it("shows not supported when Notification is missing", () => {
    stubNotification(null);
    render(() => <NotificationSettings />);
    expect(screen.getByText("This browser cannot show notifications from Kaiku.")).toBeTruthy();
  });

  it("treats Android browsers as unsupported", () => {
    stubNotification("granted");
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (Linux; Android 14) Chrome/130" });
    render(() => <NotificationSettings />);
    expect(screen.getByText("This browser cannot show notifications from Kaiku.")).toBeTruthy();
  });

  it("test button asks for permission first when not yet allowed", () => {
    stubNotification("default");
    render(() => <NotificationSettings />);
    fireEvent.click(screen.getByRole("button", { name: /Send test notification/ }));
    expect(screen.getByText("Allow notifications first to send a test.")).toBeTruthy();
  });
});

describe("live permission and constructor failures", () => {
  const ev = { type: "message_dm", channelId: "c", isDm: true } as const;
  const ctx = { username: "u", content: "hi", guildName: null, channelName: null };

  it("sends once permission is granted after init (no reload)", async () => {
    stubNotification("default");
    await initNotifications();
    await sendOsNotification(ev, ctx, true, true);
    expect(Notification).not.toHaveBeenCalled();
    (Notification as unknown as { permission: Perm }).permission = "granted";
    await sendOsNotification(ev, ctx, true, true);
    expect(Notification).toHaveBeenCalledTimes(1);
  });

  it("does not send after permission is revoked", async () => {
    stubNotification("granted");
    await initNotifications();
    (Notification as unknown as { permission: Perm }).permission = "denied";
    await sendOsNotification(ev, ctx, true, true);
    expect(Notification).not.toHaveBeenCalled();
  });

  it("refreshes the settings row when the window regains focus", async () => {
    stubNotification("default");
    render(() => <NotificationSettings />);
    expect(screen.getByText("Kaiku is not yet allowed to show notifications.")).toBeTruthy();
    (Notification as unknown as { permission: Perm }).permission = "granted";
    window.dispatchEvent(new Event("focus"));
    await waitFor(() =>
      expect(screen.getByText("Notifications are allowed in this browser.")).toBeTruthy(),
    );
  });

  it("remembers an illegal constructor and shows not supported", async () => {
    stubNotification("granted");
    (Notification as unknown as ReturnType<typeof vi.fn>).mockImplementation(() => {
      throw new TypeError("Illegal constructor");
    });
    render(() => <NotificationSettings />);
    expect(screen.getByText("Notifications are allowed in this browser.")).toBeTruthy();
    await sendOsNotification(ev, ctx, true, true);
    await waitFor(() =>
      expect(screen.getByText("This browser cannot show notifications from Kaiku.")).toBeTruthy(),
    );
    await sendOsNotification(ev, ctx, true, true);
    expect(Notification).toHaveBeenCalledTimes(1);
  });

  it("treats mobile userAgentData as unsupported", () => {
    stubNotification("granted");
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (X11; Linux) Chrome/130", userAgentData: { mobile: true } });
    render(() => <NotificationSettings />);
    expect(screen.getByText("This browser cannot show notifications from Kaiku.")).toBeTruthy();
  });
});

describe("desktop-only options", () => {
  it("hides flash taskbar, activity sharing and auto-activation in the browser", () => {
    stubNotification("granted");
    render(() => (
      <>
        <NotificationSettings />
        <PrivacySettings />
        <FocusSettings />
      </>
    ));
    expect(screen.queryByText("Flash taskbar on notification")).toBeNull();
    expect(screen.queryByText("Display current activity")).toBeNull();
    expect(screen.queryByText("Auto-activate focus modes")).toBeNull();
    expect(screen.queryByText("Custom App Detection Rules")).toBeNull();
  });

  it("shows them in Tauri mode", async () => {
    native.on = true;
    stubNotification("granted");
    vi.doMock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => true) }));
    render(() => (
      <>
        <NotificationSettings />
        <PrivacySettings />
        <FocusSettings />
      </>
    ));
    expect(screen.getByText("Flash taskbar on notification")).toBeTruthy();
    expect(screen.getByText("Display current activity")).toBeTruthy();
    expect(screen.getByText("Auto-activate focus modes")).toBeTruthy();
    expect(screen.getByText("Custom App Detection Rules")).toBeTruthy();
    expect(screen.queryByText("Browser permission")).toBeNull();
  });
});
