// NAV-25: the user menu says what it does and reports success and failure.
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  showContextMenu: vi.fn(),
  openDirectMessage: vi.fn(),
  sendFriendRequest: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("@/components/ui/ContextMenu", () => ({
  showContextMenu: mocks.showContextMenu,
  showContextMenuAt: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/stores/dms", () => ({ openDirectMessage: mocks.openDirectMessage }));
vi.mock("@/stores/friends", () => ({ sendFriendRequest: mocks.sendFriendRequest }));
vi.mock("@/components/ui/Toast", () => ({ showToast: mocks.showToast }));

import { showUserContextMenu } from "../contextMenuBuilders";

type Item = { label?: string; action?: () => unknown; separator?: boolean };

function menuFor(id: string): Item[] {
  showUserContextMenu(new MouseEvent("contextmenu"), {
    id,
    username: "oma",
    display_name: "Oma",
  });
  return mocks.showContextMenu.mock.lastCall![1] as Item[];
}

const item = (items: Item[], label: string) => items.find((i) => i.label === label)!;

afterEach(() => vi.clearAllMocks());

describe("user context menu", () => {
  it("has no misleading View Profile entry", () => {
    const labels = menuFor("u1").map((i) => i.label).filter(Boolean);
    expect(labels).not.toContain("View Profile");
    expect(labels[0]).toBe("Send Message");
  });

  it("shows a notice when the conversation cannot be opened", async () => {
    mocks.openDirectMessage.mockRejectedValue(new Error("down"));
    await item(menuFor("u1"), "Send Message").action!();
    expect(mocks.openDirectMessage).toHaveBeenCalledWith(["u1"]);
    expect(mocks.showToast).toHaveBeenCalledWith(expect.objectContaining({ type: "error" }));
  });

  it("confirms a sent friend request", async () => {
    mocks.sendFriendRequest.mockResolvedValue(undefined);
    await item(menuFor("u1"), "Add Friend").action!();
    expect(mocks.showToast).toHaveBeenCalledWith(
      expect.objectContaining({ type: "success", title: "Friend request sent" }),
    );
  });

  it("offers only Copy User ID for yourself", () => {
    const items = menuFor("me");
    expect(items.map((i) => i.label).filter(Boolean)).toEqual(["Copy User ID"]);
    expect(items[0].separator).toBeUndefined();
  });
});
