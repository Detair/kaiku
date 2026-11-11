// NAV-5, NAV-4, LIVE-1, NAV-21, NAV-24: what the server rail tells at a glance.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";

const state = vi.hoisted(() => ({
  dmUnread: 0,
  incoming: 0,
  guildUnread: {} as Record<string, number>,
}));

const loaders = vi.hoisted(() => ({
  loadDMs: vi.fn(),
  loadPendingRequests: vi.fn(),
  bindDMsToAccount: vi.fn(() => true),
  bindFriendsToAccount: vi.fn(() => true),
}));
vi.mock("@/stores/dms", () => ({
  bindDMsToAccount: loaders.bindDMsToAccount,
  loadDMs: loaders.loadDMs,
  getTotalUnreadCount: () => state.dmUnread,
}));
vi.mock("@/stores/friends", () => ({
  bindFriendsToAccount: loaders.bindFriendsToAccount,
  loadPendingRequests: loaders.loadPendingRequests,
  getIncomingRequestCount: () => state.incoming,
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "matthias" }) }));
vi.mock("@/stores/serverFeatures", () => ({ isGuildDiscoveryOff: () => true }));

const guilds = vi.hoisted(() => ({
  guildsState: {
    guilds: [{ id: "g1", name: "Family", icon_url: null }],
    activeGuildId: "g1" as string | null,
    isLoading: false,
    error: null as string | null,
  },
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: guilds.guildsState,
  selectHome: vi.fn(),
  selectGuild: vi.fn(),
  selectDiscovery: vi.fn(),
  isDiscoveryActive: () => false,
  loadGuilds: vi.fn(),
  getGuildUnreadCount: (id: string) => state.guildUnread[id] ?? 0,
}));

import ServerRail from "../ServerRail";

describe("ServerRail", () => {
  beforeEach(() => {
    cleanup();
    state.dmUnread = 0;
    state.incoming = 0;
    state.guildUnread = {};
    guilds.guildsState.error = null;
    vi.clearAllMocks();
  });

  it("loads the counts for a newly signed-in account (W3, K13)", () => {
    render(() => <ServerRail />);
    expect(loaders.bindDMsToAccount).toHaveBeenCalledWith("matthias");
    expect(loaders.bindFriendsToAccount).toHaveBeenCalledWith("matthias");
    expect(loaders.loadDMs).toHaveBeenCalledTimes(1);
    expect(loaders.loadPendingRequests).toHaveBeenCalledTimes(1);
  });

  it("does not reload when the rail is mounted again for the same account", () => {
    // Crossing the phone width mounts the other layout's rail.
    loaders.bindDMsToAccount.mockReturnValue(false);
    loaders.bindFriendsToAccount.mockReturnValue(false);
    try {
      render(() => <ServerRail />);
      expect(loaders.loadDMs).not.toHaveBeenCalled();
      expect(loaders.loadPendingRequests).not.toHaveBeenCalled();
    } finally {
      loaders.bindDMsToAccount.mockReturnValue(true);
      loaders.bindFriendsToAccount.mockReturnValue(true);
    }
  });

  it("shows unread direct messages and friend requests on Home", () => {
    state.dmUnread = 2;
    state.incoming = 1;
    render(() => <ServerRail />);
    expect(screen.getByTestId("home-unread-badge").textContent).toBe("3");
    expect(screen.getByTestId("home-button").getAttribute("aria-label")).toBe(
      "Home, 2 unread messages, 1 friend request waiting",
    );
  });

  it("shows no badge and no dot on Home when nothing is new", () => {
    render(() => <ServerRail />);
    expect(screen.queryByTestId("home-unread-badge")).toBeNull();
    const pill = screen.getByTestId("home-button").previousElementSibling as HTMLElement;
    expect(pill.style.height).toBe("0px");
  });

  it("names server buttons after the server, with the unread count", () => {
    state.guildUnread = { g1: 4 };
    render(() => <ServerRail />);
    expect(screen.getByTestId("guild-button").getAttribute("aria-label")).toBe(
      "Family, 4 unread",
    );
  });

  it("says in words that the servers did not load", () => {
    guilds.guildsState.error = "boom";
    render(() => <ServerRail />);
    expect(screen.getByTestId("guilds-retry").textContent).toContain("Retry");
  });
});
