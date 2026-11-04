import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const list = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/adminSecurity", async (orig) => ({ ...(await orig<object>()), listOpenAccountRequests: list }));
const cancel = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), cancelAccountRequest: cancel }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

const select = vi.hoisted(() => ({ selectUser: vi.fn(), loadUserDetails: vi.fn() }));
vi.mock("@/stores/admin", async (orig) => ({ ...(await orig<object>()), ...select }));

import AdminRequestsPanel from "../AdminRequestsPanel";
import { noteAccountRequestHint } from "@/stores/security";
import { HttpError } from "@/lib/tauri/common";

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

describe("AdminRequestsPanel updates (Argon review)", () => {
  const opa = {
    id: "r9",
    subject: { user_id: "opa", username: "opa", display_name: "Opa" },
    kind: "self",
    purpose: "recovery",
    state: "waiting",
    created_at: "2026-11-03T08:00:00Z",
    ready_at: "2026-11-04T08:00:00Z",
    started_by: null,
    lost_devices: [],
    new_authenticator: null,
    cancel: { via: "step_up" },
  };

  it("an account_request_updated hint reloads the list", async () => {
    list.mockReset();
    list.mockResolvedValueOnce([opa]).mockResolvedValueOnce([]);
    render(() => <AdminRequestsPanel />);
    expect(await screen.findByText("Opa")).toBeTruthy();
    noteAccountRequestHint();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText("Opa")).toBeNull());
  });

  it("a cancel that lost against completion offers to open the account", async () => {
    list.mockReset();
    list.mockResolvedValue([opa]);
    cancel.mockReset();
    cancel.mockRejectedValue(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    render(() => <AdminRequestsPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel the request of Opa" }));
    expect(
      await screen.findByText("Too late: the request already completed. If the new device is not theirs, remove it now."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open Opa" }));
    expect(select.selectUser).toHaveBeenCalledWith("opa");
    expect(select.loadUserDetails).toHaveBeenCalledWith("opa");
  });
});
