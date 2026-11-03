// Flash r3 p1 #5: a PASSWORD_REJECTED without its details is still worded.
import { describe, it, expect } from "vitest";
import { securityErrorText } from "../securityErrors";
import { HttpError } from "@/lib/tauri/common";

describe("securityErrorText", () => {
  it("a PASSWORD_REJECTED without details gets the generic sentence", () => {
    const err = new HttpError(400, "x", "PASSWORD_REJECTED", null as never);
    expect(securityErrorText(err, "security.password.failed")).toBe(
      "This password can't be used. Choose another one.",
    );
  });
});

describe("amendment codes (C26/C27, 2026-11-03)", () => {
  it("a phone that is already a key of the account says so", () => {
    expect(securityErrorText(new HttpError(400, "x", "ENROLLMENT_DEVICE_IN_USE"), "security.setupPhone.confirmFailed")).toBe(
      "This phone is already a key for this account. Sign in with it instead.",
    );
  });

  it("an admin setup code for the admin's own account points to their own settings", () => {
    expect(securityErrorText(new HttpError(400, "x", "ENROLLMENT_TARGET_SELF"), "security.setupPhone.confirmFailed")).toBe(
      "For your own account, use Set up a phone in your security settings.",
    );
  });

  it("503 SERVICE_UNAVAILABLE asks to try again", () => {
    expect(securityErrorText(new HttpError(503, "x", "SERVICE_UNAVAILABLE"), "security.setupPhone.confirmFailed")).toBe(
      "The server can't do this right now. Please try again in a moment.",
    );
  });
});
