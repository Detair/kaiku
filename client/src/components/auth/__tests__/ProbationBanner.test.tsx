import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const getMeSecurity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({
  ...(await orig<object>()),
  getMeSecurity,
}));

const passkeySignIn = vi.hoisted(() => vi.fn());
const usable = vi.hoisted(() => ({ on: true }));
vi.mock("@/lib/tauri/passkeys", async (orig) => ({
  ...(await orig<object>()),
  passkeySignIn,
  canUsePasskeys: () => usable.on,
}));

import ProbationBanner from "../ProbationBanner";
import { setAuthState, isProbation, sessionInfo } from "@/stores/auth";
import type { User } from "@/lib/types";

function user(session: unknown): User {
  return {
    id: "u1",
    username: "alice",
    display_name: "Alice",
    avatar_url: null,
    status: "online",
    email: null,
    mfa_enabled: false,
    created_at: "",
    session,
  } as User;
}

const probation = {
  id: "s1",
  assurance: "probation",
  auth_methods: ["qr"],
  auth_time: "",
  client_class: "web",
  absolute_expires_at: "",
  key_device_id: null,
};

function security(passkeys: number) {
  return {
    session: { assurance: "probation", auth_methods: ["qr"] },
    password: null,
    totp: null,
    recovery_codes_left: 0,
    key_devices: 1,
    passkeys,
    may_approve: true,
    bootstrap_open: false,
    recovery_contact: null,
    open_request: null,
    prompts: [],
  };
}

beforeEach(() => {
  getMeSecurity.mockReset();
  usable.on = true;
  passkeySignIn.mockReset();
  passkeySignIn.mockResolvedValue({ user: user({ ...probation, assurance: "full" }), setup_required: false, password_change_required: false });
});

afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("ProbationBanner", () => {
  it("probation session shows the banner with sign in with a passkey", async () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    expect(
      screen.getByText(
        "Signed in for chat only. To change sign-in settings or do admin work, sign in with a passkey.",
      ),
    ).toBeTruthy();
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeTruthy());
  });

  it("a client that cannot run a passkey ceremony offers no passkey sign-in", async () => {
    usable.on = false;
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    await waitFor(() => expect(getMeSecurity).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("button", { name: "Sign in with a passkey" })).toBeNull();
  });

  it("without a passkey it says where to add one", async () => {
    getMeSecurity.mockResolvedValue(security(0));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    await waitFor(() => expect(screen.getByText("Add a passkey from the Kaiku app on your phone.")).toBeTruthy());
    expect(screen.queryByRole("button", { name: "Sign in with a passkey" })).toBeNull();
  });

  it("full session shows no banner", () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user({ ...probation, assurance: "full" }) });
    render(() => <ProbationBanner />);
    expect(screen.queryByText(/Signed in for chat only/)).toBeNull();
    expect(isProbation()).toBe(false);
  });

  it("a /auth/me without session counts as probation", () => {
    setAuthState({ user: user(undefined) });
    expect(sessionInfo()).toBeNull();
    expect(isProbation()).toBe(true);
  });

  it("collapses to a labelled chip and opens again", async () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Hide" }));
    expect(screen.queryByText(/Signed in for chat only/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Chat only" }));
    expect(screen.getByText(/Signed in for chat only/)).toBeTruthy();
  });
});

describe("ProbationBanner across sessions (Argon review)", () => {
  it("a session that becomes chat-only later loads the overview", async () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user({ ...probation, assurance: "full" }) });
    render(() => <ProbationBanner />);
    expect(getMeSecurity).not.toHaveBeenCalled();
    setAuthState({ user: user({ ...probation, id: "s2" }) });
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeTruthy());
    expect(getMeSecurity).toHaveBeenCalled();
  });

  it("a new chat-only session shows the banner open again", async () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Hide" }));
    expect(screen.queryByText(/Signed in for chat only/)).toBeNull();
    setAuthState({ user: user({ ...probation, assurance: "full" }) });
    setAuthState({ user: user({ ...probation, id: "s3" }) });
    await waitFor(() => expect(screen.getByText(/Signed in for chat only/)).toBeTruthy());
  });

  it("the collapsed chip is a large touch target", () => {
    getMeSecurity.mockResolvedValue(security(1));
    setAuthState({ user: user(probation) });
    render(() => <ProbationBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Hide" }));
    const chip = screen.getByRole("button", { name: "Chat only" });
    expect(chip.className).toContain("min-h-11");
    expect(chip.className).toContain("text-base");
  });
});
