import { describe, it, expect } from "vitest";

describe("removed v0 security UI", () => {
  it("QR login modal and backup codes are gone", () => {
    const files = Object.keys({
      ...import.meta.glob("../../*.tsx"),
      ...import.meta.glob("../../../../lib/*.ts"),
    });
    expect(files.some((f) => /QrLoginModal|BackupCodesDisplay|qrLogin\.ts/.test(f))).toBe(false);
  });
});
