// NAV-12: picking a conversation in the phone drawer closes it.
// NAV-16: the browser tab shows how much is unread.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";

const dms = vi.hoisted(() => ({ unread: 0 }));
const [dmsState, setDmsState] = createStore({
  selectedDMId: null as string | null,
  isShowingFriends: true,
  navigationCount: 0,
});
const [guildsState, setGuildsState] = createStore({
  guilds: [{ id: "g1", name: "Familie" }] as { id: string; name: string }[],
  guildUnreadCounts: {} as Record<string, number>,
  activeGuildId: null as string | null,
});
const [chan, setChan] = createStore({ selected: null as { id: string; name: string } | null });

vi.mock("@/lib/useBreakpoint", () => ({ useIsMobile: () => () => true }));
vi.mock("@/stores/dms", () => ({
  get dmsState() {
    return dmsState;
  },
  getTotalUnreadCount: () => dms.unread,
  getSelectedDM: () =>
    dmsState.selectedDMId
      ? { id: "dm-1", name: "", participants: [{ user_id: "me", display_name: "Me" }, { user_id: "u2", display_name: "Oma" }] }
      : null,
}));
vi.mock("@/stores/channels", () => ({ selectedChannel: () => chan.selected }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/stores/guilds", () => ({
  get guildsState() {
    return guildsState;
  },
  getActiveGuild: () => guildsState.guilds.find((g) => g.id === guildsState.activeGuildId) ?? null,
}));
vi.mock("../ServerRail", () => ({ default: () => <div /> }));
vi.mock("../Sidebar", () => ({ default: () => <div /> }));
vi.mock("../ConnectionIndicator", () => ({ default: () => <div /> }));
vi.mock("../MobileHeader", () => ({
  default: (p: { onMenuClick: () => void }) => (
    <button data-testid="open-menu" onClick={() => p.onMenuClick()} />
  ),
}));
vi.mock("@/components/voice/ScreenShareViewer", () => ({ default: () => null }));

import AppShell from "../AppShell";

afterEach(() => {
  cleanup();
  setDmsState({ selectedDMId: null, isShowingFriends: true, navigationCount: 0 });
  setGuildsState({ guilds: [{ id: "g1", name: "Familie" }], guildUnreadCounts: {}, activeGuildId: null });
  setChan({ selected: null });
});

describe("AppShell", () => {
  it("closes the drawer when a conversation is opened from it", () => {
    render(() => <AppShell sidebar={<div />} />);
    fireEvent.click(screen.getByTestId("open-menu"));
    expect(screen.getByTestId("mobile-drawer-close").closest("[data-drawer-open]")?.getAttribute("data-drawer-open")).toBe("true");
    setDmsState({ selectedDMId: "dm-1", isShowingFriends: false, navigationCount: 1 });
    expect(screen.getByTestId("mobile-drawer-close").closest("[data-drawer-open]")?.getAttribute("data-drawer-open")).toBe("false");
  });

  it("closes the drawer when the conversation already open is tapped again (K2)", () => {
    setDmsState({ selectedDMId: "dm-1", isShowingFriends: false });
    render(() => <AppShell sidebar={<div />} />);
    fireEvent.click(screen.getByTestId("open-menu"));
    // Same conversation: only the navigation counter moves.
    setDmsState({ navigationCount: dmsState.navigationCount + 1 });
    expect(screen.getByTestId("mobile-drawer-close").closest("[data-drawer-open]")?.getAttribute("data-drawer-open")).toBe("false");
  });

  it("ignores unread counts of servers from a previous account (W3)", () => {
    dms.unread = 0;
    setGuildsState({ guildUnreadCounts: { g1: 1, "old-account-server": 5 } });
    render(() => <AppShell />);
    expect(document.title).toBe("(1) Friends – Kaiku");
  });

  it("puts the unread count in the browser tab title", () => {
    dms.unread = 2;
    setGuildsState({ guildUnreadCounts: { g1: 1 } });
    render(() => <AppShell />);
    expect(document.title).toBe("(3) Friends – Kaiku");
    dms.unread = 0;
    setGuildsState({ guildUnreadCounts: { g1: 0 } });
    expect(document.title).toBe("Friends – Kaiku");
  });

  it("names the open channel and server, or the person you are writing with (A11Y-18)", () => {
    dms.unread = 0;
    render(() => <AppShell />);
    setGuildsState({ activeGuildId: "g1" });
    setChan({ selected: { id: "c1", name: "general" } });
    expect(document.title).toBe("#general – Familie – Kaiku");
    setGuildsState({ activeGuildId: null });
    setDmsState({ selectedDMId: "dm-1", isShowingFriends: false });
    expect(document.title).toBe("Oma – Kaiku");
  });

  it("main is the target of the skip link and can take focus", () => {
    render(() => <AppShell />);
    const main = screen.getByRole("main");
    expect(main.id).toBe("main-content");
    main.focus();
    expect(document.activeElement).toBe(main);
  });
});
