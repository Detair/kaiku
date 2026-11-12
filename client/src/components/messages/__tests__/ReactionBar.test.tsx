import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));

import ReactionBar from "../ReactionBar";
import type { Reaction } from "@/lib/types";

afterEach(cleanup);

describe("ReactionBar", () => {
  it("tells a screen reader who reacted and whether it is mine", () => {
    const reactions = [
      { emoji: "👍", count: 2, me: true, users: ["Oma", "Tom"] },
      { emoji: "😂", count: 3, me: false },
    ] as Reaction[];
    render(() => (
      <ReactionBar reactions={reactions} onAddReaction={vi.fn()} onRemoveReaction={vi.fn()} />
    ));
    const mine = screen.getByRole("button", { name: "👍, Oma, Tom" });
    expect(mine.getAttribute("aria-pressed")).toBe("true");
    const other = screen.getByRole("button", { name: "😂, 3 people" });
    expect(other.getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByRole("button", { name: "Add reaction" })).toBeTruthy();
  });
});
