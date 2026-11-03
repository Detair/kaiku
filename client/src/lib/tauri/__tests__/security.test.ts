import { describe, it, expect, vi, beforeEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
const invoke = vi.hoisted(() => vi.fn());
const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke,
  isTauri: () => tauriMode.on,
}));

import {
  cancelAccountRequest,
  confirmContactFor,
  createRecoveryCodes,
  getMeSecurity,
  getRecoveryCodesLeft,
  listContactFor,
  listKeyDevices,
  listMyAccountRequests,
  listSecurityEvents,
  removeKeyDevice,
  removePassword,
  removeRecoveryContact,
  repudiateEvent,
  setPassword,
  setRecoveryContact,
} from "../security";
import { browserState } from "../common";

function json(status: number, body: unknown): Response {
  return new Response(body === undefined ? "" : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function call(i = -1): { url: string; method: string; body: unknown } {
  const calls = fetchMock.mock.calls;
  const [url, init] = calls[i < 0 ? calls.length + i : i] as [string, RequestInit];
  return { url, method: String(init.method), body: init.body ? JSON.parse(String(init.body)) : undefined };
}

beforeEach(() => {
  fetchMock.mockReset();
  invoke.mockReset();
  tauriMode.on = false;
  browserState.serverUrl = "https://chat.example.com";
  browserState.accessToken = "at";
});

describe("security wrappers", () => {
  it("getMeSecurity drops unknown prompts and maps unknown states", async () => {
    fetchMock.mockResolvedValueOnce(
      json(200, {
        session: { assurance: "probation", auth_methods: ["qr"] },
        password: { state: "melted" },
        totp: null,
        recovery_codes_left: 3,
        key_devices: 1,
        passkeys: 0,
        may_approve: true,
        bootstrap_open: false,
        recovery_contact: null,
        open_request: null,
        prompts: ["add_passkey", "fly_to_moon", "set_up_phone"],
      }),
    );
    const s = await getMeSecurity();
    expect(call().url).toBe("https://chat.example.com/api/me/security");
    expect(s.prompts).toEqual(["add_passkey", "set_up_phone"]);
    expect(s.password?.state).toBe("unknown");
  });

  it("security events page with before and limit, unknown types decode as unknown", async () => {
    fetchMock.mockResolvedValueOnce(
      json(200, {
        items: [{ id: "e1", event_type: "warp_drive", severity: "loud", actions: [{ action: "x", via: null, warning: null }] }],
        has_more: true,
        next_cursor: "e1",
      }),
    );
    const page = await listSecurityEvents("e0", 20);
    expect(call().url).toBe("https://chat.example.com/api/me/security-events?before=e0&limit=20");
    expect(page.items[0].event_type).toBe("unknown");
    expect(page.items[0].severity).toBe("normal");
    expect(page.items[0].actions[0].action).toBe("unknown");
  });

  it("repudiate sends part2 as a factor only when given", async () => {
    fetchMock.mockImplementation(async () => json(200, { event: { id: "e1", event_type: "sign_in", actions: [] }, ended_session_ids: [], revoked_authenticator_ids: [], compromised: [], voided_request_ids: [] }));
    await repudiateEvent("e1", null);
    expect(call()).toMatchObject({ url: "https://chat.example.com/api/me/security-events/e1/repudiate", body: { part2: null } });
    await repudiateEvent("e1", { type: "totp", code: "123456" });
    expect(call().body).toEqual({ part2: { factor: { type: "totp", code: "123456" } } });
  });

  it("cancelAccountRequest sends part2 only when given", async () => {
    fetchMock.mockImplementation(async () => json(200, { id: "r1", state: "cancelled" }));
    await cancelAccountRequest("r1", null, null);
    expect(call()).toMatchObject({ url: "https://chat.example.com/api/account-requests/r1/cancel", body: { reason: null, part2: null } });
    await cancelAccountRequest("r1", "not me", { type: "recovery_code", code: "ABCD" });
    expect(call().body).toEqual({ reason: "not me", part2: { factor: { type: "recovery_code", code: "ABCD" } } });
  });

  it("key devices, recovery codes, password and contact use their routes", async () => {
    fetchMock.mockResolvedValueOnce(json(200, { devices: [{ id: "d1", state: "zombie" }] }));
    const devices = await listKeyDevices();
    expect(devices[0].state).toBe("unknown");
    fetchMock.mockResolvedValueOnce(json(200, { id: "d1", state: "revoked" }));
    await removeKeyDevice("d1");
    expect(call()).toMatchObject({ method: "DELETE", url: "https://chat.example.com/api/approval-devices/d1" });
    fetchMock.mockResolvedValueOnce(json(200, { left: 7 }));
    expect(await getRecoveryCodesLeft()).toBe(7);
    fetchMock.mockResolvedValueOnce(json(200, { recovery_codes: ["A", "B"] }));
    expect(await createRecoveryCodes()).toEqual(["A", "B"]);
    expect(call()).toMatchObject({ method: "POST", url: "https://chat.example.com/auth/recovery-codes" });
    fetchMock.mockResolvedValueOnce(json(200, {}));
    await setPassword("correct horse");
    expect(call()).toMatchObject({ method: "POST", url: "https://chat.example.com/auth/me/password", body: { new_password: "correct horse" } });
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await removePassword();
    expect(call()).toMatchObject({ method: "DELETE", url: "https://chat.example.com/auth/me/password" });
    fetchMock.mockResolvedValueOnce(json(200, { user_id: "c1" }));
    await setRecoveryContact("c1");
    expect(call()).toMatchObject({ method: "PUT", url: "https://chat.example.com/api/me/recovery-contact", body: { user_id: "c1" } });
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await removeRecoveryContact();
    expect(call()).toMatchObject({ method: "DELETE", url: "https://chat.example.com/api/me/recovery-contact" });
    fetchMock.mockResolvedValueOnce(json(200, { accounts: [{ user_id: "o1" }] }));
    expect(await listContactFor()).toEqual([{ user_id: "o1" }]);
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await confirmContactFor("o1");
    expect(call()).toMatchObject({ method: "POST", url: "https://chat.example.com/api/me/recovery-contact-for/o1/confirm" });
    fetchMock.mockResolvedValueOnce(json(200, { requests: [{ id: "r1", state: "waiting" }] }));
    expect((await listMyAccountRequests())[0].state).toBe("waiting");
  });

  it("desktop sets the password through the session request", async () => {
    tauriMode.on = true;
    invoke.mockResolvedValue(null);
    await setPassword("pw-new-123");
    expect(invoke).toHaveBeenCalledWith("session_request", {
      method: "POST",
      path: "/auth/me/password",
      body: { new_password: "pw-new-123" },
    });
  });
});
