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
});
