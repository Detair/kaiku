// Flash r3 p1 #3: the pre-session server is never the desktop app's own
// origin (tauri://localhost, http://tauri.localhost) or a non-http(s) URL.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));

async function load() {
  vi.resetModules();
  return import("../signInServer");
}

beforeEach(() => {
  sessionStorage.clear();
  vi.stubEnv("VITE_SERVER_URL", "");
});
afterEach(() => {
  vi.unstubAllEnvs();
  tauriMode.on = false;
});

describe("signInServer", () => {
  it("the desktop never falls back to its own window origin", async () => {
    tauriMode.on = true;
    const { signInServer } = await load();
    expect(signInServer()).toBe("");
  });

  it("the desktop's own origin is never taken as a choice", async () => {
    tauriMode.on = true;
    const { setSignInServer, signInServer } = await load();
    setSignInServer("http://tauri.localhost");
    setSignInServer("tauri://localhost");
    expect(signInServer()).toBe("");
    setSignInServer("https://chat.example.com/");
    expect(signInServer()).toBe("https://chat.example.com");
  });

  it("the browser falls back to its http(s) origin", async () => {
    const { signInServer } = await load();
    expect(signInServer()).toBe(window.location.origin);
  });
});
