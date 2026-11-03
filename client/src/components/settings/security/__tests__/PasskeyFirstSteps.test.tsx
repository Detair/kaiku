// LP's empty passkey list offers L2's first steps (A5).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const listPasskeys = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/passkeys", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/passkeys")),
  listPasskeys,
}));

import PasskeysSection from "../../PasskeysSection";
import PasskeyFirstSteps from "../PasskeyFirstSteps";
import { setAuthState } from "@/stores/auth";
import type { User } from "@/lib/types";

function signIn(assurance: "full" | "probation") {
  setAuthState({
    user: { id: "u1", username: "a", session: { id: "s1", assurance, auth_methods: ["qr"] } } as unknown as User,
  });
}

function renderSection(onSetUpPhone = vi.fn()) {
  render(() => (
    <Router>
      <Route path="/" component={() => <PasskeysSection empty={<PasskeyFirstSteps onSetUpPhone={onSetUpPhone} />} />} />
      <Route path="/secure-account" component={() => <p>secure-account page</p>} />
    </Router>
  ));
  return onSetUpPhone;
}

beforeEach(() => {
  window.history.replaceState(null, "", "/");
  listPasskeys.mockReset().mockResolvedValue({ rp_id: "chat.example.com", user_handle: "dQ", passkeys: [] });
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("PasskeysSection empty state", () => {
  it("passkey empty state offers set up a phone and secure my account", async () => {
    signIn("full");
    const onSetUpPhone = renderSection();
    expect(await screen.findByText("No passkeys yet.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Set up a phone" }));
    expect(onSetUpPhone).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Secure my account" }));
    expect(await screen.findByText("secure-account page")).toBeTruthy();
  });

  it("a chat-only session gets no set up a phone (it needs a full session)", async () => {
    signIn("probation");
    renderSection();
    await screen.findByText("No passkeys yet.");
    expect(screen.queryByRole("button", { name: "Set up a phone" })).toBeNull();
    expect(screen.getByRole("button", { name: "Secure my account" })).toBeTruthy();
  });

  it("a list with passkeys shows no first steps", async () => {
    signIn("full");
    listPasskeys.mockResolvedValue({
      rp_id: "chat.example.com",
      user_handle: "dQ",
      passkeys: [
        {
          id: "p-1",
          label: "Laptop",
          state: "active",
          created_at: "2026-11-01T10:00:00Z",
          last_used_at: null,
          ready_at: null,
          backup_eligible: false,
          backup_state: false,
          transports: [],
          aaguid: null,
        },
      ],
    });
    renderSection();
    await screen.findByText("Laptop");
    expect(screen.queryByTestId("passkey-first-steps")).toBeNull();
  });
});
