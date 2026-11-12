/**
 * W1-N3: hanging up ends the call's voice session first (the intent
 * changes before anything is awaited, so no recovery attempt and no
 * pending join can run behind the hang-up), then tells the server.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const order = vi.hoisted(() => [] as string[]);
const state = vi.hoisted(() => ({
  currentCall: { status: "connected", channelId: "dm-x", participants: ["u2"], startedAt: 1 } as Record<string, unknown>,
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

afterEach(() => {
  cleanup();
  order.length = 0;
});

describe("CallBanner hang-up order", () => {
  it("leave: voice no longer wanted at once, ended, then the server, then the call", async () => {
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Leave"));
    // Synchronously with the click, before anything was awaited.
    expect(order).toEqual(["voiceNotWanted"]);
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order).toEqual(["voiceNotWanted", "voiceEnded", "leaveDMCall", "endCall"]);
  });

  it("cancel of an outgoing ring ends the voice first too", async () => {
    state.currentCall = { status: "outgoing_ringing", channelId: "dm-x", startedAt: 1 };
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Cancel"));
    expect(order).toEqual(["voiceNotWanted"]);
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order).toEqual(["voiceNotWanted", "voiceEnded", "leaveDMCall", "endCall"]);
  });
});
