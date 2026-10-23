import { afterEach, describe, expect, it, vi } from "vitest";
import { buildQrLoginUri } from "../qrLogin";

const { tauriMode } = vi.hoisted(() => ({ tauriMode: { native: false } }));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: () => tauriMode.native,
}));

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
    tauriMode.native = false;
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
    tauriMode.native = true;
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 401, json: vi.fn() }));
    const common = await import("../tauri/common");
    common.browserState.serverUrl = "tauri://localhost";
    expect(common.getServerUrl()).toBe("");
    common.browserState.serverUrl = "";
    expect(common.getServerUrl()).toBe("");
  });
});
