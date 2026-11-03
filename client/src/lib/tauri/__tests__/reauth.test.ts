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

  it("desktop has no provider re-authentication any more (C27, no native command)", async () => {
    tauriMode.on = true;
    await expect(startOidcReauth("kc", { mfaCode: "1" })).rejects.toMatchObject({
      status: 404,
      code: "REAUTH_UNAVAILABLE",
    });
    expect(invoke).not.toHaveBeenCalled();
    expect(httpRequest).not.toHaveBeenCalled();
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

  it("recognises the elevation-required error from both transports", () => {
    expect(isElevationRequiredError(new HttpError(403, "x", "elevation_required"))).toBe(true);
    expect(
      isElevationRequiredError('Link authorize failed (403 Forbidden): {"error":"elevation_required"}'),
    ).toBe(true);
    expect(isElevationRequiredError(new HttpError(403, "x", "not_admin"))).toBe(false);
    expect(isElevationRequiredError("other")).toBe(false);
  });
});
