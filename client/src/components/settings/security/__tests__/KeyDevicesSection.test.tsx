import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ listKeyDevices: vi.fn(), removeKeyDevice: vi.fn(), getMeSecurity: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import KeyDevicesSection from "../KeyDevicesSection";
import { HttpError } from "@/lib/tauri/common";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "./securityTestUtils";

function device(over: Record<string, unknown> = {}) {
  return {
    id: "d1", device_id: "x", label: "Pixel 8", platform: "android", state: "active", enrolled_via: "own",
    uv_class: "per_use", created_at: "2026-11-01T10:00:00Z", activated_at: "2026-11-01T10:00:00Z", ready_at: null,
    last_used_at: null, revoked_at: null, revoke_reason: null, is_current: false, ...over,
  };
}

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  api.getMeSecurity.mockResolvedValue(security());
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("KeyDevicesSection", () => {
  it("remove device steps up for that device then deletes", async () => {
    api.listKeyDevices.mockResolvedValue([device()]);
    api.removeKeyDevice.mockResolvedValue(device({ state: "revoked" }));
    render(() => <KeyDevicesSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove device Pixel 8" }));
    await waitFor(() => expect(api.removeKeyDevice).toHaveBeenCalledWith("d1"));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "key_device.remove", target: "d1", targetLabel: "Pixel 8" });
  });

  it("last strong authenticator shows add-another-first", async () => {
    api.listKeyDevices.mockResolvedValue([device()]);
    api.removeKeyDevice.mockRejectedValue(new HttpError(409, "x", "LAST_STRONG_AUTHENTICATOR"));
    render(() => <KeyDevicesSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove device Pixel 8" }));
    expect(await screen.findByText("This is your only phone or passkey. Add another one first.")).toBeTruthy();
  });

  it("pending key device shows ready at", async () => {
    api.listKeyDevices.mockResolvedValue([device({ state: "pending", ready_at: "2026-11-04T09:30:00Z" })]);
    render(() => <KeyDevicesSection />);
    expect(await screen.findByText(/^Ready at /)).toBeTruthy();
  });

  it("probation shows the list read-only", async () => {
    signInAs("probation");
    api.listKeyDevices.mockResolvedValue([device()]);
    render(() => <KeyDevicesSection />);
    expect(await screen.findByText("Pixel 8")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Remove device Pixel 8" })).toBeNull();
    expect(screen.getByText("Sign in with a passkey to change this.")).toBeTruthy();
  });
});

describe("device list freshness (Argon review 3 #5)", () => {
  it("authenticators_changed reloads the list", async () => {
    const { AUTHENTICATORS_CHANGED_EVENT } = await import("@/lib/types/events");
    api.listKeyDevices.mockResolvedValueOnce([device()]).mockResolvedValueOnce([device(), device({ id: "d2", label: "Tab A7" })]);
    render(() => <KeyDevicesSection />);
    await screen.findByText("Pixel 8");
    window.dispatchEvent(new CustomEvent(AUTHENTICATORS_CHANGED_EVENT));
    expect(await screen.findByText("Tab A7")).toBeTruthy();
  });
});

