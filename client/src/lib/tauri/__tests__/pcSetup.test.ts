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

import {
  enterPcSetupCode,
  finishPcSetupPasskey,
  newPcSetupProof,
  pollPcSetup,
  startPcSetup,
  startPcSetupPasskey,
} from "../pcSetup";
import { isServerSetupComplete } from "../auth";
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

describe("the web page's proof", () => {
  it("is 32 fresh random bytes in base64url without padding", () => {
    const a = newPcSetupProof();
    const b = newPcSetupProof();
    expect(a).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(b).not.toBe(a);
  });

  it("travels in the body of the code request and the passkey start, never in the URL", async () => {
    const proof = newPcSetupProof();
    fetchMock.mockResolvedValueOnce(json(200, { state: "enrolled", enrollment_id: "e1" }));
    await enterPcSetupCode(ID, "123456", proof);
    fetchMock.mockResolvedValueOnce(json(200, { ceremony_id: "c1", options: {} }));
    await startPcSetupPasskey(ID, proof);
    const [codeUrl, codeInit] = fetchMock.mock.calls[0] as [string, RequestInit];
    const [startUrl, startInit] = fetchMock.mock.calls[1] as [string, RequestInit];
    expect(codeUrl).toBe(`https://chat.example.com/auth/pc-setups/${ID}/code`);
    expect(startUrl).toBe(`https://chat.example.com/auth/pc-setups/${ID}/passkey/start`);
    expect(JSON.parse(codeInit.body as string)).toEqual({ code: "123456", proof });
    expect(JSON.parse(startInit.body as string)).toEqual({ proof });
  });

  it("travels in the body of the passkey finish too", async () => {
    const proof = newPcSetupProof();
    fetchMock.mockResolvedValueOnce(json(201, { id: "p1" }));
    await finishPcSetupPasskey(ID, "c1", { id: "cred" }, "Firefox on Linux", proof);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(`https://chat.example.com/auth/pc-setups/${ID}/passkey/finish`);
    expect(JSON.parse(init.body as string)).toEqual({
      ceremony_id: "c1",
      credential: { id: "cred" },
      label: "Firefox on Linux",
      proof,
    });
  });

  it("the code, start and finish requests do not follow redirects", async () => {
    const proof = newPcSetupProof();
    fetchMock.mockResolvedValueOnce(json(200, { state: "enrolled", enrollment_id: "e1" }));
    await enterPcSetupCode(ID, "123456", proof);
    fetchMock.mockResolvedValueOnce(json(200, { ceremony_id: "c1", options: {} }));
    await startPcSetupPasskey(ID, proof);
    fetchMock.mockResolvedValueOnce(json(201, { id: "p1" }));
    await finishPcSetupPasskey(ID, "c1", {}, null, proof);
    for (const [, init] of fetchMock.mock.calls as [string, RequestInit][]) expect(init.redirect).toBe("error");
  });

  it("a redirect the browser refuses rejects the request", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await expect(enterPcSetupCode(ID, "123456", newPcSetupProof())).rejects.toBeInstanceOf(TypeError);
  });

  it("a finish counts as stored only on a 201 that carries the passkey's id", async () => {
    const proof = newPcSetupProof();
    for (const [code, body, stored] of [
      [201, { id: "p1", label: "x" }, true],
      [200, { id: "p1" }, false],
      [201, {}, false],
      [204, null, false],
    ] as const) {
      fetchMock.mockResolvedValueOnce(
        body === null ? new Response(null, { status: code }) : json(code, body),
      );
      const out = await finishPcSetupPasskey(ID, "c1", {}, null, proof);
      expect(out !== null, `${code} ${JSON.stringify(body)}`).toBe(stored);
    }
  });

  it("a page without the proof starts with an empty body", async () => {
    fetchMock.mockResolvedValueOnce(json(409, { error: "APPROVAL_STATE", message: "x" }));
    await expect(startPcSetupPasskey(ID, undefined)).rejects.toBeTruthy();
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toEqual({});
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

  it("aborting cancels the flow by its own cancel handle and rejects as ABORTED", async () => {
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

describe("an unreadable success answer", () => {
  it("is logged without any part of its body (it may carry the QR token)", async () => {
    const secret = "kaiku://pcsetup/v1?o=x&t=SECRETTOKENSECRETTOKEN";
    fetchMock.mockResolvedValue(
      new Response(`{"attempt_id":"${ID}","qr_payload":"${secret}"`, { status: 201 }),
    );
    const logged = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await expect(startPcSetup()).rejects.toThrow();
    const text = logged.mock.calls.flat().map(String).join(" ");
    expect(text).not.toContain("SECRETTOKEN");
    expect(text).not.toContain("qr_payload");
    logged.mockRestore();
  });
});

describe("isServerSetupComplete", () => {
  it("rejects an answer without a yes or no, so the could-not-check path runs", async () => {
    for (const body of [{}, { setup_complete: "true" }, null]) {
      fetchMock.mockResolvedValueOnce(json(200, body));
      await expect(isServerSetupComplete()).rejects.toThrow();
    }
    fetchMock.mockResolvedValueOnce(json(200, { setup_complete: false }));
    await expect(isServerSetupComplete()).resolves.toBe(false);
  });
});
