import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, waitFor, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";

type Msg = {
  id: string;
  channel_id?: string;
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
const markChannelAsRead = vi.hoisted(() => vi.fn());
const loadInitialMessages = vi.hoisted(() => vi.fn());
const loadNewerMessages = vi.hoisted(() => vi.fn());
const newer = vi.hoisted(() => ({ channels: new Set<string>() }));

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
    loadInitialMessages: (...a: unknown[]) => loadInitialMessages(...a),
    loadMessages: vi.fn(),
    loadMessagesForJump: (...a: unknown[]) => loadMessagesForJump(...a),
    loadNewerMessages: (...a: unknown[]) => loadNewerMessages(...a),
    hasNewerMessages: (id: string) => newer.channels.has(id),
    hasMoreMessages: () => false,
  };
});
vi.mock("@/stores/channels", () => ({
  channelsState: { channels: [{ id: "g-chan", guild_id: "g1" }] },
  markChannelAsRead: (...a: unknown[]) => markChannelAsRead(...a),
  getChannelLastReadMessageId: () => null,
  isChannelUnread: () => true,
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

const msg = (id: string, iso: string, author = "a", content = `text ${id}`, channel_id = "live"): Msg => ({
  id,
  channel_id,
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
  markChannelAsRead.mockReset();
  loadInitialMessages.mockReset();
  loadNewerMessages.mockReset();
  newer.channels.clear();
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

  it("K1: a channel switch drops queued announcements of the old channel", async () => {
    vi.useFakeTimers();
    try {
      store.set("byChannel", "live", [msg("m1", "2026-01-01T10:00:00Z")]);
      store.set("byChannel", "other", [msg("o1", "2026-01-01T10:00:00Z", "a", "x", "other")]);
      const [channel, setChannel] = createSignal("live");
      render(() => <MessageList channelId={channel()} />);
      await vi.advanceTimersByTimeAsync(10);
      store.set("byChannel", "live", [
        ...store.state.byChannel.live,
        msg("m2", "2026-01-01T10:01:00Z", "oma", "Geheim aus A"),
      ]);
      await vi.advanceTimersByTimeAsync(500);
      setChannel("other");
      await vi.advanceTimersByTimeAsync(1600);
      expect(screen.getByTestId("new-message-announcer").textContent).toBe("");
    } finally {
      vi.useRealTimers();
    }
  });

  it("W4: an older window never marks read, loads newer at the bottom, and Jump to newest reloads", async () => {
    vi.useFakeTimers();
    try {
      newer.channels.add("old");
      store.set("byChannel", "old", [
        msg("m10", "2025-01-01T10:00:00Z", "a", "x", "old"),
        msg("m11", "2025-01-01T10:01:00Z", "a", "y", "old"),
      ]);
      render(() => <MessageList channelId="old" />);
      await vi.advanceTimersByTimeAsync(10);
      fireEvent.scroll(screen.getByRole("list"));
      await vi.advanceTimersByTimeAsync(3500);
      expect(markChannelAsRead).not.toHaveBeenCalled();
      expect(loadNewerMessages).toHaveBeenCalledWith("old");
      loadInitialMessages.mockClear();
      fireEvent.click(screen.getByTestId("jump-to-latest"));
      expect(loadInitialMessages).toHaveBeenCalledWith("old");
    } finally {
      vi.useRealTimers();
    }
  });

  it("R3-1: a read timer started in one channel never marks another", async () => {
    vi.useFakeTimers();
    try {
      store.set("byChannel", "a", [msg("a1", "2026-01-01T10:00:00Z", "x", "x", "a")]);
      store.set("byChannel", "b", [msg("b1", "2026-01-01T10:00:00Z", "x", "x", "b")]);
      const [channel, setChannel] = createSignal("a");
      render(() => <MessageList channelId={channel()} />);
      await vi.advanceTimersByTimeAsync(10);
      fireEvent.scroll(screen.getByRole("list"));
      await vi.advanceTimersByTimeAsync(1000);
      setChannel("b");
      // The timer from channel a is due now; b has been open for 2 s only.
      await vi.advanceTimersByTimeAsync(2100);
      expect(markChannelAsRead).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("W4: at the newest message, reading still marks read", async () => {
    vi.useFakeTimers();
    try {
      store.set("byChannel", "now", [msg("n1", "2026-01-01T10:00:00Z", "a", "x", "now")]);
      render(() => <MessageList channelId="now" />);
      await vi.advanceTimersByTimeAsync(10);
      fireEvent.scroll(screen.getByRole("list"));
      await vi.advanceTimersByTimeAsync(3500);
      expect(markChannelAsRead).toHaveBeenCalledWith("now", "n1");
      expect(loadNewerMessages).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("N1: a newer page loaded below the view is not marked read", async () => {
    vi.useFakeTimers();
    try {
      newer.channels.add("gap");
      store.set("byChannel", "gap", [msg("g10", "2025-01-01T10:00:00Z", "a", "x", "gap")]);
      render(() => <MessageList channelId="gap" />);
      await vi.advanceTimersByTimeAsync(10);
      const list = screen.getByRole("list");
      const size = { scrollHeight: 1000, clientHeight: 600, scrollTop: 400 };
      for (const key of Object.keys(size) as (keyof typeof size)[]) {
        Object.defineProperty(list, key, { configurable: true, get: () => size[key] });
      }
      // The next page is the newest one: it goes in below the view.
      loadNewerMessages.mockImplementationOnce(async () => {
        newer.channels.delete("gap");
        size.scrollHeight = 5000;
        store.set("byChannel", "gap", [
          ...store.state.byChannel.gap,
          msg("g11", "2026-01-01T10:00:00Z", "a", "y", "gap"),
        ]);
      });
      fireEvent.scroll(list);
      await vi.advanceTimersByTimeAsync(3500);
      expect(loadNewerMessages).toHaveBeenCalledWith("gap");
      expect(markChannelAsRead).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("a message I send while at the bottom marks the channel read once it is confirmed", async () => {
    vi.useFakeTimers();
    try {
      store.set("byChannel", "send", [msg("s1", "2026-01-01T10:00:00Z", "a", "x", "send")]);
      render(() => <MessageList channelId="send" />);
      await vi.advanceTimersByTimeAsync(3500);
      markChannelAsRead.mockClear();
      store.set("byChannel", "send", [
        ...store.state.byChannel.send,
        msg("pending:1", "2026-01-01T10:01:00Z", "me", "hi", "send"),
      ]);
      await vi.advanceTimersByTimeAsync(100);
      // Confirmed by the server before the timer runs.
      store.set("byChannel", "send", [
        store.state.byChannel.send[0],
        msg("s2", "2026-01-01T10:01:00Z", "me", "hi", "send"),
      ]);
      await vi.advanceTimersByTimeAsync(3500);
      expect(markChannelAsRead).toHaveBeenCalledWith("send", "s2");
    } finally {
      vi.useRealTimers();
    }
  });

  it("a message I send after a far jump does not mark the unseen newer stretch read", async () => {
    vi.useFakeTimers();
    try {
      newer.channels.add("far");
      store.set("byChannel", "far", [msg("f1", "2025-01-01T10:00:00Z", "a", "x", "far")]);
      render(() => <MessageList channelId="far" />);
      await vi.advanceTimersByTimeAsync(10);
      store.set("byChannel", "far", [
        ...store.state.byChannel.far,
        msg("f2", "2026-01-01T10:01:00Z", "me", "hi", "far"),
      ]);
      await vi.advanceTimersByTimeAsync(3500);
      expect(markChannelAsRead).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});

