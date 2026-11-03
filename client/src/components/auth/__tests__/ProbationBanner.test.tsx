import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const getMeSecurity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({
  ...(await orig<object>()),
  getMeSecurity,
}));

import ProbationBanner from "../ProbationBanner";
import { setAuthState, isProbation, sessionInfo } from "@/stores/auth";
import { registerPasskeyCeremonies } from "@/lib/passkeySeam";
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
  registerPasskeyCeremonies({
    signIn: vi.fn().mockResolvedValue({ user: user({ ...probation, assurance: "full" }), setup_required: false, password_change_required: false }),
    stepUp: vi.fn(),
  });
});

afterEach(() => {
  cleanup();
  registerPasskeyCeremonies(null);
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
