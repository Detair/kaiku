import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

vi.mock("@/stores/voice", () => ({
  isInChannel: () => true,
  voiceState: { state: "connected", channelId: "v1" },
  getParticipants: () => [
    { user_id: "me" },
    { user_id: "aaaaaaaa-1111", username: "ann", display_name: "Ann Example" },
    { user_id: "bbbbbbbb-2222" },
  ],
}));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));
vi.mock("@/stores/members", () => ({
  getMember: (_g: string, id: string) =>
    id === "bbbbbbbb-2222"
      ? { user_id: id, username: "ben", display_name: "Ben Member", nickname: null }
      : undefined,
}));

import ChannelItem from "../ChannelItem";

afterEach(cleanup);

describe("ChannelItem", () => {
  it("voice tooltip shows display names", () => {
    const { container } = render(() => (
      <ChannelItem
        channel={{ id: "v1", name: "Lounge", channel_type: "voice" } as never}
        isSelected={false}
        onClick={() => {}}
        guildId="g1"
      />
    ));
    fireEvent.mouseEnter(container.querySelector("[title]") as Element);
    expect(screen.getByText("Ann Example")).toBeTruthy();
    expect(screen.getByText("Ben Member")).toBeTruthy();
    expect(container.textContent).not.toMatch(/aaaaaaaa|bbbbbbbb/);
  });

  it("says a click opens the voice channel and marks it as open (NAV-14)", () => {
    render(() => (
      <ChannelItem
        channel={{ id: "v2", name: "Kitchen", channel_type: "voice" } as never}
        isSelected={true}
        onClick={() => {}}
      />
    ));
    const button = screen.getByTestId("channel-item");
    expect(button.getAttribute("aria-current")).toBe("page");
    // isInChannel is mocked to true: the hint explains, it does not promise
    // a disconnect.
    expect(button.getAttribute("title")).toBe(
      "You are in this voice channel. Click to open it.",
    );
  });
});
