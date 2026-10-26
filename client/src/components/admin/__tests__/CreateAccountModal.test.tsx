import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({ adminCreateUser: vi.fn() }));

vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return {
    HttpError: common.HttpError,
    adminCreateUser: mocks.adminCreateUser,
    getServerUrl: () => "https://kaiku.example",
  };
});
vi.mock("@/stores/admin", () => ({
  adminState: { isElevated: true },
  createUser: (req: unknown) => mocks.adminCreateUser(req),
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: {
    guilds: [
      { id: "g1", name: "Family" },
      { id: "g2", name: "Friends" },
    ],
  },
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import CreateAccountModal from "../CreateAccountModal";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("CreateAccountModal", () => {
  it("create account shows the username-taken sentence", async () => {
    mocks.adminCreateUser.mockRejectedValue(new HttpError(409, "x", "USER_EXISTS"));
    const { container } = render(() => <CreateAccountModal onClose={vi.fn()} />);
    const input = container.querySelector("input") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "anna" } });
    fireEvent.submit(container.querySelector("form")!);
    expect(await screen.findByText("That username is already taken.")).toBeTruthy();
  });
});

describe("CreateAccountModal servers", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mocks.adminCreateUser.mockReset();
    mocks.adminCreateUser.mockResolvedValue({ username: "bob", display_name: "Bob" });
  });

  it("create account sends the checked servers", async () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    // Every server is ticked by default.
    expect((screen.getByLabelText("Family") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByLabelText("Friends"));
    fireEvent.input(screen.getByLabelText(/^Username/), { target: { value: "bob" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(mocks.adminCreateUser).toHaveBeenCalledTimes(1));
    expect(mocks.adminCreateUser.mock.calls[0][0].guild_ids).toEqual(["g1"]);
  });

  it("sends every server by default", async () => {
    render(() => <CreateAccountModal onClose={() => {}} />);
    fireEvent.input(screen.getByLabelText(/^Username/), { target: { value: "bob" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(mocks.adminCreateUser).toHaveBeenCalled());
    expect(mocks.adminCreateUser.mock.calls[0][0].guild_ids).toEqual(["g1", "g2"]);
  });
});
