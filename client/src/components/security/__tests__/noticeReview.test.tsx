// PR #746 review round 1: unknown C28 values never crash the Kaiku DM
// (Fable M2), every event of a coalesced notice can be acted on (Codex M5),
// removal goes by authenticator type (Codex M4), follow-ups without a named
// device (Argon 2 #2), no empty request cancel (2 #4), literal values (m4).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  repudiateEvent: vi.fn(),
  cancelAccountRequest: vi.fn(),
  removeKeyDevice: vi.fn(),
  confirmContactFor: vi.fn(),
}));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const passkeys = vi.hoisted(() => ({ deletePasskey: vi.fn() }));
vi.mock("@/lib/tauri/passkeys", async (orig) => ({ ...(await orig<object>()), ...passkeys }));
const admin = vi.hoisted(() => ({ adminRemoveAuthenticator: vi.fn() }));
vi.mock("@/lib/tauri/adminSecurity", async (orig) => ({ ...(await orig<object>()), ...admin }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));
const security = vi.hoisted(() => ({ refreshMeSecurity: vi.fn(async () => undefined) }));
vi.mock("@/stores/security", async (orig) => ({ ...(await orig<object>()), ...security }));

import SecurityNoticeCard from "../SecurityNoticeCard";
import { noticeKey } from "../noticeText";
import { removeDevice, runNoticeAction } from "../runNoticeAction";
import { withDecodedNotice } from "@/stores/messages";
import { t } from "@/i18n";
import { HttpError } from "@/lib/tauri/common";
import { event, notice } from "./fixtures";
import type { Message, SecurityNotice } from "@/lib/types";

beforeEach(() => {
  for (const f of [...Object.values(api), ...Object.values(admin), ...Object.values(passkeys)]) f.mockReset();
  stepUp.mockClear();
  security.refreshMeSecurity.mockClear();
});
afterEach(cleanup);

describe("unknown C28 values", () => {
  it("a raw security message with a new event type and action renders the generic sentence", () => {
    const raw = {
      id: "m1",
      channel_id: "k1",
      message_type: "security",
      security_notice: {
        ...notice({}, [event()]),
        event_type: "something_new",
        events: [{ ...event(), event_type: "something_new", actions: [{ action: "new_action", via: "step_up", warning: "new_warning" }] }],
      },
    } as unknown as Message;
    const decoded = withDecodedNotice(raw);
    render(() => <SecurityNoticeCard notice={decoded.security_notice!} meId="me" />);
    expect(screen.getByText(`${t("security.notice.generic", { subject: "your account" })}.`, { exact: false })).toBeTruthy();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("lookups are total, also for prototype names", () => {
    expect(noticeKey("toString")).toBe("security.notice.generic");
    expect(noticeKey(undefined)).toBe("security.notice.generic");
    expect(noticeKey("sign_in")).toBe("security.notice.signIn");
  });
});

describe("coalesced notices", () => {
  it("the sixth event of a coalesced notice can still be repudiated", async () => {
    const events = Array.from({ length: 6 }, (_, i) =>
      event({ id: `e${i + 1}`, actions: [{ action: "repudiate", via: "step_up", warning: null }] }),
    );
    api.repudiateEvent.mockResolvedValue({
      event: events[5],
      ended_session_ids: [],
      revoked_authenticator_ids: [],
      compromised: [],
      voided_request_ids: [],
    });
    render(() => <SecurityNoticeCard notice={notice({ count: 6 }, events)} meId="me" />);
    expect(screen.getAllByRole("button", { name: "That wasn't me" })).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: "Show all 6 events" }));
    const buttons = screen.getAllByRole("button", { name: "That wasn't me" });
    expect(buttons).toHaveLength(6);
    fireEvent.click(buttons[5]);
    await waitFor(() => expect(api.repudiateEvent).toHaveBeenCalledWith("e6", null));
  });
});

describe("removing a device from a notice", () => {
  const own = (type: string): [ReturnType<typeof event>, SecurityNotice] => {
    const ev = event({ authenticator: { id: "a1", type, label: "Laptop" } });
    return [ev, notice({}, [ev])];
  };

  it("a passkey goes through passkey.remove and LP's delete", async () => {
    passkeys.deletePasskey.mockResolvedValue({ rp_id: "chat.example.com", user_handle: "dQ", credential_id: "Yw", remaining_credential_ids: [] });
    const [ev, n] = own("passkey");
    await removeDevice(ev, n, "me");
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "passkey.remove", target: "a1" });
    expect(passkeys.deletePasskey).toHaveBeenCalledWith("a1");
    expect(api.removeKeyDevice).not.toHaveBeenCalled();
  });

  it("a key device keeps its own route", async () => {
    const [ev, n] = own("key_device");
    await removeDevice(ev, n, "me");
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "key_device.remove", target: "a1" });
    expect(api.removeKeyDevice).toHaveBeenCalledWith("a1");
  });

  it("an unknown authenticator type is refused", async () => {
    const [ev, n] = own("future_kind");
    await expect(removeDevice(ev, n, "me")).rejects.toThrow("AUTHENTICATOR_TYPE_UNKNOWN");
    expect(stepUp).not.toHaveBeenCalled();
  });
});

describe("cancel request", () => {
  const cancelAction = { action: "cancel_request" as const, via: "step_up" as const, warning: null };

  it("a cancel without a named request does nothing", async () => {
    const ev = event({ event_type: "request_started", request_id: null, actions: [cancelAction] });
    const r = await runNoticeAction(cancelAction, ev, notice({}, [ev]), "me");
    expect(r).toEqual({ kind: "cancelled" });
    expect(stepUp).not.toHaveBeenCalled();
  });

  it("too late without a named device: the owner goes to settings, a contact asks an admin", async () => {
    api.cancelAccountRequest.mockRejectedValue(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    const ev = event({ event_type: "request_started", request_id: "r1", authenticator: null, actions: [cancelAction] });
    const own = await runNoticeAction(cancelAction, ev, notice({}, [ev]), "me");
    expect(own).toMatchObject({ kind: "tooLate", followUp: "openSecurity" });
    const other = notice({ subject: { user_id: "oma", username: "oma", display_name: "Oma" } }, [ev]);
    const contact = await runNoticeAction(cancelAction, ev, other, "me", { isAdmin: false });
    expect(contact).toMatchObject({ kind: "tooLate", followUp: "askAdmin" });
    expect(security.refreshMeSecurity).toHaveBeenCalled();
  });
});

describe("template values", () => {
  it("a name with $ sequences is inserted literally", () => {
    expect(t("security.notice.accountOf", { name: "$& and $` and {{count}}" })).toContain("$& and $` and {{count}}");
  });
});
