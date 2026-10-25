import { describe, it, expect, vi, beforeEach } from "vitest";

const httpRequest = vi.fn();
vi.mock("../common", () => ({
  httpRequest: (...a: unknown[]) => httpRequest(...a),
  getAccessToken: vi.fn(),
  getServerUrl: vi.fn(),
}));

import { adminElevate } from "../admin";
import { mfaGenerateBackupCodes } from "../auth";

describe("re-auth requests", () => {
  beforeEach(() => httpRequest.mockReset());

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
