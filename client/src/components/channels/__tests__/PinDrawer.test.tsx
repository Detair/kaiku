import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const setSearchParams = vi.fn();
vi.mock("@solidjs/router", () => ({ useSearchParams: () => [{}, setSearchParams] }));
vi.mock("@/stores/channelPins", () => ({
  channelPins: () => [
    {
      pinned_at: "2026-01-01T10:00:00Z",
      message: {
        id: "old-1",
        content: "Treffen am Sonntag",
        author: { display_name: "Oma", avatar_url: null },
      },
    },
  ],
  isPinsLoading: () => false,
  unpinMessageAction: vi.fn(),
}));
vi.mock("@/components/ui/Avatar", () => ({ default: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import PinDrawer from "../PinDrawer";

afterEach(cleanup);

describe("PinDrawer", () => {
  it("jumping to a pin asks the message list to show it, then closes", () => {
    const onJump = vi.fn();
    render(() => (
      <PinDrawer channelId="c1" canUnpin={false} onClose={vi.fn()} onJumpToMessage={onJump} />
    ));
    expect(screen.getByRole("complementary", { name: "Pinned Messages" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Show in chat" }));
    expect(setSearchParams).toHaveBeenCalledWith({ highlight: "old-1" });
    expect(onJump).toHaveBeenCalledWith("old-1");
  });
});
