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
  cancelEnrollment,
  claimStatus,
  confirmEnrollment,
  finishPasskeyClaim,
  getEnrollment,
  issueEnrollment,
  startPasskeyClaim,
  type EnrollmentIssuer,
} from "../enrollments";
import { browserState } from "../common";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

const ID = "55555555-5555-4555-8555-555555555555";
const TOKEN = "QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8";

beforeEach(() => {
  fetchMock.mockReset();
  invoke.mockReset();
  tauriMode.on = false;
  browserState.serverUrl = "https://chat.example.com";
  browserState.accessToken = "at";
  browserState.tokenServerUrl = "https://chat.example.com";
});

describe("enrolment wrappers", () => {
  it("confirmEnrollment uses the issuer base path for own, request and registration", async () => {
    const cases: [EnrollmentIssuer, string, boolean][] = [
      [{ kind: "own" }, "/auth/enrollments", true],
      [{ kind: "request" }, "/auth/account-requests/current/enrollments", false],
      [{ kind: "registration" }, "/auth/registration/enrollments", false],
    ];
    for (const [issuer, base, bearer] of cases) {
      fetchMock.mockResolvedValueOnce(json(200, { id: ID, state: "done", variant: "own" }));
      const e = await confirmEnrollment(issuer, ID, "123456");
      const [url, init] = fetchMock.mock.lastCall as [string, RequestInit];
      expect(url).toBe(`https://chat.example.com${base}/${ID}/confirm`);
      expect(JSON.parse(String(init.body))).toEqual({ code: "123456" });
      expect(Boolean((init.headers as Record<string, string>).Authorization)).toBe(bearer);
      expect(e.state).toBe("done");
    }
  });

  it("issue, get and cancel use the issuer base", async () => {
    fetchMock.mockResolvedValueOnce(json(201, { enrollment_id: ID, setup_token: TOKEN, setup_url: "x", expires_at: "t" }));
    await issueEnrollment({ kind: "own" });
    expect(fetchMock.mock.lastCall?.[0]).toBe("https://chat.example.com/auth/enrollments");
    fetchMock.mockResolvedValueOnce(json(200, { id: ID, state: "beamed" }));
    expect((await getEnrollment({ kind: "own" }, ID)).state).toBe("unknown");
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await cancelEnrollment({ kind: "own" }, ID);
    expect((fetchMock.mock.lastCall?.[1] as RequestInit).method).toBe("DELETE");
  });

  it("desktop request and ticket issuers go through pre_session_request", async () => {
    tauriMode.on = true;
    invoke.mockResolvedValue({ id: ID, state: "claimed" });
    await getEnrollment({ kind: "request" }, ID);
    expect(invoke).toHaveBeenCalledWith("pre_session_request", {
      kind: "request",
      method: "GET",
      path: `/auth/account-requests/current/enrollments/${ID}`,
      body: null,
    });
    await confirmEnrollment({ kind: "registration" }, ID, "654321");
    expect(invoke).toHaveBeenCalledWith("pre_session_request", {
      kind: "ticket",
      method: "POST",
      path: `/auth/registration/enrollments/${ID}/confirm`,
      body: { code: "654321" },
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("setup token travels only in bodies, never in a URL", async () => {
    fetchMock
      .mockResolvedValueOnce(json(200, { ceremony_id: "c1", options: {}, server_name: "S", subject: { username: "oma", display_name: "Oma" } }))
      .mockResolvedValueOnce(json(201, { enrollment_id: ID, state: "claimed", code: "123456", code_expires_at: "t" }))
      .mockResolvedValueOnce(json(200, { state: "done", authenticator_state: "active", ready_at: null }));
    await startPasskeyClaim(TOKEN);
    await finishPasskeyClaim(TOKEN, "c1", { id: "cred" }, null);
    await claimStatus(ID, 25);
    for (const [url, init] of fetchMock.mock.calls as [string, RequestInit][]) {
      expect(url).not.toContain(TOKEN);
      expect(init.credentials).toBe("include");
    }
    const bodies = (fetchMock.mock.calls as [string, RequestInit][]).map(([, i]) => JSON.parse(String(i.body)));
    expect(bodies[0]).toEqual({ setup_token: TOKEN });
    expect(bodies[1]).toEqual({ setup_token: TOKEN, ceremony_id: "c1", credential: { id: "cred" }, label: null });
    expect(bodies[2]).toEqual({ enrollment_id: ID, wait: 25 });
    expect(fetchMock.mock.calls[2][0]).toBe("https://chat.example.com/auth/enrollments/status");
  });
});
