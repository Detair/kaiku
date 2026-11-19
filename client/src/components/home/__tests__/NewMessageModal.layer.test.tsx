import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
vi.mock("@/stores/dms", () => ({ openDirectMessage: vi.fn() }));
vi.mock("@/stores/friends", () => ({
  friendsState: { friends: [], blocked: [] },
  loadFriends: vi.fn(),
  loadBlocked: vi.fn(async () => true),
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: { guilds: [], members: {} },
  loadAllGuildMembers: vi.fn(async () => 0),
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));

import NewMessageModal from "../NewMessageModal";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("NewMessageModal layer", () => {
  it("opens in front of a window that is already open (it takes the keyboard)", async () => {
    // For example the first-run tour, which sits on the dialog layer.
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.dialog);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => <NewMessageModal onClose={() => {}} />);
    await Promise.resolve();
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    expect(zOf(screen.getByRole("dialog"))).toBeGreaterThan(earlier);
  });
});
