import { describe, expect, it } from "vitest";
import { t } from "../index";
import { errorText, CALL_START_ERRORS } from "../errors";
import { HttpError } from "@/lib/tauri/common";

describe("errorText", () => {
  it("maps known codes before status", () => {
    const err = new HttpError(401, "x", "INVALID_CREDENTIALS");
    const text = errorText(
      err,
      "auth.login.failed",
      { INVALID_CREDENTIALS: "auth.login.invalidCredentials" },
      { 401: "auth.forcedChange.wrongTemporary" },
    );
    expect(text).toBe("Invalid username or password.");
  });

  it("falls back to the status map when the code is unknown", () => {
    const err = new HttpError(409, "dup", "WHATEVER");
    expect(errorText(err, "auth.register.failed", {}, { 409: "auth.register.alreadyExists" })).toBe(
      t("auth.register.alreadyExists"),
    );
  });

  it("uses the fallback for unknown codes and bodiless errors", () => {
    const bodiless = errorText(new HttpError(502, "HTTP 502: Bad Gateway"), "auth.login.failed");
    expect(bodiless).toBe(t("errors.server"));
    expect(bodiless).not.toContain("502");

    const unknown = errorText(new HttpError(418, "teapot", "WHATEVER"), "auth.login.failed");
    expect(unknown).toBe(t("auth.login.failed"));
    expect(unknown).not.toContain("WHATEVER");
  });

  it("maps rate limits and server errors in every screen", () => {
    expect(errorText(new HttpError(429, "slow down"), "auth.login.failed")).toBe(t("errors.rateLimited"));
    expect(errorText(new HttpError(400, "x", "RATE_LIMITED"), "auth.login.failed")).toBe(
      t("errors.rateLimited"),
    );
    expect(errorText(new HttpError(500, "x", "INTERNAL_ERROR"), "auth.login.failed")).toBe(
      t("errors.server"),
    );
  });

  it("maps fetch TypeError to errors.network", () => {
    expect(errorText(new TypeError("Failed to fetch"), "auth.login.failed")).toBe(t("errors.network"));
  });

  it("shows validation messages verbatim", () => {
    const err = new HttpError(400, "Username must be 3–32 characters", "VALIDATION_ERROR");
    expect(errorText(err, "auth.register.failed")).toBe("Username must be 3–32 characters");
  });

  it("does not show the code for a validation error without a message", () => {
    const err = new HttpError(400, "VALIDATION_ERROR", "VALIDATION_ERROR");
    expect(errorText(err, "auth.register.failed")).toBe(t("auth.register.failed"));
  });

  it("does not show the code when message and code differ only in case", () => {
    const err = new HttpError(400, "validation_error", "VALIDATION_ERROR");
    expect(errorText(err, "auth.register.failed")).toBe(t("auth.register.failed"));
  });

  it("ignores prototype-shaped codes", () => {
    for (const code of ["constructor", "toString", "hasOwnProperty", "__proto__"]) {
      const text = errorText(
        new HttpError(400, "x", code),
        "auth.login.failed",
        { INVALID_CREDENTIALS: "auth.login.invalidCredentials" },
        {},
      );
      expect(text).toBe(t("auth.login.failed"));
    }
  });

  it("gives plain native strings the fallback", () => {
    expect(errorText("Session expired", "auth.changePassword.failed")).toBe(
      t("auth.changePassword.failed"),
    );
    expect(errorText(new Error("boom"), "auth.changePassword.failed")).toBe(
      t("auth.changePassword.failed"),
    );
  });

  it("looks through a wrapped cause", () => {
    const wrapped = new Error("Invalid credentials", {
      cause: new HttpError(401, "Invalid credentials", "INVALID_CREDENTIALS"),
    });
    expect(
      errorText(wrapped, "auth.login.failed", { INVALID_CREDENTIALS: "auth.login.invalidCredentials" }),
    ).toBe("Invalid username or password.");
  });

  it("maps a call to a blocked user to its own sentence", () => {
    const err = new HttpError(403, "blocked", "BLOCKED");
    expect(
      errorText(err, "voice.call.startFailed", CALL_START_ERRORS.byCode),
    ).toBe(t("voice.call.blocked"));
    expect(
      errorText(new HttpError(500, "x"), "voice.call.startFailed", CALL_START_ERRORS.byCode),
    ).toBe(t("errors.server"));
  });

  it("maps call_already_exists (any case) on call start", () => {
    for (const code of ["call_already_exists", "CALL_ALREADY_EXISTS"]) {
      expect(
        errorText(new HttpError(409, "x", code), "voice.call.startFailed", CALL_START_ERRORS.byCode),
      ).toBe(t("voice.call.error.alreadyExists"));
    }
  });

  it("maps CALL_CONFLICT on call start", () => {
    expect(
      errorText(new HttpError(409, "x", "CALL_CONFLICT"), "voice.call.startFailed", CALL_START_ERRORS.byCode),
    ).toBe(t("voice.call.error.conflict"));
  });
});
