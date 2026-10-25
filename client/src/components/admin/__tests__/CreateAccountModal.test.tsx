import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const createUser = vi.fn();
vi.mock("@/stores/admin", () => ({
  adminState: { isElevated: true },
  createUser: (...a: unknown[]) => createUser(...a),
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: {
    guilds: [
      { id: "g1", name: "Family" },
      { id: "g2", name: "Friends" },
    ],
  },
}));
vi.mock("@/lib/tauri", () => ({
  getServerUrl: () => "https://chat.example",
  HttpError: class HttpError extends Error {},
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import CreateAccountModal from "../CreateAccountModal";

describe("CreateAccountModal servers", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    createUser.mockReset();
    createUser.mockResolvedValue({ username: "bob", display_name: "Bob" });
  });

  it("create account sends the checked servers", async () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    // Every server is ticked by default.
    expect((screen.getByLabelText("Family") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByLabelText("Friends"));
    fireEvent.input(screen.getByLabelText(/^Username/), { target: { value: "bob" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(createUser).toHaveBeenCalledTimes(1));
    expect(createUser.mock.calls[0][0].guild_ids).toEqual(["g1"]);
  });

  it("sends every server by default", async () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    fireEvent.input(screen.getByLabelText(/^Username/), { target: { value: "bob" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(createUser).toHaveBeenCalled());
    expect(createUser.mock.calls[0][0].guild_ids).toEqual(["g1", "g2"]);
  });
});
