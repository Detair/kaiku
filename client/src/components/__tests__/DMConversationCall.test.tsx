/**
 * Starting a call from the DM view (W1 round 3): the outgoing state is set
 * before the HTTP start, the native join carries this call's own voice
 * token, and a failed start still rolls back via abortOutgoingCall.
 * (Kept outside components/home/: that folder belongs to another stream.)
 */
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const tauri = vi.hoisted(() => ({
  startDMCall: vi.fn(),
  joinVoice: vi.fn(),
  leaveVoice: vi.fn(),
  leaveVoiceSilently: vi.fn(),
  leaveDMCall: vi.fn(),
  uploadDMAvatar: vi.fn(),
  updateDMName: vi.fn(),
  validateFileSize: vi.fn(),
  getUploadLimitText: () => "5 MB",
}));
const toast = vi.hoisted(() => ({ showToast: vi.fn(), dismissToast: vi.fn() }));

vi.mock("@/lib/tauri", () => tauri);
vi.mock("@/lib/tauri/common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri/common")>()),
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/lib/sound/ring", () => ({ startRinging: vi.fn(), stopRinging: vi.fn() }));
vi.mock("@/stores/dms", () => ({
  getSelectedDM: () => ({
    id: "dm1",
    name: "Anna",
    icon_url: null,
    participants: [
      { user_id: "me", username: "me", display_name: "Me" },
      { user_id: "u2", username: "anna", display_name: "Anna" },
    ],
  }),
  updateDMIconUrl: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/components/messages/MessageList", () => ({ default: () => null }));
vi.mock("@/components/messages/MessageInput", () => ({ default: () => null }));
vi.mock("@/components/messages/TypingIndicator", () => ({ default: () => null }));
vi.mock("@/components/call", () => ({ CallBanner: () => null }));
vi.mock("@/components/ui/Toast", () => toast);

import DMConversation from "@/components/home/DMConversation";
import { callState, setCallState, callEventMatches } from "@/stores/call";
import { isVoiceEpochStale } from "@/lib/webrtc/voiceSession";
import { startRinging } from "@/lib/sound/ring";

beforeEach(() => {
  setCallState({ currentCall: { status: "idle" }, activeCallsByChannel: {}, callIds: {}, endedCallIds: {} });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("DM view: start call", () => {
  it("is outgoing before the HTTP start, then joins with this call's token", async () => {
    let respond!: (v: { call_id: string }) => void;
    tauri.startDMCall.mockImplementationOnce(() => new Promise((r) => (respond = r)));
    tauri.joinVoice.mockImplementationOnce(async (_ch: string, token: number) => {
      expect(isVoiceEpochStale(token)).toBe(false);
    });
    render(() => <DMConversation />);
    fireEvent.click(screen.getByLabelText("Start voice call"));

    // Before the response: our own outgoing call, so the server's echo of
    // it is matched to this call and nothing rings.
    expect(callState.currentCall.status).toBe("outgoing_ringing");
    expect(callEventMatches("dm1")).toBe(true);
    expect(startRinging).not.toHaveBeenCalled();

    await waitFor(() => expect(respond).toBeTypeOf("function"));
    respond({ call_id: "call-1" });
    await waitFor(() => expect(tauri.joinVoice).toHaveBeenCalled());
    expect(tauri.joinVoice).toHaveBeenCalledWith("dm1", expect.any(Number));
    expect(callState.callIds.dm1).toBe("call-1");
    expect(startRinging).not.toHaveBeenCalled();
  });

  it("a failed start shows the error and rolls back as before", async () => {
    tauri.startDMCall.mockRejectedValueOnce(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(() => <DMConversation />);
    fireEvent.click(screen.getByLabelText("Start voice call"));
    await waitFor(() => expect(toast.showToast).toHaveBeenCalled());
    expect(tauri.joinVoice).not.toHaveBeenCalled();
    // abortOutgoingCall(..., "failed") ends the optimistic call.
    expect(callState.currentCall.status).toBe("ended");
  });
});
