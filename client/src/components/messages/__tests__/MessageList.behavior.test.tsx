import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, waitFor } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";

type Msg = {
  id: string;
  created_at: string;
  author: { id: string; display_name: string; username: string };
  content: string;
  message_type?: string;
};

const store = vi.hoisted(() => ({
  state: undefined as unknown as {
    byChannel: Record<string, Msg[]>;
    loadingChannels: Record<string, boolean>;
    hasMore: Record<string, boolean>;
    loadError: string | null;
  },
  set: undefined as unknown as (...args: unknown[]) => void,
}));
const loadMessagesForJump = vi.hoisted(() => vi.fn());
const params = vi.hoisted(() => ({
  value: undefined as unknown as Record<string, string | undefined>,
  set: undefined as unknown as (next: Record<string, string | undefined>) => void,
}));
const dms = vi.hoisted(() => ({ dms: [] as { id: string }[] }));
const guildThreads = vi.hoisted(() => ({ enabled: true }));
const showToast = vi.hoisted(() => vi.fn());

vi.mock("@/stores/messages", () => {
  const [state, set] = createStore({
    byChannel: {},
    loadingChannels: {},
    hasMore: {},
    loadError: null,
  } as typeof store.state);
  store.state = state;
  store.set = set as typeof store.set;
  return {
    messagesState: state,
    setMessagesState: set,
    loadInitialMessages: vi.fn(),
    loadMessages: vi.fn(),
    loadMessagesForJump: (...a: unknown[]) => loadMessagesForJump(...a),
    hasMoreMessages: () => false,
  };
});
vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "g-chan", guild_id: "g1" }] },
  markChannelAsRead: vi.fn(),
  getChannelLastReadMessageId: () => null,
  isChannelUnread: () => false,
  getUnreadCount: () => 0,
}));
vi.mock("@/stores/guilds", () => ({ areThreadsEnabled: () => guildThreads.enabled }));
vi.mock("@/stores/dms", () => ({ dmsState: dms }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/components/ui/Toast", () => ({ showToast }));
vi.mock("@solidjs/router", () => {
  const [value, set] = createStore<Record<string, string | undefined>>({});
  params.value = value;
  params.set = (next) => set(next);
  return { useSearchParams: () => [value, params.set] };
});
vi.mock("../MessageItem", () => ({
  default: (props: { message: { id: string }; compact?: boolean; threadsEnabled?: boolean }) => (
    <div data-testid="msg" data-compact={String(!!props.compact)} data-threads={String(props.threadsEnabled)}>
      {props.message.id}
    </div>
  ),
  MessageImageLightbox: () => null,
}));

import MessageList from "../MessageList";

const msg = (id: string, iso: string, author = "a", content = `text ${id}`): Msg => ({
  id,
  created_at: iso,
  author: { id: author, display_name: author === "oma" ? "Oma" : author, username: author },
  content,
});

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  globalThis.IntersectionObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof IntersectionObserver;
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
});

beforeEach(() => {
  params.set({ highlight: undefined });
  dms.dms = [];
  guildThreads.enabled = true;
  loadMessagesForJump.mockReset();
  showToast.mockReset();
});
afterEach(cleanup);

const rows = () => screen.queryAllByTestId("msg");
const tick = () => new Promise((r) => setTimeout(r, 0));

describe("MessageList", () => {
  it("never offers threads in direct messages", async () => {
    dms.dms = [{ id: "dm1" }];
    store.set("byChannel", "dm1", [msg("m1", "2026-01-01T10:00:00Z")]);
    render(() => <MessageList channelId="dm1" />);
    await tick();
    expect(rows()[0].dataset.threads).toBe("false");
  });

  it("server channels follow the server's thread setting", async () => {
    guildThreads.enabled = false;
    store.set("byChannel", "g-chan", [msg("m1", "2026-01-01T10:00:00Z")]);
    render(() => <MessageList channelId="g-chan" />);
    await tick();
    expect(rows()[0].dataset.threads).toBe("false");
  });

  it("direct messages get their own empty-state hint", async () => {
    dms.dms = [{ id: "dm2" }];
    store.set("byChannel", "dm2", []);
    render(() => <MessageList channelId="dm2" />);
    expect(await screen.findByText("Write the first message.")).toBeTruthy();
  });

  it("separates days and never groups across midnight", async () => {
    store.set("byChannel", "days", [
      msg("m1", "2025-03-04T23:58:00"),
      msg("m2", "2025-03-05T00:01:00"),
      msg("m3", "2025-03-05T00:02:00"),
    ]);
    render(() => <MessageList channelId="days" />);
    await tick();
    expect(screen.getAllByRole("separator")).toHaveLength(2);
    const [m1, m2, m3] = rows();
    expect(m1.dataset.compact).toBe("false");
    expect(m2.dataset.compact).toBe("false");
    expect(m3.dataset.compact).toBe("true");
  });

  it("announces a new message from someone else to screen readers", async () => {
    vi.useFakeTimers();
    try {
      store.set("byChannel", "live", [msg("m1", "2026-01-01T10:00:00Z")]);
      render(() => <MessageList channelId="live" />);
      await vi.advanceTimersByTimeAsync(10);
      store.set("byChannel", "live", [
        ...store.state.byChannel.live,
        msg("m2", "2026-01-01T10:01:00Z", "oma", "Hallo zusammen"),
      ]);
      const region = screen.getByTestId("new-message-announcer");
      expect(region.getAttribute("aria-live")).toBe("polite");
      expect(region.textContent).toBe("");
      await vi.advanceTimersByTimeAsync(1600);
      expect(region.textContent).toBe("Oma: Hallo zusammen");

      // My own messages are not read back to me.
      store.set("byChannel", "live", [
        ...store.state.byChannel.live,
        msg("m3", "2026-01-01T10:02:00Z", "me", "mine"),
      ]);
      await vi.advanceTimersByTimeAsync(1600);
      expect(region.textContent).toBe("Oma: Hallo zusammen");
    } finally {
      vi.useRealTimers();
    }
  });

  it("loads the surroundings of a search result that is not loaded", async () => {
    store.set("byChannel", "jump", [msg("m50", "2026-01-01T10:00:00Z")]);
    loadMessagesForJump.mockImplementation(async () => {
      store.set("byChannel", "jump", [
        msg("m10", "2025-12-01T10:00:00Z"),
        ...store.state.byChannel.jump,
      ]);
      return true;
    });
    render(() => <MessageList channelId="jump" />);
    await tick();
    // A search result in the channel that is already open.
    params.set({ highlight: "m10" });
    await waitFor(() => expect(loadMessagesForJump).toHaveBeenCalledWith("jump", "m10"));
    await waitFor(() => expect(rows().map((r) => r.textContent)).toContain("m10"));
    expect(showToast).not.toHaveBeenCalled();
  });

  it("says so when the message cannot be found", async () => {
    store.set("byChannel", "gone", [msg("m50", "2026-01-01T10:00:00Z")]);
    loadMessagesForJump.mockResolvedValue(false);
    params.set({ highlight: "deleted" });
    render(() => <MessageList channelId="gone" />);
    await waitFor(() => expect(loadMessagesForJump).toHaveBeenCalledWith("gone", "deleted"));
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ title: "That message could not be found. It may have been deleted." }),
      ),
    );
  });
});
