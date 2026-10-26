import { describe, it, expect, vi, beforeEach } from "vitest";

const httpRequest = vi.fn();
const tauriMode = vi.hoisted(() => ({ on: false }));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), invoke }));
vi.mock("../common", async (orig) => ({
  ...(await orig<object>()),
  httpRequest: (...a: unknown[]) => httpRequest(...a),
  getAccessToken: vi.fn(),
  getServerUrl: () => "https://chat.example/",
  isTauri: () => tauriMode.on,
}));

import { adminElevate } from "../admin";
import {
  mfaGenerateBackupCodes,
  getReauthMethods,
  startOidcReauth,
  isElevationRequiredError,
} from "../auth";
import { HttpError } from "../common";

describe("re-auth requests", () => {
  beforeEach(() => {
    httpRequest.mockReset();
    invoke.mockReset();
    tauriMode.on = false;
  });

  it("loads the method list", async () => {
    httpRequest.mockResolvedValue({ password: true, mfa_required: false, oidc: [] });
    await getReauthMethods();
    expect(httpRequest).toHaveBeenCalledWith("GET", "/auth/reauth/methods");
  });

  it("browser authorize sends a null redirect and the code", async () => {
    httpRequest.mockResolvedValue({ url: "https://idp/x" });
    const r = await startOidcReauth("kc", { reason: "why", mfaCode: "123456" });
    expect(httpRequest).toHaveBeenCalledWith("POST", "/auth/reauth/oidc/authorize/kc", {
      redirect_uri: null,
      reason: "why",
      mfa_code: "123456",
    });
    expect(r).toEqual({ mode: "browser", url: "https://idp/x" });
  });

  it("desktop runs the native command and maps its error codes", async () => {
    tauriMode.on = true;
    invoke.mockResolvedValueOnce({ expires_at: null });
    await startOidcReauth("kc", { mfaCode: "1" });
    expect(invoke).toHaveBeenCalledWith("oidc_reauth", {
      serverUrl: "https://chat.example",
      providerId: "kc",
      reason: null,
      mfaCode: "1",
      attemptId: expect.any(String),
    });
    invoke.mockRejectedValueOnce("REAUTH_STALE");
    await expect(startOidcReauth("kc", {})).rejects.toMatchObject({ code: "REAUTH_STALE" });
    invoke.mockRejectedValueOnce("HTTP_429");
    await expect(startOidcReauth("kc", {})).rejects.toMatchObject({ status: 429 });
    // The status stays next to the code (lowercase server codes included).
    invoke.mockRejectedValueOnce("HTTP_503:service_unavailable");
    await expect(startOidcReauth("kc", {})).rejects.toMatchObject({
      status: 503,
      code: "service_unavailable",
    });
  });

  it("elevate sends password and code", async () => {
    httpRequest.mockResolvedValue({ elevated: true });
    await adminElevate({ password: "pw", mfa_code: "123456", reason: "audit" });
    expect(httpRequest).toHaveBeenCalledWith("POST", "/api/admin/elevate", {
      password: "pw",
      mfa_code: "123456",
      reason: "audit",
    });
  });

  it("elevate sends null for a missing code and reason", async () => {
    httpRequest.mockResolvedValue({ elevated: true });
    await adminElevate({ password: "pw", mfa_code: null });
    expect(httpRequest.mock.calls[0][2]).toEqual({
      password: "pw",
      mfa_code: null,
      reason: null,
    });
  });

  it("backup code regeneration sends the code", async () => {
    httpRequest.mockResolvedValue({ codes: ["a", "b"] });
    const codes = await mfaGenerateBackupCodes("654321");
    expect(httpRequest).toHaveBeenCalledWith("POST", "/auth/mfa/backup-codes", {
      mfa_code: "654321",
    });
    expect(codes).toEqual(["a", "b"]);
  });

  it("does not call the native command or the server for an aborted attempt", async () => {
    const ac = new AbortController();
    ac.abort();
    await expect(startOidcReauth("kc", { signal: ac.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
    tauriMode.on = true;
    await expect(startOidcReauth("kc", { signal: ac.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
    expect(invoke).not.toHaveBeenCalled();
    expect(httpRequest).not.toHaveBeenCalled();
  });

  it("cancels the native attempt with the same id when aborted mid-flow", async () => {
    tauriMode.on = true;
    const ac = new AbortController();
    let release: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd: string) =>
      cmd === "oidc_reauth" ? new Promise((r) => (release = r)) : Promise.resolve(),
    );
    const p = startOidcReauth("kc", { signal: ac.signal });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("oidc_reauth", expect.anything()));
    const id = (invoke.mock.calls[0][1] as { attemptId: string }).attemptId;
    ac.abort();
    expect(invoke).toHaveBeenCalledWith("oidc_reauth_cancel", { attemptId: id });
    release(undefined);
    await p;
  });

  it("recognises the elevation-required error from both transports", () => {
    expect(isElevationRequiredError(new HttpError(403, "x", "elevation_required"))).toBe(true);
    expect(
      isElevationRequiredError('Link authorize failed (403 Forbidden): {"error":"elevation_required"}'),
    ).toBe(true);
    expect(isElevationRequiredError(new HttpError(403, "x", "not_admin"))).toBe(false);
    expect(isElevationRequiredError("other")).toBe(false);
  });
});
