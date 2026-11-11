/**
 * W1-N3: hanging up stops the call's voice recovery first, then tears the
 * voice down, then tells the server.
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
  stopCallVoiceRecovery: vi.fn(() => order.push("stopRecovery")),
}));
vi.mock("@/lib/tauri", () => ({
  joinDMCall: vi.fn(),
  declineDMCall: vi.fn(),
  leaveDMCall: vi.fn(async () => {
    order.push("leaveDMCall");
  }),
  joinVoice: vi.fn(),
  leaveVoice: vi.fn(async () => {
    order.push("leaveVoice");
  }),
}));

import CallBanner from "../CallBanner";

afterEach(() => {
  cleanup();
  order.length = 0;
});

describe("CallBanner hang-up order", () => {
  it("leave: recovery stopped, then voice, then server", async () => {
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Leave"));
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order).toEqual(["stopRecovery", "leaveVoice", "leaveDMCall", "endCall"]);
  });

  it("cancel of an outgoing ring stops recovery first too", async () => {
    state.currentCall = { status: "outgoing_ringing", channelId: "dm-x", startedAt: 1 };
    render(() => <CallBanner channelId="dm-x" />);
    fireEvent.click(screen.getByTitle("Cancel"));
    await waitFor(() => expect(order).toContain("endCall"));
    expect(order[0]).toBe("stopRecovery");
  });
});
