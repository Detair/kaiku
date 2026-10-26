import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

const typing = vi.hoisted(() => ({ byChannel: {} as Record<string, string[]> }));
vi.mock("@/stores/websocket", () => ({ typingState: typing }));

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
});
