// Desktop: a "reused" report after a plain expiry still says so (Argon 5 #7);
// a native FULL_SESSION_REQUIRED opens the full-session dialog (Argon 5 #2).
import { describe, it, expect, vi, afterEach } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke,
  isTauri: () => true,
}));

import {
  expireNativeSession,
  FULL_SESSION_REQUIRED_EVENT,
  SESSION_ENDED_EVENT,
  setNativeAuthInfo,
} from "../common";
import { nativeSessionRequest } from "../auth";

afterEach(() => {
  invoke.mockReset();
  setNativeAuthInfo({ server_url: "https://chat.example.com", access_token: "at", expires_at_ms: 0 });
});

describe("native session end", () => {
  it("upgrades an earlier expiry to reused", () => {
    setNativeAuthInfo({ server_url: "https://chat.example.com", access_token: "at", expires_at_ms: 0 });
    const ended = vi.fn();
    const expired = vi.fn();
    window.addEventListener(SESSION_ENDED_EVENT, ended);
    window.addEventListener("kaiku:session-expired", expired);
    expireNativeSession("expired");
    expireNativeSession("reused");
    expireNativeSession("reused");
    expireNativeSession("expired");
    window.removeEventListener(SESSION_ENDED_EVENT, ended);
    window.removeEventListener("kaiku:session-expired", expired);
    expect(expired).toHaveBeenCalledTimes(1);
    expect(ended).toHaveBeenCalledTimes(1);
    expect((ended.mock.calls[0][0] as CustomEvent).detail).toEqual({ reason: "reused" });
  });

  it("a native FULL_SESSION_REQUIRED opens the full-session dialog", async () => {
    invoke.mockRejectedValue("FULL_SESSION_REQUIRED");
    const seen = vi.fn();
    window.addEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    await expect(nativeSessionRequest("DELETE", "/auth/sessions")).rejects.toMatchObject({
      status: 403,
      code: "FULL_SESSION_REQUIRED",
    });
    window.removeEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    expect(seen).toHaveBeenCalledTimes(1);
  });
});
