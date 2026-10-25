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
import { mfaGenerateBackupCodes, getReauthMethods, startOidcReauth } from "../auth";

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
    });
    invoke.mockRejectedValueOnce("REAUTH_STALE");
    await expect(startOidcReauth("kc", {})).rejects.toMatchObject({ code: "REAUTH_STALE" });
    invoke.mockRejectedValueOnce("HTTP_429");
    await expect(startOidcReauth("kc", {})).rejects.toMatchObject({ status: 429 });
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
});
