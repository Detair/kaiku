// Review round 2 (Codex 2.3): a passkey sign-in whose signal is already
// aborted (e.g. cancelled while the desktop app set up its auth events)
// starts nothing.
import { describe, expect, it, vi } from "vitest";

const env = vi.hoisted(() => ({ tauri: true, ctl: null as AbortController | null }));

vi.mock("@/lib/tauri", async () => {
  const passkeys = await vi.importActual<typeof import("@/lib/tauri/passkeys")>("@/lib/tauri/passkeys");
  return {
    // The cancel arrives while the native auth events are set up.
    initNativeAuthEvents: vi.fn(async () => env.ctl?.abort()),
    passkeySignIn: vi.fn(),
    getServerUrl: vi.fn(() => ""),
    PasskeyError: passkeys.PasskeyError,
  };
});
vi.mock("@/lib/tauri/common", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/common")),
  isTauri: () => env.tauri,
}));

import * as tauri from "@/lib/tauri";
import { loginWithPasskey } from "../auth";

describe("loginWithPasskey", () => {
  it("an already aborted signal starts no sign-in", async () => {
    env.ctl = new AbortController();
    await expect(
      loginWithPasskey("https://chat.example.com", { mediation: "optional", signal: env.ctl.signal }),
    ).rejects.toMatchObject({ code: "ABORTED" });
    expect(tauri.passkeySignIn).not.toHaveBeenCalled();
  });
});
