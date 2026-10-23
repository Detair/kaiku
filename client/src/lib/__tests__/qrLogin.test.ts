import { afterEach, describe, expect, it, vi } from "vitest";
import { buildQrLoginUri } from "../qrLogin";

describe("buildQrLoginUri", () => {
  it("encodes server and token", () => {
    expect(buildQrLoginUri("https://kaiku.example", "a+b")).toBe(
      "kaiku://qr/login?server=https%3A%2F%2Fkaiku.example&token=a%2Bb",
    );
  });

  it("rejects an empty server", () => {
    expect(() => buildQrLoginUri("", "t")).toThrow("QR_NO_SERVER");
    expect(() => buildQrLoginUri("tauri://localhost", "t")).toThrow("QR_NO_SERVER");
  });

  it("rejects values that are not absolute http(s) URLs", () => {
    expect(() => buildQrLoginUri("kaiku.example", "t")).toThrow("QR_NO_SERVER");
    expect(() => buildQrLoginUri("ftp://kaiku.example", "t")).toThrow("QR_NO_SERVER");
  });
});

describe("getServerUrl", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
    localStorage.removeItem("serverUrl");
  });

  it("falls back to the page origin in browser mode", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 401, json: vi.fn() }));
    const common = await import("../tauri/common");
    common.browserState.serverUrl = "";
    expect(common.getServerUrl()).toBe(window.location.origin);
    common.browserState.serverUrl = "https://kaiku.example";
    expect(common.getServerUrl()).toBe("https://kaiku.example");
  });

  it("never returns a tauri:// origin in native mode", async () => {
    vi.stubGlobal("__TAURI__", {});
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 401, json: vi.fn() }));
    (window as unknown as Record<string, unknown>).__TAURI__ = {};
    const common = await import("../tauri/common");
    delete (window as unknown as Record<string, unknown>).__TAURI__;
    common.browserState.serverUrl = "tauri://localhost";
    expect(common.getServerUrl()).toBe("");
    common.browserState.serverUrl = "";
    expect(common.getServerUrl()).toBe("");
  });
});
