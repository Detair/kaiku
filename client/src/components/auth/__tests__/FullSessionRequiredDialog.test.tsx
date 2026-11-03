import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

import FullSessionRequiredDialog from "../FullSessionRequiredDialog";
import { browserState, FULL_SESSION_REQUIRED_EVENT, httpRequest } from "@/lib/tauri/common";
import { registerPasskeyCeremonies } from "@/lib/passkeySeam";
import { replaceSessionWithPasskey, setAuthState } from "@/stores/auth";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => {
  cleanup();
  fetchMock.mockReset();
  registerPasskeyCeremonies(null);
  setAuthState({ user: null });
  vi.useRealTimers();
});

describe("FullSessionRequiredDialog", () => {
  it("FULL_SESSION_REQUIRED opens the dialog once", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const seen = vi.fn();
    window.addEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "at";
    fetchMock.mockImplementation(async () => json(403, { error: "FULL_SESSION_REQUIRED", message: "x" }));
    render(() => <FullSessionRequiredDialog />);
    await expect(httpRequest("DELETE", "/api/approval-devices/x")).rejects.toMatchObject({
      code: "FULL_SESSION_REQUIRED",
    });
    await expect(httpRequest("DELETE", "/api/approval-devices/y")).rejects.toMatchObject({ status: 403 });
    expect(seen).toHaveBeenCalledTimes(1);
    expect(screen.getByText("This needs a sign-in with a passkey.")).toBeTruthy();
    expect(screen.getByText("Or do it in the Kaiku app on your phone.")).toBeTruthy();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2100);
    await expect(httpRequest("DELETE", "/api/approval-devices/z")).rejects.toBeTruthy();
    expect(seen).toHaveBeenCalledTimes(2);
    window.removeEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("replaceSessionWithPasskey ends the old session with credentials omit", async () => {
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "old-token";
    const signIn = vi.fn(async () => {
      browserState.accessToken = "new-token";
      return {
        user: { id: "u1", session: { id: "s2", assurance: "full" } },
        setup_required: false,
        password_change_required: false,
      };
    });
    registerPasskeyCeremonies({ signIn, stepUp: vi.fn() } as never);
    fetchMock.mockImplementation(async () => new Response(null, { status: 204 }));
    await replaceSessionWithPasskey();
    expect(signIn).toHaveBeenCalledWith("https://chat.example.com", expect.objectContaining({ mediation: "optional" }));
    const logout = fetchMock.mock.calls.find(([url]) => String(url).endsWith("/auth/logout"));
    expect(logout).toBeTruthy();
    const init = logout![1] as RequestInit;
    expect(init.method).toBe("POST");
    expect(init.credentials).toBe("omit");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer old-token");
  });
});
