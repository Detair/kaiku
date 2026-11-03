import { describe, expect, it } from "vitest";
import { nativeAuthError, nativeSsoError } from "../tauri/auth";
import { errorText, SSO_ERRORS } from "@/i18n/errors";
import { HttpError } from "../tauri/common";

describe("nativeAuthError", () => {
  it("maps the Rust invalid-credentials string to INVALID_CREDENTIALS", () => {
    const err = nativeAuthError("Invalid username or password");
    expect(err).toBeInstanceOf(HttpError);
    expect((err as HttpError).status).toBe(401);
    expect((err as HttpError).code).toBe("INVALID_CREDENTIALS");
  });

  it("maps the Rust suspended-account string to ACCOUNT_BANNED", () => {
    const err = nativeAuthError("ACCOUNT_BANNED") as HttpError;
    expect(err).toBeInstanceOf(HttpError);
    expect(err.status).toBe(403);
    expect(err.code).toBe("ACCOUNT_BANNED");
  });

  it("maps a connection failure to a TypeError (network)", () => {
    expect(nativeAuthError("Connection failed: refused")).toBeInstanceOf(TypeError);
  });

  it("maps the duplicate-user string to a 409 without a code", () => {
    const err = nativeAuthError("Username or email already exists") as HttpError;
    expect(err).toBeInstanceOf(HttpError);
    expect(err.status).toBe(409);
    expect(err.code).toBeUndefined();
  });

  it("turns a bare server code into an HttpError with its code", () => {
    expect(nativeAuthError("LOGIN_ATTEMPT_ENDED")).toMatchObject({ status: 410, code: "LOGIN_ATTEMPT_ENDED" });
    expect(nativeAuthError("TOO_MANY_ATTEMPTS:90")).toMatchObject({
      status: 429,
      code: "TOO_MANY_ATTEMPTS",
      retryAfter: 90,
    });
    expect(nativeAuthError("CLIENT_OUTDATED")).toMatchObject({ status: 400, code: "CLIENT_OUTDATED" });
  });

  it("keeps the extra error-body fields of a native code", () => {
    expect(nativeAuthError('ENROLL_CODE_WRONG::{"tries_left":2}')).toMatchObject({
      code: "ENROLL_CODE_WRONG",
      details: { tries_left: 2 },
    });
    expect(nativeAuthError('TOO_MANY_ATTEMPTS:60:{"x":1}')).toMatchObject({ retryAfter: 60, details: { x: 1 } });
  });

  it("maps a native proxy error to its status (transient)", () => {
    expect(nativeAuthError("Server error (502)")).toMatchObject({ status: 502 });
  });

  it("passes unknown errors through unchanged", () => {
    const other = new Error("boom");
    expect(nativeAuthError(other)).toBe(other);
    expect(nativeAuthError("something else")).toBe("something else");
  });
});

describe("nativeSsoError", () => {
  it("gives the SSO codes from the native flows an HttpError code", () => {
    for (const code of [
      "OIDC_CLIENT_OUTDATED",
      "OIDC_HANDOFF_INVALID",
      "OIDC_PROVIDER_ERROR",
      "OIDC_EXCHANGE_FAILED",
      "IDENTITY_ALREADY_LINKED",
    ]) {
      const err = nativeSsoError(code) as HttpError;
      expect(err).toBeInstanceOf(HttpError);
      expect(err.code).toBe(code);
    }
  });

  it("leaves other errors to nativeAuthError", () => {
    expect(nativeSsoError("MFA_REQUIRED")).toMatchObject({ code: "MFA_REQUIRED" });
    expect(nativeSsoError("Connection failed: refused")).toBeInstanceOf(TypeError);
    expect(nativeSsoError("OIDC login timed out (60s). Please try again.")).toBe(
      "OIDC login timed out (60s). Please try again.",
    );
  });

  it("maps to readable texts via SSO_ERRORS", () => {
    const text = (code: string) =>
      errorText(nativeSsoError(code), "auth.login.ssoFailed", SSO_ERRORS.byCode);
    expect(text("OIDC_CLIENT_OUTDATED")).toBe("Update Kaiku to sign in with this server.");
    expect(text("OIDC_HANDOFF_INVALID")).toBe(
      "Sign-in could not be completed. Please try again.",
    );
    expect(text("OIDC_PROVIDER_ERROR")).not.toContain("OIDC_");
    expect(text("OIDC_EXCHANGE_FAILED")).not.toContain("OIDC_");
    expect(text("IDENTITY_ALREADY_LINKED")).toBe(
      "That account is already linked to another Kaiku account.",
    );
  });
});
