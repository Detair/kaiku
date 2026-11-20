// A non-admin who signs in while the first setup is open gets a plain notice,
// not a wizard they cannot complete.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const admin = vi.hoisted(() => ({ checkAdminStatus: vi.fn() }));
vi.mock("@/lib/tauri/admin", async (orig) => ({ ...(await orig<object>()), ...admin }));
const fetchMock = vi.hoisted(() => {
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

import { Suspense } from "solid-js";
import SetupWizard from "../SetupWizard";
import { authState, setAuthState } from "@/stores/auth";

beforeEach(() => {
  admin.checkAdminStatus.mockReset();
  fetchMock.mockReset();
  fetchMock.mockResolvedValue(
    new Response(JSON.stringify({ server_name: "Kaiku", registration_policy: "open" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }),
  );
  setAuthState({ setupRequired: true });
});

afterEach(() => {
  cleanup();
  setAuthState({ setupRequired: false });
});

const mount = () =>
  render(() => (
    <Suspense>
      <SetupWizard />
    </Suspense>
  ));

describe("SetupWizard", () => {
  it("a non-admin sees a closable notice instead of the wizard", async () => {
    admin.checkAdminStatus.mockResolvedValue({ is_admin: false, is_elevated: false, elevation_expires_at: null });
    mount();
    expect(await screen.findByText("This server is still being set up")).toBeTruthy();
    expect(screen.queryByTestId("setup-wizard")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(authState.setupRequired).toBe(false);
  });

  it("an admin gets the wizard", async () => {
    admin.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: false, elevation_expires_at: null });
    mount();
    expect(await screen.findByTestId("setup-wizard")).toBeTruthy();
  });
});
