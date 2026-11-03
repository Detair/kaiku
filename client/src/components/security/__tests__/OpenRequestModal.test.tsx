import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ getMeSecurity: vi.fn(), cancelAccountRequest: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import OpenRequestModal, { resetOpenRequestDismissals } from "../OpenRequestModal";
import { refreshMeSecurity } from "@/stores/security";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "@/components/settings/security/__tests__/securityTestUtils";
import type { AccountRequestSummary } from "@/lib/types";

function request(over: Partial<AccountRequestSummary> = {}): AccountRequestSummary {
  return {
    id: "r1",
    subject: { user_id: "me", username: "alice", display_name: "Alice" },
    kind: "self",
    purpose: "recovery",
    state: "waiting",
    created_at: "2026-11-03T08:00:00Z",
    ready_at: "2026-11-04T08:00:00Z",
    started_by: null,
    lost_devices: [{ id: "d1", label: "Pixel 8" }],
    new_authenticator: null,
    cancel: { via: "step_up" },
    ...over,
  };
}

async function mount(req: AccountRequestSummary | null) {
  api.getMeSecurity.mockResolvedValue(security({ open_request: req }));
  await refreshMeSecurity();
  render(() => <OpenRequestModal />);
}

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  resetOpenRequestDismissals();
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("OpenRequestModal", () => {
  it("open request modal shows at start with cancel", async () => {
    api.cancelAccountRequest.mockResolvedValue(request({ state: "cancelled" }));
    await mount(request());
    expect(screen.getByText("Someone asked to change how you sign in to your account.")).toBeTruthy();
    expect(screen.getByText(/Pixel 8/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    await waitFor(() => expect(api.cancelAccountRequest).toHaveBeenCalledWith("r1", null, null));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "request.cancel", target: "r1" });
  });

  it("admin setup request names the admin", async () => {
    await mount(request({ kind: "admin", purpose: "admin_setup", started_by: { user_id: "a1", username: "fabian" } }));
    expect(screen.getByText("Set up by fabian. If that is right, there is nothing to do.")).toBeTruthy();
  });

  it("that's right closes until next start", async () => {
    await mount(request());
    fireEvent.click(screen.getByRole("button", { name: "That's right" }));
    await waitFor(() => expect(screen.queryByText(/Someone asked to change/)).toBeNull());
    cleanup();
    render(() => <OpenRequestModal />);
    expect(screen.queryByText(/Someone asked to change/)).toBeNull();
    resetOpenRequestDismissals();
    cleanup();
    render(() => <OpenRequestModal />);
    expect(screen.getByText(/Someone asked to change/)).toBeTruthy();
  });

  it("a completed request shows nothing", async () => {
    await mount(request({ state: "completed" }));
    expect(screen.queryByText(/Someone asked to change/)).toBeNull();
  });
});
