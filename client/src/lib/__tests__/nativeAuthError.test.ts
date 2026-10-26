import { describe, expect, it } from "vitest";
import { nativeAuthError } from "../tauri/auth";
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

  it("passes unknown errors through unchanged", () => {
    const other = new Error("boom");
    expect(nativeAuthError(other)).toBe(other);
    expect(nativeAuthError("something else")).toBe("something else");
  });
});
