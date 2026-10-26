import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const fetchMock = vi.fn();

function respond(status: number, body: unknown, statusText = "") {
  return {
    ok: false,
    status,
    statusText,
    text: vi.fn().mockResolvedValue(typeof body === "string" ? body : JSON.stringify(body)),
  };
}

describe("HttpError codes (C16)", () => {
  beforeEach(async () => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    vi.resetModules();
    const common = await import("../tauri/common");
    common.setNativeAuthInfo({
      server_url: "https://srv",
      access_token: "tok",
      expires_at_ms: Date.now() + 900_000,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("httpRequest puts the server error code on HttpError", async () => {
    const common = await import("../tauri/common");
    fetchMock.mockResolvedValue(
      respond(401, { error: "INVALID_CREDENTIALS", message: "Invalid credentials" }),
    );

    const err = await common.httpRequest("POST", "/auth/login", { a: 1 }).catch((e: unknown) => e);

    expect(err).toBeInstanceOf(common.HttpError);
    expect((err as InstanceType<typeof common.HttpError>).code).toBe("INVALID_CREDENTIALS");
    expect((err as Error).message).toBe("Invalid credentials");
  });

  it("httpRequest leaves the code empty when the body has none", async () => {
    const common = await import("../tauri/common");
    fetchMock.mockResolvedValue(respond(502, "<html>Bad Gateway</html>", "Bad Gateway"));

    const err = await common.httpRequest("GET", "/api/guilds").catch((e: unknown) => e);

    expect((err as InstanceType<typeof common.HttpError>).status).toBe(502);
    expect((err as InstanceType<typeof common.HttpError>).code).toBeUndefined();
  });
});
