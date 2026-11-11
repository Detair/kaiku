import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const search = vi.fn();
vi.mock("@solidjs/router", () => ({ useNavigate: () => vi.fn() }));
vi.mock("@/stores/search", () => ({
  searchState: { results: [], total: 0, query: "", isSearching: false, error: null },
  search: (...a: unknown[]) => search(...a),
  searchDMs: vi.fn(),
  searchGlobal: vi.fn(),
  loadMore: vi.fn(),
  clearSearch: vi.fn(),
  hasMore: () => false,
}));
vi.mock("@/stores/guilds", () => ({
  getActiveGuild: () => ({ id: "g1" }),
  guildsState: {
    members: {
      g1: [
        { user_id: "u-tom", display_name: "Tom", username: "tom", nickname: null },
        { user_id: "u-oma", display_name: "Erna", username: "oma_erna", nickname: "Oma" },
      ],
    },
  },
}));
vi.mock("@/stores/dms", () => ({ dmsState: { dms: [] } }));
vi.mock("@/components/ui/Avatar", () => ({ default: () => null }));

import SearchPanel from "../SearchPanel";

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});
afterEach(cleanup);

describe("SearchPanel", () => {
  it("filters by a person's name instead of an internal id", async () => {
    vi.useFakeTimers();
    try {
      render(() => <SearchPanel onClose={vi.fn()} />);
      fireEvent.input(screen.getByRole("textbox", { name: "Search this server" }), {
        target: { value: "kuchen" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Filters" }));
      const author = screen.getByLabelText("Written by") as HTMLSelectElement;
      expect([...author.options].map((o) => o.textContent)).toEqual(["Anyone", "Oma", "Tom"]);
      fireEvent.change(author, { target: { value: "u-oma" } });
      expect(search).toHaveBeenLastCalledWith(
        "g1",
        "kuchen",
        expect.objectContaining({ author_id: "u-oma" }),
      );
      expect(screen.queryByText("Author ID")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("speaks the user's language for hints and scopes", () => {
    render(() => <SearchPanel onClose={vi.fn()} channelId="c1" />);
    expect(screen.getByText("Type at least 2 letters to search.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "This channel" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Close search" })).toBeTruthy();
  });
});
