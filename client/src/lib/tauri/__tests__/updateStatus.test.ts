import { describe, it, expect, vi, beforeEach } from "vitest";

const send = vi.hoisted(() => vi.fn());
vi.mock("../websocket", () => ({
  getBrowserWebSocket: () => ({ send }),
  wsSend: vi.fn(),
}));
vi.mock("../common", async (orig) => ({
  ...(await orig<object>()),
  isTauri: () => false,
}));

import { updateStatus } from "../auth";

describe("updateStatus", () => {
  beforeEach(() => send.mockReset());

  it("never sends invisible; falls back to online", async () => {
    await updateStatus("invisible" as never);
    expect(JSON.parse(send.mock.calls[0][0])).toEqual({
      type: "set_status",
      status: "online",
    });
  });

  it("never sends offline either (the server only accepts online, away, busy)", async () => {
    await updateStatus("offline");
    expect(JSON.parse(send.mock.calls[0][0])).toEqual({
      type: "set_status",
      status: "online",
    });
  });

  it("still maps idle and dnd", async () => {
    await updateStatus("dnd");
    expect(JSON.parse(send.mock.calls[0][0]).status).toBe("busy");
  });
});
