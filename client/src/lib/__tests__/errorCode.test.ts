import { describe, expect, it } from "vitest";
import { codeIs, messageHasCode, normalizeCode } from "../errorCode";
import { errorText } from "@/i18n/errors";
import { t } from "@/i18n";
import { HttpError } from "../tauri/common";
import { isElevationRequiredError } from "../tauri/auth";
import { reauthErrorText } from "../reauth";
import { PasskeyError } from "../tauri/passkeys";

describe("normalizeCode", () => {
  it("trims and upper-cases strings, null for anything else", () => {
    expect(normalizeCode("elevation_required")).toBe("ELEVATION_REQUIRED");
    expect(normalizeCode("  Not_Found ")).toBe("NOT_FOUND");
    expect(normalizeCode("RESYNC")).toBe("RESYNC");
    expect(normalizeCode(undefined)).toBeNull();
    expect(normalizeCode(null)).toBeNull();
    expect(normalizeCode(42)).toBeNull();
    expect(normalizeCode({})).toBeNull();
    expect(normalizeCode("  ")).toBeNull();
  });
  it("codeIs / messageHasCode ignore case", () => {
    expect(codeIs("call_not_found", "CALL_NOT_FOUND")).toBe(true);
    expect(codeIs("CALL_NOT_FOUND", "call_not_found")).toBe(true);
    expect(codeIs(undefined, "X")).toBe(false);
    expect(messageHasCode('{"error":"elevation_required"}', "ELEVATION_REQUIRED")).toBe(true);
    expect(messageHasCode("x ELEVATION_REQUIRED y", "elevation_required")).toBe(true);
    expect(messageHasCode("other", "ELEVATION_REQUIRED")).toBe(false);
  });
});

describe("errorText code lookup is case-insensitive", () => {
  it.each(["call_not_found", "CALL_NOT_FOUND", "Call_Not_Found"])("byCode hit for %s", (code) => {
    const text = errorText(new HttpError(404, "x", code), "auth.login.failed", {
      CALL_NOT_FOUND: "chat.send.channelGone",
    });
    expect(text).toBe(t("chat.send.channelGone"));
  });
  it.each(["rate_limited", "RATE_LIMITED"])("common rate limit %s", (code) => {
    expect(errorText(new HttpError(400, "x", code), "auth.login.failed")).toBe(t("errors.rateLimited"));
  });
  it.each(["internal_error", "INTERNAL_ERROR"])("common internal %s", (code) => {
    expect(errorText(new HttpError(400, "x", code), "auth.login.failed")).toBe(t("errors.server"));
  });
  it.each(["validation_error", "VALIDATION_ERROR"])("validation message %s", (code) => {
    expect(errorText(new HttpError(400, "bad name", code), "auth.login.failed")).toBe("bad name");
  });
});

describe("isElevationRequiredError", () => {
  it.each(["elevation_required", "ELEVATION_REQUIRED"])("HttpError code %s", (code) => {
    expect(isElevationRequiredError(new HttpError(403, "x", code))).toBe(true);
  });
  it.each(['{"error":"elevation_required"}', '{"error":"ELEVATION_REQUIRED"}'])("native text %s", (s) => {
    expect(isElevationRequiredError(s)).toBe(true);
    expect(isElevationRequiredError(new Error(s))).toBe(true);
  });
  it("rejects others", () => {
    expect(isElevationRequiredError(new HttpError(403, "x", "NOT_ADMIN"))).toBe(false);
  });
});

describe("reauthErrorText", () => {
  it("looks codes up case-insensitively, also behind a cause", () => {
    expect(reauthErrorText(new HttpError(403, "x", "not_admin"))).toBe(t("security.stepUp.error.notAdmin"));
    expect(reauthErrorText(new Error("wrapped", { cause: new HttpError(403, "x", "STEP_UP_REQUIRED") }))).toBe(
      t("passkeys.error.stepUpRequired"),
    );
  });
  it("words passkey failures with LP's texts", () => {
    expect(reauthErrorText(new PasskeyError("PASSKEY_USER_MISMATCH"))).toBe(t("passkeys.error.userMismatch"));
    expect(reauthErrorText(new PasskeyError("PASSKEY_CANCELLED"))).toBe("");
  });
});
