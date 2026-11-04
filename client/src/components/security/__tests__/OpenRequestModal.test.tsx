import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ getMeSecurity: vi.fn(), cancelAccountRequest: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import OpenRequestModal, { resetOpenRequestDismissals } from "../OpenRequestModal";
import { StepUpDialog } from "@/components/auth/StepUpDialog";
import type { PendingStepUp } from "@/lib/stepUp";
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

  it("Escape does not answer that's right (Argon review 2 #3)", async () => {
    await mount(request());
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByText(/Someone asked to change/)).toBeTruthy();
  });

  it("a dismissed waiting request comes back once it is ready", async () => {
    await mount(request());
    fireEvent.click(screen.getByRole("button", { name: "That's right" }));
    await waitFor(() => expect(screen.queryByText(/Someone asked to change/)).toBeNull());
    api.getMeSecurity.mockResolvedValue(security({ open_request: request({ state: "ready" }) }));
    await refreshMeSecurity();
    expect(await screen.findByText(/Someone asked to change/)).toBeTruthy();
  });

  it("a new session shows a dismissed request again", async () => {
    await mount(request());
    fireEvent.click(screen.getByRole("button", { name: "That's right" }));
    await waitFor(() => expect(screen.queryByText(/Someone asked to change/)).toBeNull());
    const { resetMeSecurity } = await import("@/stores/security");
    resetMeSecurity();
    await refreshMeSecurity();
    expect(await screen.findByText(/Someone asked to change/)).toBeTruthy();
  });

  it("a cancel that lost against completion says it is too late (Argon review 2 #2)", async () => {
    const { HttpError } = await import("@/lib/tauri/common");
    api.cancelAccountRequest.mockRejectedValue(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    await mount(request());
    api.getMeSecurity.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    expect(await screen.findByRole("status")).toBeTruthy();
    expect(api.getMeSecurity).toHaveBeenCalled();
  });

  it("the too-late answer stays on screen after the refresh, until dismissed (Flash r3 p1 #2)", async () => {
    const { HttpError } = await import("@/lib/tauri/common");
    api.cancelAccountRequest.mockRejectedValue(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    await mount(request());
    // The refresh after the 409 finds the request completed.
    api.getMeSecurity.mockResolvedValue(security({ open_request: request({ state: "completed" }) }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    await waitFor(() => expect(api.getMeSecurity).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.getByRole("status").textContent).toBeTruthy();
    // Nothing to cancel any more; the person closes it themselves.
    expect(screen.queryByRole("button", { name: "Cancel this request" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("a completed request shows nothing", async () => {
    await mount(request({ state: "completed" }));
    expect(screen.queryByText(/Someone asked to change/)).toBeNull();
  });
});

/** The z-index of the overlay that holds the dialog named `name`. */
function layerOf(name: string): number {
  const overlay = screen.getByRole("dialog", { name }).closest<HTMLElement>(".fixed");
  return Number(overlay?.style.zIndex || getComputedStyle(overlay!).zIndex);
}

describe("OpenRequestModal stacking (Codex review)", () => {
  const prompt = "Someone asked to change how you sign in to your account.";

  it("the part-2 confirmation opens above the prompt", async () => {
    await mount(request({ cancel: { via: "part2" } }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Confirm with a code" })).toBeTruthy());
    expect(layerOf("Confirm with a code")).toBeGreaterThan(layerOf(prompt));
  });

  it("the step-up dialog opens above the prompt", async () => {
    await mount(request());
    const pending = {
      req: { action: "request.cancel", target: "r1", targetLabel: "Alice" },
      mode: "phone",
      running: () => false,
      error: () => null,
      confirm: vi.fn(),
      cancel: vi.fn(),
    } as unknown as PendingStepUp;
    render(() => <StepUpDialog pending={pending} />);
    const stepUpLayer = Number(
      screen.getByTestId("step-up-dialog").closest<HTMLElement>(".fixed")!.style.zIndex,
    );
    expect(stepUpLayer).toBeGreaterThan(layerOf(prompt));
  });
});
