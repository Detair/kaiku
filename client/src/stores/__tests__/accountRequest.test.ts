// Flash r3 p2 #3: the request is read again from the server it was started
// on (requestServer), not whatever the transport points at.
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

import { reloadRequest, request } from "../accountRequest";
import { setSignInServer } from "@/lib/signInServer";
import { browserState, clearBrowserTokens } from "@/lib/tauri/common";

const current = { id: "r1", kind: "self", purpose: "recovery", state: "waiting", created_at: "a", ready_at: "b", expires_at: null, factors: ["recovery_code", "password"], lost_key_devices: 0, staged: { authenticator: null, password: false, totp: false, recovery_contact: null } };

beforeEach(() => {
  fetchMock.mockReset();
  clearBrowserTokens();
});

describe("reloadRequest", () => {
  it("asks the server the request was started on", async () => {
    setSignInServer("https://other.example.org");
    browserState.serverUrl = "https://chat.example.com";
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify(current), { status: 200, headers: { "Content-Type": "application/json" } }),
    );
    await reloadRequest();
    expect(String(fetchMock.mock.calls[0][0])).toBe("https://other.example.org/auth/account-requests/current");
    expect(request()?.id).toBe("r1");
  });
});
