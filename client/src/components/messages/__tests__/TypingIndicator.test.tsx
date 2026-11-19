import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

const typing = vi.hoisted(() => ({ byChannel: {} as Record<string, string[]> }));
vi.mock("@/stores/websocket", () => ({ typingState: typing }));
vi.mock("@/stores/messages", () => ({
  messagesState: {
    byChannel: {
      c1: [{ id: "m1", author: { id: "oma", display_name: "Oma", username: "oma_erna" } }],
    },
  },
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: {
    members: { g1: [{ user_id: "tom", display_name: "Tom", username: "tom", nickname: null }] },
  },
}));
vi.mock("@/stores/dms", () => ({ dmsState: { dms: [] } }));

import TypingIndicator from "../TypingIndicator";

afterEach(cleanup);

describe("TypingIndicator", () => {
  it("typing indicator text uses plural forms", () => {
    typing.byChannel = { c1: ["u1"] };
    render(() => <TypingIndicator channelId="c1" />);
    expect(screen.getByText("Someone is typing...")).toBeTruthy();
    cleanup();

    typing.byChannel = { c1: ["u1", "u2", "u3"] };
    render(() => <TypingIndicator channelId="c1" />);
    expect(screen.getByText("3 people are typing...")).toBeTruthy();
  });

  it("two typers read as 2 people", () => {
    typing.byChannel = { c1: ["u1", "u2"] };
    render(() => <TypingIndicator channelId="c1" />);
    expect(screen.getByText("2 people are typing...")).toBeTruthy();
  });

  it("names who is typing when the name is known", () => {
    typing.byChannel = { c1: ["oma"] };
    render(() => <TypingIndicator channelId="c1" />);
    expect(screen.getByText("Oma is typing...")).toBeTruthy();
    cleanup();

    typing.byChannel = { c1: ["oma", "tom"] };
    render(() => <TypingIndicator channelId="c1" />);
    expect(screen.getByText("Oma and Tom are typing...")).toBeTruthy();
  });

  it("sits in a polite live region that exists before anyone types", () => {
    typing.byChannel = {};
    render(() => <TypingIndicator channelId="c1" />);
    const region = screen.getByRole("status");
    expect(region.getAttribute("aria-live")).toBe("polite");
    expect(region.textContent).toBe("");
  });
});
