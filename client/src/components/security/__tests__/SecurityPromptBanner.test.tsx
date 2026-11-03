import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ getMeSecurity: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const nav = vi.hoisted(() => vi.fn());
vi.mock("@solidjs/router", async (orig) => ({ ...(await orig<object>()), useNavigate: () => nav }));

import SecurityPromptBanner from "../SecurityPromptBanner";
import { currentPrompt, refreshMeSecurity, snoozePrompt } from "@/stores/security";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "@/components/settings/security/__tests__/securityTestUtils";
import type { SecurityPrompt } from "@/lib/types";

async function load(prompts: SecurityPrompt[]) {
  api.getMeSecurity.mockResolvedValue(security({ prompts }));
  await refreshMeSecurity();
}

beforeEach(() => {
  localStorage.clear();
  nav.mockReset();
  signInAs("full");
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  setAuthState({ user: null });
});

describe("security prompts", () => {
  it("prompts are shown one at a time in priority order", async () => {
    await load(["renew_recovery_codes", "add_passkey", "secure_my_account", "open_request"]);
    expect(currentPrompt()).toBe("secure_my_account");
    render(() => <SecurityPromptBanner />);
    expect(screen.getByText("Your account is not secured yet. Secure it now, so only you can sign in.")).toBeTruthy();
    expect(screen.queryByText(/Add a passkey/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Secure my account" }));
    expect(nav).toHaveBeenCalledWith("/secure-account");
  });

  it("snooze hides a prompt for seven days", async () => {
    await load(["add_passkey", "name_recovery_contact"]);
    vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-11-03T12:00:00Z"));
    snoozePrompt("add_passkey");
    expect(currentPrompt()).toBe("name_recovery_contact");
    vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-11-10T12:00:01Z"));
    await load(["add_passkey", "name_recovery_contact"]);
    expect(currentPrompt()).toBe("add_passkey");
  });

  it("broken localStorage never hides a prompt", async () => {
    await load(["add_passkey"]);
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    snoozePrompt("add_passkey");
    expect(currentPrompt()).toBe("add_passkey");
  });

  it("set_up_phone in probation shows the admin hint", async () => {
    signInAs("probation");
    await load(["set_up_phone"]);
    render(() => <SecurityPromptBanner />);
    expect(screen.getByText("Ask your admin to set up your phone, or use 'Secure my account'.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Set up a phone" })).toBeNull();
  });
});
