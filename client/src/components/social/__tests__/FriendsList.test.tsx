// NAV-3, NAV-4, NAV-13, NAV-17: friends can be written to, requests show at
// every width, the header wraps.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

import { createSignal } from "solid-js";
const [tabWish, setTabWish] = createSignal(false);

const mocks = vi.hoisted(() => ({
  openDirectMessage: vi.fn(),
  removeFriend: vi.fn(),
  state: {
    friends: [
      { user_id: "u1", username: "anna", display_name: "Anna", is_online: true, friendship_id: "f1" },
    ],
    pendingRequests: [
      { user_id: "u2", username: "oma", display_name: "Oma", friendship_id: "f2", direction: "incoming" },
    ],
    blocked: [],
    isLoading: false,
  },
}));

vi.mock("@/stores/dms", () => ({ openDirectMessage: mocks.openDirectMessage }));
vi.mock("@/stores/friends", () => ({
  friendsState: mocks.state,
  loadFriends: vi.fn(),
  loadPendingRequests: vi.fn(),
  loadBlocked: vi.fn(),
  getOnlineFriends: () => mocks.state.friends.filter((f) => f.is_online),
  getIncomingRequestCount: () => mocks.state.pendingRequests.length,
  requestsTabWanted: () => tabWish(),
  takeRequestsTabWish: () => setTabWish(false),
  acceptFriendRequest: vi.fn(),
  rejectFriendRequest: vi.fn(),
  cancelFriendRequest: vi.fn(),
  removeFriend: mocks.removeFriend,
  unblockUser: vi.fn(),
}));
vi.mock("@/stores/presence", () => ({
  getUserActivity: () => undefined,
  getUserStatus: () => "online",
  getUserPresence: () => undefined,
}));

import FriendsList from "../FriendsList";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("FriendsList", () => {
  it("opens the conversation from the row and from Message", () => {
    render(() => <FriendsList />);
    fireEvent.click(screen.getByTestId("friend-open"));
    expect(mocks.openDirectMessage).toHaveBeenCalledWith(["u1"]);
    fireEvent.click(screen.getByTestId("friend-message"));
    expect(mocks.openDirectMessage).toHaveBeenCalledTimes(2);
  });

  it("the row says who is online and their status, not only the name (UX2 K12)", () => {
    render(() => <FriendsList />);
    const row = screen.getByTestId("friend-open");
    const desc = document.getElementById(row.getAttribute("aria-describedby")!);
    expect(desc?.textContent).toContain("Online");
    expect(desc?.textContent).toContain("@anna");
  });

  it("asks before removing a friend", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(() => <FriendsList />);
    fireEvent.click(screen.getByTestId("friend-remove"));
    expect(confirm).toHaveBeenCalled();
    expect(mocks.removeFriend).not.toHaveBeenCalled();
  });

  it("shows waiting requests outside the Requests tab and jumps there", () => {
    render(() => <FriendsList />);
    const banner = screen.getByTestId("friends-request-banner");
    expect(banner.textContent).toContain("1 person wants to be your friend.");
    fireEvent.click(screen.getByText("Show"));
    expect(screen.queryByTestId("friends-request-banner")).toBeNull();
    expect(screen.getByText("Oma")).toBeTruthy();
  });

  it("lets the header wrap on narrow windows", () => {
    render(() => <FriendsList />);
    expect(screen.getByTestId("friends-header").className).toContain("flex-wrap");
  });

  it("opens the Requests tab when a request notice asks for it (K3)", () => {
    setTabWish(true);
    render(() => <FriendsList />);
    expect(screen.getByTestId("friends-tab-pending").getAttribute("aria-pressed")).toBe("true");
    expect(tabWish()).toBe(false);
  });
});
