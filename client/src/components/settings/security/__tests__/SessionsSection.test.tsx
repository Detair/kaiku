import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  listSessions: vi.fn(),
  revokeSession: vi.fn(),
  revokeAllOtherSessions: vi.fn(),
  removeKeyDevice: vi.fn(),
}));
vi.mock("@/lib/tauri", async (orig) => ({ ...(await orig<object>()), ...api }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), removeKeyDevice: api.removeKeyDevice }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import SessionsSection from "../../SessionsSection";
import { setAuthState } from "@/stores/auth";
import { signInAs } from "./securityTestUtils";

const base = { ip_address: null, city: null, country: null, created_at: "2026-11-01T00:00:00Z", expires_at: "", last_used_at: "2026-11-03T08:00:00Z" };
const sessions = [
  { ...base, id: "s-me", device: "Firefox on Linux", is_current: true, assurance: "full", client_class: "web", authenticator: { id: "p1", type: "passkey", label: "YubiKey" } },
  { ...base, id: "s2", device: "Kaiku desktop on Windows", is_current: false, assurance: "probation", client_class: "desktop", authenticator: null },
  { ...base, id: "s3", device: "Pixel 8", is_current: false, assurance: "full", client_class: "android", authenticator: { id: "d1", type: "key_device", label: "Pixel 8" } },
];

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  api.listSessions.mockResolvedValue({ sessions });
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("SessionsSection", () => {
  it("sessions show assurance and end one with a step-up", async () => {
    api.revokeSession.mockResolvedValue(undefined);
    render(() => <SessionsSection />);
    expect(await screen.findByText("Kaiku desktop on Windows")).toBeTruthy();
    expect(screen.getAllByText("Full").length).toBeGreaterThan(0);
    expect(screen.getByText("Chat only")).toBeTruthy();
    expect(screen.getByText("With YubiKey")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "End the session Kaiku desktop on Windows" }));
    await waitFor(() => expect(api.revokeSession).toHaveBeenCalledWith("s2"));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "sessions.end", target: "s2", targetLabel: "Kaiku desktop on Windows" });
  });

  it("sign out everywhere else uses target others", async () => {
    api.revokeAllOtherSessions.mockResolvedValue({ revoked_count: 2 });
    render(() => <SessionsSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Sign out everywhere else" }));
    await waitFor(() => expect(api.revokeAllOtherSessions).toHaveBeenCalled());
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "sessions.end", target: "others" });
  });

  it("key device session row offers remove device", async () => {
    api.removeKeyDevice.mockResolvedValue({ id: "d1", state: "revoked" });
    render(() => <SessionsSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove device Pixel 8" }));
    await waitFor(() => expect(api.removeKeyDevice).toHaveBeenCalledWith("d1"));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "key_device.remove", target: "d1", targetLabel: "Pixel 8" });
    expect(screen.queryByRole("button", { name: "End the session Pixel 8" })).toBeNull();
  });

  it("probation renders the list read-only", async () => {
    signInAs("probation");
    render(() => <SessionsSection />);
    expect(await screen.findByText("Kaiku desktop on Windows")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /End the session/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Sign out everywhere else" })).toBeNull();
    expect(screen.getByText("Sign in with a passkey to change this.")).toBeTruthy();
  });
});
