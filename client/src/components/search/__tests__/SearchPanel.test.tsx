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
import GlobalSearchDialog from "../GlobalSearchDialog";

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});
afterEach(cleanup);

describe("GlobalSearchDialog (A11Y-1)", () => {
  it("is a named dialog: focus in the field, Escape from anywhere, focus back", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const onClose = vi.fn();
    const view = render(() => <GlobalSearchDialog initialScope="all" onClose={onClose} />);
    await new Promise((r) => setTimeout(r, 0));
    const dialog = screen.getByRole("dialog", { name: "Search messages" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(document.activeElement).toBe(screen.getByTestId("search-input"));
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.click(dialog.parentElement!);
    expect(onClose).toHaveBeenCalledTimes(2);
    view.unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });
});

describe("SearchPanel", () => {
  it("closes with Escape while focus is in the panel (A11Y-1)", () => {
    const onClose = vi.fn();
    render(() => <SearchPanel onClose={onClose} />);
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search this server" }), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("takes the focus over the sidebar and hands it back on close", () => {
    // The button that opens the panel holds the focus, so the browser
    // ignores `autofocus` on the field.
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const view = render(() => <SearchPanel onClose={vi.fn()} />);
    expect(document.activeElement).toBe(screen.getByTestId("search-input"));
    view.unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("leaves focus to the dialog in the global search", () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const back = vi.spyOn(opener, "focus");
    const view = render(() => <SearchPanel mode="global" onClose={vi.fn()} />);
    // Rendered bare, without its dialog: the panel neither takes the focus
    // nor hands it back.
    expect(document.activeElement).toBe(opener);
    view.unmount();
    expect(back).not.toHaveBeenCalled();
    opener.remove();
  });

  it("leaves the focus alone on close when it has moved elsewhere", () => {
    const opener = document.createElement("button");
    const elsewhere = document.createElement("input");
    document.body.append(opener, elsewhere);
    opener.focus();
    const view = render(() => <SearchPanel mode="dm" onClose={vi.fn()} />);
    elsewhere.focus();
    view.unmount();
    expect(document.activeElement).toBe(elsewhere);
    opener.remove();
    elsewhere.remove();
  });

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
