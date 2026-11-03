import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  repudiateEvent: vi.fn(),
  cancelAccountRequest: vi.fn(),
  removeKeyDevice: vi.fn(),
  confirmContactFor: vi.fn(),
}));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const admin = vi.hoisted(() => ({ adminRemoveAuthenticator: vi.fn() }));
vi.mock("@/lib/tauri/adminSecurity", async (orig) => ({ ...(await orig<object>()), ...admin }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import NoticeActions from "../NoticeActions";
import { HttpError } from "@/lib/tauri/common";
import { event, notice } from "./fixtures";
import type { NoticeAction } from "@/lib/types";

const repudiateStep: NoticeAction = { action: "repudiate", via: "step_up", warning: null };

beforeEach(() => {
  for (const f of [...Object.values(api), ...Object.values(admin)]) f.mockReset();
  stepUp.mockClear();
  api.repudiateEvent.mockResolvedValue({
    event: event(),
    ended_session_ids: ["s9"],
    revoked_authenticator_ids: [],
    compromised: ["password"],
    voided_request_ids: [],
  });
});
afterEach(cleanup);

function mount(actions: NoticeAction[], ev = event({ actions }), n = notice({}, [ev]), meId = "me") {
  render(() => <NoticeActions event={ev} notice={n} meId={meId} />);
}

describe("NoticeActions", () => {
  it("repudiate via step_up asks for a passkey for that event", async () => {
    mount([repudiateStep]);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    await waitFor(() => expect(api.repudiateEvent).toHaveBeenCalledWith("e1", null));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "event.repudiate", target: "e1" });
    expect(await screen.findByText("1 session signed out. Your password was marked unsafe: set a new one.")).toBeTruthy();
  });

  it("repudiate shows the voids-own-request warning first", async () => {
    mount([{ action: "repudiate", via: "step_up", warning: "voids_own_request" }]);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    expect(await screen.findByText("This also stops your own request to secure your account.")).toBeTruthy();
    expect(stepUp).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(api.repudiateEvent).toHaveBeenCalled());
  });

  it("voids_own_bootstrap adds that only an admin can help", async () => {
    mount([{ action: "repudiate", via: "step_up", warning: "voids_own_bootstrap" }]);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    expect(
      await screen.findByText("This also stops your own request to secure your account. Afterwards only an admin can help."),
    ).toBeTruthy();
  });

  it("repudiate via part2 sends the chosen factor", async () => {
    mount([{ action: "repudiate", via: "part2", warning: null }]);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Recovery code" }));
    expect(screen.getByText("A wrong code counts as a failed try.")).toBeTruthy();
    fireEvent.input(screen.getByLabelText("Code"), { target: { value: "abcde-fghjk mnp" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() =>
      expect(api.repudiateEvent).toHaveBeenCalledWith("e1", { type: "recovery_code", code: "ABCDEFGHJKMNP" }),
    );
    expect(stepUp).not.toHaveBeenCalled();
  });

  it("oidc repudiation adds the provider password hint", async () => {
    const ev = event({ actions: [repudiateStep], details: { auth_methods: ["oidc", "phone_confirm"] } });
    mount([repudiateStep], ev);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    expect(await screen.findByText(/Also change your password at your sign-in provider\./)).toBeTruthy();
  });

  it("an already reported event says so", async () => {
    api.repudiateEvent.mockRejectedValueOnce(new HttpError(409, "x", "EVENT_ALREADY_REPUDIATED"));
    mount([repudiateStep]);
    fireEvent.click(screen.getByRole("button", { name: "That wasn't me" }));
    expect(await screen.findByText("Already reported.")).toBeTruthy();
  });

  it("cancel lost against completion offers remove this device", async () => {
    const ev = event({
      event_type: "request_started",
      request_id: "r1",
      authenticator: { id: "d7", type: "key_device", label: "New phone" },
      actions: [{ action: "cancel_request", via: "step_up", warning: null }],
    });
    api.cancelAccountRequest.mockRejectedValueOnce(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    api.removeKeyDevice.mockResolvedValue({ id: "d7", state: "revoked" });
    mount(ev.actions, ev);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    expect(await screen.findByText("Too late: the request already completed.")).toBeTruthy();
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "request.cancel", target: "r1" });
    fireEvent.click(screen.getByRole("button", { name: "Remove this device" }));
    await waitFor(() => expect(api.removeKeyDevice).toHaveBeenCalledWith("d7"));
  });

  it("admin reader gets remove device now", async () => {
    const ev = event({
      event_type: "authenticator_added",
      subject_id: "oma",
      authenticator: { id: "d8", type: "key_device", label: "Unknown phone" },
      actions: [{ action: "remove_device", via: "step_up", warning: null }],
    });
    const n = notice({ subject: { user_id: "oma", username: "oma", display_name: "Oma" } }, [ev]);
    admin.adminRemoveAuthenticator.mockResolvedValue(undefined);
    mount(ev.actions, ev, n, "me");
    fireEvent.click(screen.getByRole("button", { name: "Remove device now" }));
    await waitFor(() => expect(admin.adminRemoveAuthenticator).toHaveBeenCalledWith("oma", "d8"));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.authenticator.remove", target: "d8" });
  });

  it("I will help confirms the contact without a step-up", async () => {
    const ev = event({ event_type: "recovery_contact_named", subject_id: "oma", actions: [{ action: "confirm_contact", via: null, warning: null }] });
    const n = notice({ subject: { user_id: "oma", username: "oma", display_name: "Oma" } }, [ev]);
    api.confirmContactFor.mockResolvedValue(undefined);
    mount(ev.actions, ev, n);
    fireEvent.click(screen.getByRole("button", { name: "I will help Oma" }));
    await waitFor(() => expect(api.confirmContactFor).toHaveBeenCalledWith("oma"));
    expect(stepUp).not.toHaveBeenCalled();
  });
});
