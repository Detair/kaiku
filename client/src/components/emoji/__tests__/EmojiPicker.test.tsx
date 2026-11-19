import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

vi.mock("@/stores/emoji", () => ({
  emojiState: { recents: [], guildEmojis: {} },
  addEmojiStringToRecents: vi.fn(),
}));

import EmojiPicker from "../EmojiPicker";
import { EMOJI_CATEGORIES } from "@/lib/emojiData";

afterEach(cleanup);

describe("EmojiPicker", () => {
  it("focuses the search box and shows every emoji of each category", () => {
    render(() => <EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);
    const search = screen.getByRole("textbox", { name: "Search emoji" });
    expect(document.activeElement).toBe(search);
    expect(screen.getByText("Smileys & Emotion")).toBeTruthy();
    const total = EMOJI_CATEGORIES.reduce((n, c) => n + c.emojis.length, 0);
    expect(screen.getByTestId("emoji-picker").querySelectorAll("button").length).toBe(total);
  });

  it("says when a search finds nothing", () => {
    render(() => <EmojiPicker onSelect={vi.fn()} onClose={vi.fn()} />);
    const search = screen.getByTestId("emoji-search") as HTMLInputElement;
    fireEvent.input(search, { target: { value: "zzzqqq" } });
    expect(screen.getByText("No emoji found. Try another word.")).toBeTruthy();
  });
});
