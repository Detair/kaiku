import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const sendThreadReply = vi.fn();
vi.mock("@/stores/threads", () => ({
  threadsState: {
    activeThreadId: "p1",
    activeThreadParent: null,
    repliesByThread: {},
    loadingThreads: {},
    hasMore: {},
  },
  closeThread: vi.fn(),
  loadMoreThreadReplies: vi.fn(),
  sendThreadReply: (...a: unknown[]) => sendThreadReply(...a),
}));
vi.mock("@/stores/guilds", () => ({ areThreadsEnabled: () => true }));
vi.mock("../MessageItem", () => ({ default: () => null }));

import ThreadSidebar from "../ThreadSidebar";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const typeReply = (text: string) => {
  const ta = screen.getByTestId("thread-reply-input") as HTMLTextAreaElement;
  ta.value = text;
  fireEvent.input(ta);
  fireEvent.keyDown(ta, { key: "Enter" });
  return ta;
};

describe("ThreadSidebar", () => {
  it("keeps the reply text when sending fails", async () => {
    sendThreadReply.mockResolvedValueOnce(null);
    render(() => <ThreadSidebar channelId="c1" guildId="g1" />);
    const ta = typeReply("my long answer");
    await waitFor(() => expect(sendThreadReply).toHaveBeenCalledWith("p1", "c1", "my long answer"));
    await Promise.resolve();
    expect(ta.value).toBe("my long answer");
  });

  it("clears the reply text once it is sent", async () => {
    sendThreadReply.mockResolvedValueOnce({ id: "r1" });
    render(() => <ThreadSidebar channelId="c1" guildId="g1" />);
    const ta = typeReply("ok");
    await waitFor(() => expect(ta.value).toBe(""));
  });

  it("is labelled and takes the full screen on narrow windows", () => {
    render(() => <ThreadSidebar channelId="c1" guildId="g1" />);
    const panel = screen.getByRole("complementary", { name: "Thread" });
    expect(panel.className).toContain("fixed inset-0");
    expect(panel.className).toContain("md:w-[400px]");
    expect(screen.getByText("No replies yet. Write the first one.")).toBeTruthy();
  });
});
