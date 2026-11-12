/**
 * Hanging up ends the call's voice session first (the intent
 * changes before anything is awaited, so no recovery attempt and no
 * pending join can run behind the hang-up), then tells the server.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const order = vi.hoisted(() => [] as string[]);
const connected = { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 };
const state = vi.hoisted(() => ({
  currentCall: { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 } as Record<string, unknown>,
  voiceReconnecting: false,
}));

vi.mock("@/stores/call", () => ({
  callState: state,
  setChannelCallId: vi.fn(),
  joinCall: vi.fn(),
  declineCall: vi.fn(),
  endCall: vi.fn(() => order.push("endCall")),
  acceptCall: vi.fn(),
  leaveCallVoice: vi.fn(() => {
    order.push("voiceNotWanted");
    return Promise.resolve().then(() => {
      order.push("voiceEnded");
    });
  }),
}));
vi.mock("@/lib/tauri", () => ({
  declineDMCall: vi.fn(),
  leaveDMCall: vi.fn(async () => {
    order.push("leaveDMCall");
  }),
}));

import CallBanner from "../CallBanner";
import { leaveCallVoice } from "@/stores/call";

afterEach(() => {
  cleanup();
  order.length = 0;
  state.currentCall = { ...connected };
  state.voiceReconnecting = false;
});

describe("CallBanner hang-up order", () => {
  it("leave: voice no longer wanted at once, then the server, then the call", async () => {
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Leave"));
    // Synchronously with the click: the voice is not wanted and the server is being told.
    expect(order).toEqual(["voiceNotWanted", "leaveDMCall"]);
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order.indexOf("leaveDMCall")).toBeLessThan(order.indexOf("endCall"));
  });

  it("cancel of an outgoing ring ends the voice first too", async () => {
    state.currentCall = { status: "outgoing_ringing", channelId: "dm-x", startedAt: 1 };
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Cancel"));
    expect(order).toEqual(["voiceNotWanted", "leaveDMCall"]);
    await waitFor(() => expect(order).toContain("endCall"));
  });

  it("the server is told although the voice teardown never returns", async () => {
    vi.mocked(leaveCallVoice).mockImplementationOnce(() => {
      order.push("voiceNotWanted");
      return new Promise<void>(() => {});
    });
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Leave"));
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order).toEqual(["voiceNotWanted", "leaveDMCall", "endCall"]);
  });

  it("while the call's voice reconnects the banner says so and hang-up works at once", async () => {
    state.voiceReconnecting = true;
    render(() => <CallBanner channelId="dm-x" />);
    expect(screen.getByText("Reconnecting...")).toBeTruthy();
    expect(screen.getByText("Connection lost. Getting the call's sound back.")).toBeTruthy();
    fireEvent.click(screen.getByTitle("Leave"));
    expect(order).toEqual(["voiceNotWanted", "leaveDMCall"]);
    await waitFor(() => expect(order).toContain("endCall"));
  });
});
