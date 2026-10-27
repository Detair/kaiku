import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { render } from "@solidjs/testing-library";
import { createSignal, onCleanup } from "solid-js";
import { createStore } from "solid-js/store";

// Real Solid store standing in for the messages store, so the test drives
// MessageList's rows (virtualizer + <For> + <Show>) with actual reactivity.
const store = vi.hoisted(() => ({
  state: undefined as unknown as {
    byChannel: Record<string, { id: string; created_at: string; author: { id: string } }[]>;
    loadingChannels: Record<string, boolean>;
    hasMore: Record<string, boolean>;
    loadError: string | null;
  },
  set: undefined as unknown as (...args: unknown[]) => void,
}));

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
    hasMoreMessages: () => false,
  };
});

vi.mock("@/stores/channels", () => ({
  markChannelAsRead: vi.fn(),
  getChannelLastReadMessageId: () => null,
  isChannelUnread: () => false,
  getUnreadCount: () => 0,
}));

vi.mock("@/stores/guilds", () => ({ areThreadsEnabled: () => true }));
vi.mock("@solidjs/router", () => ({ useSearchParams: () => [{}, vi.fn()] }));

// The hazard: a row's MessageItem reads its message prop during cleanup.
const cleanupReads = vi.hoisted(() => [] as string[]);
vi.mock("../MessageItem", () => ({
  default: (props: { message: { id: string } }) => {
    onCleanup(() => cleanupReads.push(props.message.id));
    return <div data-testid="msg">{props.message.id}</div>;
  },
  MessageImageLightbox: () => null,
}));

import MessageList from "../MessageList";

const msg = (n: number) => ({
  id: `m${String(n).padStart(3, "0")}`,
  created_at: new Date(Date.UTC(2026, 0, 1, 0, n)).toISOString(),
  author: { id: n % 2 ? "a" : "b" },
  content: `message ${n}`,
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
  // A scroll container with a real size so the virtualizer renders rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
});

beforeEach(() => {
  cleanupReads.length = 0;
});

function renderList(channelId: () => string) {
  return render(() => <MessageList channelId={channelId()} />);
}

const renderedIds = (container: HTMLElement) =>
  [...container.querySelectorAll('[data-testid="msg"]')].map((el) => el.textContent);

describe("MessageList rows", () => {
  it("disposes rows of removed messages without stale reads (eviction)", async () => {
    store.set("byChannel", "c1", Array.from({ length: 30 }, (_, i) => msg(i)));
    const { container } = renderList(() => "c1");
    await Promise.resolve();
    expect(renderedIds(container).length).toBeGreaterThan(0);

    expect(() =>
      store.set("byChannel", "c1", store.state.byChannel.c1.slice(10, 20)),
    ).not.toThrow();
    await Promise.resolve();
    for (const id of renderedIds(container)) {
      expect(store.state.byChannel.c1.some((m) => m.id === id)).toBe(true);
    }
  });

  it("removes the last message while it is rendered", async () => {
    store.set("byChannel", "c2", Array.from({ length: 3 }, (_, i) => msg(i)));
    const { container } = renderList(() => "c2");
    await Promise.resolve();
    expect(renderedIds(container)).toContain("m002");

    expect(() =>
      store.set("byChannel", "c2", store.state.byChannel.c2.slice(0, 2)),
    ).not.toThrow();
    await Promise.resolve();
    expect(renderedIds(container)).not.toContain("m002");
    expect(cleanupReads).toContain("m002");
  });

  it("switches to an empty channel without errors", async () => {
    store.set("byChannel", "c3", Array.from({ length: 10 }, (_, i) => msg(i)));
    store.set("byChannel", "c4", []);
    const [channel, setChannel] = createSignal("c3");
    const { container } = renderList(channel);
    await Promise.resolve();
    expect(renderedIds(container).length).toBeGreaterThan(0);

    expect(() => setChannel("c4")).not.toThrow();
    await Promise.resolve();
    expect(renderedIds(container)).toEqual([]);
  });

  it("keeps a row bound to its message when older messages are prepended", async () => {
    store.set("byChannel", "c5", Array.from({ length: 3 }, (_, i) => msg(i + 10)));
    const { container } = renderList(() => "c5");
    await Promise.resolve();
    const before = container.querySelector('[data-testid="msg"]');
    expect(before?.textContent).toBe("m010");

    store.set("byChannel", "c5", [msg(1), msg(2), ...store.state.byChannel.c5]);
    await Promise.resolve();
    // the same element still shows m010; no row was rebuilt for it
    const same = [...container.querySelectorAll('[data-testid="msg"]')].find(
      (el) => el.textContent === "m010",
    );
    expect(same).toBe(before);
    expect(cleanupReads).not.toContain("m010");
  });
});
