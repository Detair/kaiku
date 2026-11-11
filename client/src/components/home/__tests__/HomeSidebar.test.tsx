// NAV-6, NAV-8, NAV-4: the Home sidebar keeps voice controls, always offers
// "New Message" and shows waiting friend requests.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";

const state = vi.hoisted(() => ({ incoming: 0 }));

vi.mock("@/stores/dms", () => ({
  dmsState: { dms: [], isLoading: false, isShowingFriends: true },
  loadDMs: vi.fn(),
  selectFriendsTab: vi.fn(),
}));
vi.mock("@/stores/friends", () => ({ getIncomingRequestCount: () => state.incoming }));
vi.mock("@/components/voice/VoicePanel", () => ({
  default: () => <div data-testid="voice-panel-slot" />,
}));
vi.mock("@/components/layout/UserPanel", () => ({ default: () => <div /> }));
vi.mock("@/components/search/SearchPanel", () => ({ default: () => <div /> }));
vi.mock("../NewMessageModal", () => ({ default: () => <div data-testid="new-message-modal" /> }));
vi.mock("@/components/social/AddFriend", () => ({ default: () => <div /> }));

import HomeSidebar from "../HomeSidebar";

describe("HomeSidebar", () => {
  beforeEach(() => {
    cleanup();
    state.incoming = 0;
  });

  it("keeps the voice panel on Home so a running call stays controllable", () => {
    render(() => <HomeSidebar />);
    expect(screen.getByTestId("voice-panel-slot")).toBeTruthy();
  });

  it("always shows a labelled New Message button", () => {
    render(() => <HomeSidebar />);
    const button = screen.getByTestId("new-dm-button");
    expect(button.textContent).toContain("New Message");
    expect(button.className).not.toContain("opacity-0");
    button.click();
    expect(screen.getByTestId("new-message-modal")).toBeTruthy();
  });

  it("counts friend requests waiting for an answer", () => {
    state.incoming = 2;
    render(() => <HomeSidebar />);
    const count = screen.getByTestId("friends-request-count");
    expect(count.textContent).toBe("2");
    expect(count.getAttribute("aria-label")).toBe("2 friend requests waiting");
  });
});
