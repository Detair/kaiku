import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const list = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/adminSecurity", async (orig) => ({ ...(await orig<object>()), listOpenAccountRequests: list }));
const cancel = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), cancelAccountRequest: cancel }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import AdminRequestsPanel from "../AdminRequestsPanel";

afterEach(cleanup);

describe("AdminRequestsPanel", () => {
  it("open requests list cancels with a step-up", async () => {
    list.mockResolvedValue([
      { id: "r9", subject: { user_id: "opa", username: "opa", display_name: "Opa" }, kind: "self", purpose: "recovery", state: "waiting", created_at: "2026-11-03T08:00:00Z", ready_at: "2026-11-04T08:00:00Z", started_by: null, lost_devices: [], new_authenticator: null, cancel: { via: "step_up" } },
    ]);
    cancel.mockResolvedValue({ id: "r9", state: "cancelled" });
    render(() => <AdminRequestsPanel />);
    expect(await screen.findByText("Opa")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel the request of Opa" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("r9", null, null));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "request.cancel", target: "r9", targetLabel: "Opa" });
  });
});
