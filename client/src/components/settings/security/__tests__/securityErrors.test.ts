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
