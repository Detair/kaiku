// Components render through @solidjs/testing-library with the real i18n
// (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/stores/dms", () => ({
  getSelectedDM: () => ({
    id: "dm1",
    name: "Weekend plans",
    icon_url: null,
    participants: [
      { user_id: "me", username: "me", display_name: "Me" },
      { user_id: "u2", username: "anna", display_name: "Anna" },
      { user_id: "u3", username: "ben", display_name: "Ben" },
    ],
  }),
  updateDMIconUrl: vi.fn(),
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
vi.mock("@/components/messages/MessageList", () => ({ default: () => null }));
vi.mock("@/components/messages/MessageInput", () => ({ default: () => null }));
vi.mock("@/components/messages/TypingIndicator", () => ({ default: () => null }));
vi.mock("@/components/call", () => ({ CallBanner: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import DMConversation from "../DMConversation";

afterEach(cleanup);

describe("DMConversation", () => {
  it("group DM header pluralises members", () => {
    render(() => <DMConversation />);
    expect(screen.getByText("3 members")).toBeTruthy();
  });
});
