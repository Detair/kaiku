import { describe, it, expect, vi, afterEach } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke,
  isTauri: () => true,
}));
const authStore = vi.hoisted(() => ({ login: vi.fn(), completeSignIn: vi.fn() }));
vi.mock("@/stores/auth", () => authStore);

import { leaveFlow, loginFlow, startPhoneSignIn } from "../loginFlow";

const ID = "44444444-4444-4444-8444-444444444444";

afterEach(async () => {
  await leaveFlow();
  vi.useRealTimers();
});

describe("login flow on the desktop", () => {
  it("desktop flow never reads a verifier", async () => {
    vi.useFakeTimers();
    const attempt = {
      attempt_id: ID,
      kind: "qr",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
      totp_available: false,
    };
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "login_attempt_create") return attempt;
      if (cmd === "login_attempt_poll") return { ...attempt, state: "approved", qr: null };
      if (cmd === "login_attempt_collect") return { user: { id: "u1" }, setup_required: false, password_change_required: false };
      if (cmd === "login_attempt_cancel") return null;
      return null;
    });
    authStore.completeSignIn.mockResolvedValue({ id: "u1" });
    await startPhoneSignIn("https://chat.example.com");
    await vi.advanceTimersByTimeAsync(2000);
    const commands = invoke.mock.calls.map(([cmd]) => cmd);
    expect(commands).toContain("login_attempt_create");
    expect(commands).toContain("login_attempt_poll");
    expect(commands).toContain("login_attempt_collect");
    const wire = JSON.stringify(invoke.mock.calls);
    expect(wire).not.toContain("verifier");
    expect(wire).not.toContain("client_challenge");
    expect(JSON.stringify(loginFlow())).not.toContain("verifier");
  });
});
