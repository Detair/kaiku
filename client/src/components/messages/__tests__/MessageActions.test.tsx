import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import { createGenerator } from "unocss";
import config from "../../../../uno.config";

vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));

import MessageActions, { ACTION_BAR_HIDDEN } from "../MessageActions";

afterEach(cleanup);

const renderBar = () =>
  render(() => (
    <div class="group">
      <MessageActions onAddReaction={vi.fn()} onShowContextMenu={vi.fn()} onReplyInThread={vi.fn()} />
    </div>
  ));

describe("MessageActions", () => {
  it("is a labelled toolbar", () => {
    renderBar();
    expect(screen.getByRole("toolbar", { name: "Message actions" })).toBeTruthy();
  });

  it("the hidden bar shows up for keyboard focus, not only for the mouse", async () => {
    renderBar();
    const bar = screen.getByTestId("message-action-bar");
    for (const cls of ACTION_BAR_HIDDEN.split(" ")) expect(bar.classList).toContain(cls);

    const uno = await createGenerator({ ...config, safelist: [] });
    const { css } = await uno.generate(ACTION_BAR_HIDDEN, { preflights: false });
    // Focus anywhere in the bar, or anywhere in the message row (the group).
    expect(css).toMatch(/:focus-within[^{]*\{opacity:1;\}/);
    expect(css).toMatch(/\.group:focus-within [^{]*\{opacity:1;\}/);
    expect(css).toMatch(/:focus-within[^{]*\{pointer-events:auto;\}/);
    // While hidden, it cannot be tapped by accident.
    expect(css).toMatch(/\.pointer-events-none\{pointer-events:none;\}/);
  });

  it("never grows wider than the screen: buttons keep their size and wrap", async () => {
    renderBar();
    const bar = screen.getByTestId("message-action-bar");
    const uno = await createGenerator({ ...config, safelist: [] });
    const { css } = await uno.generate(bar.className, { preflights: false });
    expect(css).toMatch(/max-width:calc\(100vw - 1\.5rem\)/);
    expect(css).toMatch(/flex-wrap:wrap/);
    const buttons = bar.querySelectorAll("button");
    expect(buttons.length).toBeGreaterThan(4);
    for (const b of buttons) expect(b.classList).toContain("shrink-0");
  });

  it("stays visible while its emoji picker is open", () => {
    renderBar();
    fireEvent.click(screen.getByRole("button", { name: "More reactions" }));
    const bar = screen.getByTestId("message-action-bar");
    expect(bar.classList).toContain("opacity-100");
    expect(bar.classList).not.toContain("opacity-0");
  });

  it("offers no thread button when threads are off", () => {
    render(() => (
      <MessageActions
        onAddReaction={vi.fn()}
        onShowContextMenu={vi.fn()}
        onReplyInThread={vi.fn()}
        threadsEnabled={false}
      />
    ));
    expect(screen.queryByRole("button", { name: "Reply in Thread" })).toBeNull();
  });
});
