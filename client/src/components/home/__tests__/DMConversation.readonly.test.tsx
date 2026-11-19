import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

const selected = vi.hoisted(() => ({ dm: null as unknown }));
vi.mock("@/stores/dms", () => ({
  getSelectedDM: () => selected.dm,
  updateDMIconUrl: vi.fn(),
  readVisibleDM: vi.fn(),
  isWindowActive: () => true,
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/stores/call", () => ({
  callState: { currentCall: { status: "idle" } },
  startCall: vi.fn(),
  endCall: vi.fn(),
  isInCallForChannel: () => false,
}));
vi.mock("@/lib/tauri", () => ({
  startDMCall: vi.fn(),
  joinVoice: vi.fn(),
  uploadDMAvatar: vi.fn(),
  updateDMName: vi.fn(),
  validateFileSize: vi.fn(),
  getUploadLimitText: () => "5 MB",
}));
const messages = vi.hoisted(() => ({
  loadInitialMessages: vi.fn().mockResolvedValue(undefined),
  messagesState: { byChannel: {} },
}));
vi.mock("@/stores/messages", () => messages);
vi.mock("@/components/messages/MessageList", () => ({ default: () => <div data-testid="list" /> }));
vi.mock("@/components/messages/MessageInput", () => ({ default: () => <div data-testid="composer" /> }));
vi.mock("@/components/messages/TypingIndicator", () => ({ default: () => null }));
vi.mock("@/components/call", () => ({ CallBanner: () => <div data-testid="call-banner" /> }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import DMConversation from "../DMConversation";
import { isKaikuDM } from "@/lib/kaikuDm";

const kaiku = { user_id: "6b61696b-7500-4000-8000-000000000001", username: "kaiku", display_name: "Kaiku", avatar_url: null, joined_at: "", is_system: true };
const me = { user_id: "me", username: "alice", display_name: "Alice", avatar_url: null, joined_at: "" };

beforeEach(() => messages.loadInitialMessages.mockClear());
afterEach(cleanup);

describe("Kaiku DM", () => {
  it("kaiku dm has no composer and no call buttons", () => {
    selected.dm = { id: "k1", name: "", icon_url: null, read_only: true, participants: [me, kaiku] };
    render(() => <DMConversation />);
    expect(screen.queryByTestId("composer")).toBeNull();
    expect(screen.queryByTestId("call-banner")).toBeNull();
    expect(screen.queryByRole("button", { name: "Start call" })).toBeNull();
    expect(screen.getByText("Only Kaiku writes here: notices about your account's security.")).toBeTruthy();
    expect(screen.getByText("Security")).toBeTruthy();
  });

  it("read_only alone closes the composer and calls (Fable review m3)", () => {
    selected.dm = { id: "k2", name: "", icon_url: null, read_only: true, participants: [me, { ...kaiku, is_system: false }] };
    render(() => <DMConversation />);
    expect(screen.queryByTestId("composer")).toBeNull();
    expect(screen.queryByRole("button", { name: "Start call" })).toBeNull();
  });

  it("opening the kaiku dm reloads its latest page", () => {
    selected.dm = { id: "k1", name: "", icon_url: null, read_only: true, participants: [me, kaiku] };
    render(() => <DMConversation />);
    expect(messages.loadInitialMessages).toHaveBeenCalledWith("k1");
  });

  it("kaiku dm is recognised by is_system and read_only only", () => {
    const impostor = { ...kaiku, is_system: false };
    expect(isKaikuDM({ read_only: true, participants: [me, kaiku] })).toBe(true);
    expect(isKaikuDM({ read_only: false, participants: [me, kaiku] })).toBe(false);
    expect(isKaikuDM({ read_only: true, participants: [me, impostor] })).toBe(false);
    selected.dm = { id: "d2", name: "", icon_url: null, participants: [me, impostor] };
    render(() => <DMConversation />);
    expect(screen.getByTestId("composer")).toBeTruthy();
    expect(messages.loadInitialMessages).not.toHaveBeenCalled();
  });
});
