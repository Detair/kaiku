import { afterEach, describe, expect, it, vi } from "vitest";

async function loadServerUrl(stored: string | null, env?: string): Promise<string> {
  vi.resetModules();
  localStorage.clear();
  if (stored) localStorage.setItem("serverUrl", stored);
  if (env) vi.stubEnv("VITE_SERVER_URL", env);
  const mod = await import("../tauri/common");
  return mod.getServerUrl();
}

describe("stored server URL in browser mode", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    localStorage.clear();
  });

  it("ignores a stored URL from another origin", async () => {
    expect(await loadServerUrl("https://old.example.com")).toBe(window.location.origin);
  });

  it("keeps a stored URL for the current origin", async () => {
    const same = `${window.location.origin}`;
    expect(await loadServerUrl(same)).toBe(same);
  });

  it("lets VITE_SERVER_URL setups keep the stored URL", async () => {
    expect(await loadServerUrl("http://dev.example:3000", "http://dev.example:3000")).toBe(
      "http://dev.example:3000",
    );
  });
});
