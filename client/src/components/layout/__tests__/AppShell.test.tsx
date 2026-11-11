// NAV-12: picking a conversation in the phone drawer closes it.
// NAV-16: the browser tab shows how much is unread.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";

const dms = vi.hoisted(() => ({ unread: 0 }));
const [dmsState, setDmsState] = createStore({
  selectedDMId: null as string | null,
  isShowingFriends: true,
});
const [guildsState, setGuildsState] = createStore({
  guildUnreadCounts: {} as Record<string, number>,
});

vi.mock("@/lib/useBreakpoint", () => ({ useIsMobile: () => () => true }));
vi.mock("@/stores/dms", () => ({
  get dmsState() {
    return dmsState;
  },
  getTotalUnreadCount: () => dms.unread,
}));
vi.mock("@/stores/guilds", () => ({
  get guildsState() {
    return guildsState;
  },
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
  setDmsState({ selectedDMId: null, isShowingFriends: true });
  setGuildsState({ guildUnreadCounts: {} });
});

describe("AppShell", () => {
  it("closes the drawer when a conversation is opened from it", () => {
    render(() => <AppShell sidebar={<div />} />);
    fireEvent.click(screen.getByTestId("open-menu"));
    expect(screen.getByTestId("mobile-drawer-close")).toBeTruthy();
    setDmsState({ selectedDMId: "dm-1", isShowingFriends: false });
    expect(screen.queryByTestId("mobile-drawer-close")).toBeNull();
  });

  it("puts the unread count in the browser tab title", () => {
    dms.unread = 2;
    setGuildsState({ guildUnreadCounts: { g1: 1 } });
    render(() => <AppShell />);
    expect(document.title).toBe("(3) Kaiku");
    dms.unread = 0;
    setGuildsState({ guildUnreadCounts: { g1: 0 } });
    expect(document.title).toBe("Kaiku");
  });
});
