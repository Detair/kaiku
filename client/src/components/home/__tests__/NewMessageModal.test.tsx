// Components render through @solidjs/testing-library with the real i18n
// (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  openDirectMessage: vi.fn(),
  loadAllGuildMembers: vi.fn(async () => 0),
  loadBlocked: vi.fn(async () => {}),
}));

vi.mock("@/stores/dms", () => ({ openDirectMessage: mocks.openDirectMessage }));
vi.mock("@/stores/friends", () => ({
  friendsState: {
    friends: [
      { user_id: "u2", username: "ben", display_name: "Ben" },
      { user_id: "u1", username: "anna", display_name: "Anna" },
    ],
    blocked: [{ user_id: "u4", username: "troll", display_name: "Troll" }],
  },
  loadFriends: vi.fn(),
  loadBlocked: mocks.loadBlocked,
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: {
    guilds: [{ id: "g1" }],
    members: {
      g1: [
        { user_id: "me", username: "me", display_name: "Me", nickname: null },
        { user_id: "u1", username: "anna", display_name: "Anna", nickname: null },
        { user_id: "u3", username: "oma", display_name: "Oma Hilde", nickname: null },
        { user_id: "u4", username: "troll", display_name: "Troll", nickname: null },
        { user_id: "u5", username: "hk", display_name: "Hildegard Krause", nickname: "Uroma" },
      ],
    },
  },
  loadAllGuildMembers: mocks.loadAllGuildMembers,
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));

import NewMessageModal from "../NewMessageModal";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
  vi.clearAllMocks();
  mocks.loadAllGuildMembers.mockImplementation(async () => 0);
});

const names = () =>
  screen.getAllByTestId("new-dm-person").map((el) => el.querySelector(".font-medium")?.textContent);

describe("NewMessageModal", () => {
  it("offers server members who are not friends, friends first, never me", () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    expect(mocks.loadAllGuildMembers).toHaveBeenCalled();
    expect(names()).toEqual(["Anna", "Ben", "Oma Hilde", "Uroma"]);
  });

  it("never offers someone I blocked (W4) and loads the block list", () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    expect(mocks.loadBlocked).toHaveBeenCalled();
    expect(names()).not.toContain("Troll");
  });

  it("finds a server member by name, also by the real name behind a nickname (K6)", () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.input(screen.getByTestId("new-dm-input"), { target: { value: "hil" } });
    expect(names()).toEqual(["Oma Hilde", "Uroma"]);
  });

  it("says it is loading and when some people could not be loaded (K4)", async () => {
    let finish: (n: number) => void = () => {};
    mocks.loadAllGuildMembers.mockImplementation(
      () => new Promise<number>((resolve) => (finish = resolve)),
    );
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.input(screen.getByTestId("new-dm-input"), { target: { value: "nobody" } });
    expect(screen.getByTestId("new-dm-empty").textContent).toBe("Loading people...");
    finish(1);
    expect(await screen.findByTestId("new-dm-partial")).toBeTruthy();
    expect(screen.getByTestId("new-dm-empty").textContent).toBe("No one found with that name.");
  });

  it("opens the conversation with a server member", async () => {
    mocks.openDirectMessage.mockResolvedValue("dm-1");
    const onClose = vi.fn();
    render(() => <NewMessageModal onClose={onClose} />);
    fireEvent.click(screen.getByText("Oma Hilde"));
    fireEvent.click(screen.getByTestId("new-dm-submit"));
    await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(mocks.openDirectMessage).toHaveBeenCalledWith(["u3"]);
  });

  it("is a dialog that closes with Escape", () => {
    const onClose = vi.fn();
    render(() => <NewMessageModal onClose={onClose} />);
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("selected count uses plural forms", () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.click(screen.getByText("Anna"));
    expect(screen.getByText("1 user selected")).toBeTruthy();
    expect(screen.getAllByRole("checkbox")[0].getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByText("Ben"));
    expect(screen.getByText("2 users selected")).toBeTruthy();
    expect(screen.getByText("(group chat)")).toBeTruthy();
    expect(screen.getByTestId("new-dm-submit").textContent).toBe("Start group chat");
  });

  it("shows the fallback sentence instead of a raw server error", async () => {
    mocks.openDirectMessage.mockRejectedValue(new HttpError(400, "HTTP 400: Bad Request"));
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.click(screen.getByText("Anna"));
    fireEvent.click(screen.getByTestId("new-dm-submit"));
    expect(await screen.findByText("Failed to create conversation")).toBeTruthy();
    expect(screen.queryByText(/Bad Request/)).toBeNull();
  });
});
