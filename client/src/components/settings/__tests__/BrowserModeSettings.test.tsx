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
import { initNotifications, cleanupNotifications } from "@/lib/notifications";

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
