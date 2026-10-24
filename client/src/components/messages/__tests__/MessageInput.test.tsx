import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";

const sendMessage = vi.fn();
const uploadMessageWithFile = vi.fn();

vi.mock("@/stores/messages", () => ({
  sendMessage: (...a: unknown[]) => sendMessage(...a),
  messagesState: { error: null },
  addMessage: vi.fn(),
}));
vi.mock("@/stores/websocket", () => ({ stopTyping: vi.fn(), sendTyping: vi.fn() }));
vi.mock("@/lib/tauri", () => ({
  uploadMessageWithFile: (...a: unknown[]) => uploadMessageWithFile(...a),
  validateFileSize: () => "File too large",
  getUploadLimitText: () => "10 MB",
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/drafts", () => ({
  getDraft: () => "hello draft",
  saveDraft: vi.fn(),
  clearDraft: vi.fn(),
}));
vi.mock("@/stores/guilds", () => ({ guildsState: { members: {}, guilds: [] } }));
vi.mock("@/stores/channels", () => ({ channelsState: { channels: [] } }));
vi.mock("@/lib/api/bots", () => ({ listGuildCommands: vi.fn(async () => []) }));
vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));
vi.mock("../AutocompletePopup", () => ({ default: () => null }));

import MessageInput from "../MessageInput";

describe("MessageInput", () => {
  it("dismissing an upload error does not send the draft", async () => {
    const { container } = render(() => (
      <MessageInput channelId="c1" channelName="general" guildId="g1" />
    ));
    // Drop an oversized file so the upload error shows.
    const form = container.querySelector("form")!;
    const file = new File(["x"], "big.bin");
    fireEvent.drop(form, { dataTransfer: { files: [file] } });
    const dismiss = await screen.findByRole("button", { name: "Dismiss error" });
    expect(dismiss.getAttribute("type")).toBe("button");
    const submit = vi.fn((e: Event) => e.preventDefault());
    form.addEventListener("submit", submit);
    fireEvent.click(dismiss);
    expect(submit).not.toHaveBeenCalled();
    expect(sendMessage).not.toHaveBeenCalled();
    expect(uploadMessageWithFile).not.toHaveBeenCalled();
    expect(screen.queryByText("File too large")).toBeNull();
  });
});
