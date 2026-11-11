// Components render through @solidjs/testing-library with the real i18n
// (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";

const [friends, setFriends] = createStore({
  friends: [
    { user_id: "u2", username: "ben", display_name: "Ben" },
    { user_id: "u1", username: "anna", display_name: "Anna" },
  ],
  blocked: [{ user_id: "u4", username: "troll", display_name: "Troll" }] as {
    user_id: string;
    username: string;
    display_name: string;
  }[],
});

const mocks = vi.hoisted(() => ({
  openDirectMessage: vi.fn(),
  loadAllGuildMembers: vi.fn(async () => 0),
  loadBlocked: vi.fn(async () => true),
}));

vi.mock("@/stores/dms", () => ({ openDirectMessage: mocks.openDirectMessage }));
vi.mock("@/stores/friends", () => ({
  get friendsState() {
    return friends;
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
  mocks.loadBlocked.mockImplementation(async () => true);
  setFriends("blocked", [{ user_id: "u4", username: "troll", display_name: "Troll" }]);
});

/** Wait until the block list is in and people are offered. */
const ready = () => screen.findAllByTestId("new-dm-person");

const names = () =>
  screen.getAllByTestId("new-dm-person").map((el) => el.querySelector(".font-medium")?.textContent);

describe("NewMessageModal", () => {
  it("offers server members who are not friends, friends first, never me", async () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await ready();
    expect(mocks.loadAllGuildMembers).toHaveBeenCalled();
    expect(names()).toEqual(["Anna", "Ben", "Oma Hilde", "Uroma"]);
  });

  it("never offers someone I blocked (W4) and loads the block list", async () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await ready();
    expect(mocks.loadBlocked).toHaveBeenCalled();
    expect(names()).not.toContain("Troll");
  });

  it("finds a server member by name, also by the real name behind a nickname (K6)", async () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await ready();
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
    await ready();
    fireEvent.click(screen.getByText("Oma Hilde"));
    fireEvent.click(screen.getByTestId("new-dm-submit"));
    await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(mocks.openDirectMessage).toHaveBeenCalledWith(["u3"]);
  });

  it("is a dialog that closes with Escape", async () => {
    const onClose = vi.fn();
    render(() => <NewMessageModal onClose={onClose} />);
    await ready();
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("selected count uses plural forms", async () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await ready();
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
    await ready();
    fireEvent.click(screen.getByText("Anna"));
    fireEvent.click(screen.getByTestId("new-dm-submit"));
    expect(await screen.findByText("Failed to create conversation")).toBeTruthy();
    expect(screen.queryByText(/Bad Request/)).toBeNull();
  });

  it("offers no one and blocks sending until the block list is loaded (N3)", async () => {
    let finish: (ok: boolean) => void = () => {};
    mocks.loadBlocked.mockImplementation(() => new Promise<boolean>((r) => (finish = r)));
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await Promise.resolve();
    expect(screen.queryAllByTestId("new-dm-person")).toHaveLength(0);
    expect((screen.getByTestId("new-dm-submit") as HTMLButtonElement).disabled).toBe(true);
    finish(true);
    expect((await ready()).length).toBeGreaterThan(0);
  });

  it("says when the block list failed and retries on request (N3)", async () => {
    mocks.loadBlocked.mockImplementationOnce(async () => false);
    render(() => <NewMessageModal onClose={vi.fn()} />);
    const alert = await screen.findByTestId("new-dm-blocks-failed");
    expect(screen.queryAllByTestId("new-dm-person")).toHaveLength(0);
    fireEvent.click(alert.querySelector("button")!);
    expect((await ready()).length).toBeGreaterThan(0);
    expect(screen.queryByTestId("new-dm-blocks-failed")).toBeNull();
  });

  it("drops a selected person who turns out to be blocked (N3)", async () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    await ready();
    fireEvent.click(screen.getByText("Oma Hilde"));
    expect(screen.getByText("1 user selected")).toBeTruthy();
    setFriends("blocked", (b) => [...b, { user_id: "u3", username: "oma", display_name: "Oma Hilde" }]);
    expect(screen.queryByText("1 user selected")).toBeNull();
    expect((screen.getByTestId("new-dm-submit") as HTMLButtonElement).disabled).toBe(true);
  });
});
