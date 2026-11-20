// "Set up this PC" transport: the PC's long-poll can be aborted (it holds a
// server slot), and the desktop hand-off runs the Rust `passkey_pc_setup`
// command with the attempt id as its cancel handle.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
const invoke = vi.hoisted(() => vi.fn());
const fetchMock = vi.hoisted(() => {
  // No session restore at import: the module-level refresh would hit fetch.
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

import { pollPcSetup } from "../pcSetup";
import { setUpThisPcInBrowser, PasskeyError } from "../passkeys";
import { browserState, clearBrowserTokens, setNativeAuthInfo } from "../common";

const ID = "00000000-0000-4000-8000-000000000001";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

beforeEach(() => {
  fetchMock.mockReset();
  invoke.mockReset();
  tauriMode.on = false;
  browserState.serverUrl = "https://chat.example.com";
  clearBrowserTokens();
});

afterEach(() => {
  clearBrowserTokens();
  setNativeAuthInfo(null);
});

describe("pollPcSetup", () => {
  it("hands its signal to the request, so closing the dialog ends the long-poll", async () => {
    fetchMock.mockResolvedValue(json(200, { attempt_id: ID, state: "waiting", reason: null }));
    const ctl = new AbortController();
    await pollPcSetup(ID, 20, ctl.signal);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(`https://chat.example.com/auth/pc-setups/${ID}?wait=20`);
    expect(init.signal).toBe(ctl.signal);
  });

  it("keeps the wait within the server's 0..25 s and carries no secret in the query", async () => {
    fetchMock.mockResolvedValue(json(200, { attempt_id: ID, state: "waiting", reason: null }));
    await pollPcSetup(ID, 99);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(new URL(url).search).toBe("?wait=25");
    expect(init.signal).toBeUndefined();
  });

  it("an aborted poll rejects", async () => {
    const ctl = new AbortController();
    fetchMock.mockImplementation(
      (_url: string, init: RequestInit) =>
        new Promise((_, reject) => {
          // Like fetch: an already aborted signal rejects at once.
          if (init.signal?.aborted) reject(new DOMException("aborted", "AbortError"));
          init.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        }),
    );
    const pending = pollPcSetup(ID, 20, ctl.signal);
    ctl.abort();
    await expect(pending).rejects.toBeTruthy();
  });
});

describe("setUpThisPcInBrowser", () => {
  beforeEach(() => {
    tauriMode.on = true;
    setNativeAuthInfo({ server_url: "https://chat.example.com/", access_token: "tok" } as never);
  });

  it("runs passkey_pc_setup with the server and the attempt id", async () => {
    invoke.mockResolvedValue(undefined);
    await setUpThisPcInBrowser(ID, new AbortController().signal);
    expect(invoke).toHaveBeenCalledWith("passkey_pc_setup", {
      serverUrl: "https://chat.example.com",
      attemptId: ID,
      cancelId: expect.any(String),
    });
  });

  it("aborting cancels the flow by the attempt id and rejects as ABORTED", async () => {
    let fail: (e: unknown) => void = () => undefined;
    invoke.mockImplementation((cmd: string) =>
      cmd === "passkey_pc_setup" ? new Promise((_, reject) => (fail = reject)) : Promise.resolve(undefined),
    );
    const ctl = new AbortController();
    const pending = setUpThisPcInBrowser(ID, ctl.signal);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("passkey_pc_setup", expect.anything()));
    ctl.abort();
    // The cancel handle is the invocation's own, never the server's attempt id.
    const cancelId = (invoke.mock.calls.find((c) => c[0] === "passkey_pc_setup")![1] as { cancelId: string }).cancelId;
    expect(cancelId).not.toBe(ID);
    expect(invoke).toHaveBeenCalledWith("passkey_cancel", { attemptId: cancelId });
    fail("PASSKEY_CANCELLED");
    await expect(pending).rejects.toMatchObject({ code: "ABORTED" });
  });

  it("a redeem refusal reaches the UI as its error code", async () => {
    invoke.mockRejectedValue("PASSKEY_LIMIT");
    await expect(setUpThisPcInBrowser(ID, new AbortController().signal)).rejects.toMatchObject({
      code: "PASSKEY_LIMIT",
    });
  });

  it("is desktop only", async () => {
    tauriMode.on = false;
    await expect(setUpThisPcInBrowser(ID, new AbortController().signal)).rejects.toBeInstanceOf(PasskeyError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("every invocation gets its own cancel handle", async () => {
    invoke.mockResolvedValue(undefined);
    await setUpThisPcInBrowser(ID, new AbortController().signal);
    await setUpThisPcInBrowser(ID, new AbortController().signal);
    const ids = invoke.mock.calls.filter((c) => c[0] === "passkey_pc_setup").map((c) => (c[1] as { cancelId: string }).cancelId);
    expect(ids).toHaveLength(2);
    expect(ids[0]).not.toBe(ids[1]);
  });
});
