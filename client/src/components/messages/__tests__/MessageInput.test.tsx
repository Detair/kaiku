import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const sendMessage = vi.fn();
const uploadMessageWithFile = vi.fn();
const validateFileSize = vi.fn((): string | null => null);

vi.mock("@/stores/messages", () => ({
  sendMessage: (...a: unknown[]) => sendMessage(...a),
  messagesState: { error: null, sendError: null },
  addMessage: vi.fn(),
}));
vi.mock("@/stores/websocket", () => ({ stopTyping: vi.fn(), sendTyping: vi.fn() }));
vi.mock("@/lib/tauri", () => ({
  uploadMessageWithFile: (...a: unknown[]) => uploadMessageWithFile(...a),
  validateFileSize: () => validateFileSize(),
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

const dropFiles = (form: Element, files: File[]) => {
  const ev = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(ev, "dataTransfer", { value: { files } });
  form.dispatchEvent(ev);
};

beforeEach(() => {
  validateFileSize.mockReturnValue(null);
  URL.createObjectURL = vi.fn(() => "blob:x");
  URL.revokeObjectURL = vi.fn();
});
afterEach(cleanup);

describe("MessageInput", () => {
  it("dismissing an upload error does not send the draft", async () => {
    validateFileSize.mockReturnValue("File too large");
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

  it("pending files label pluralises", async () => {
    const { container } = render(() => <MessageInput channelId="c1" channelName="general" />);
    const form = container.querySelector("form")!;
    dropFiles(form, [new File(["a"], "a.txt")]);
    expect(await screen.findByText("1 file ready to send")).toBeTruthy();
    dropFiles(form, [new File(["b"], "b.txt")]);
    expect(await screen.findByText("2 files ready to send")).toBeTruthy();
  });

  it("placeholder carries the channel name", () => {
    render(() => <MessageInput channelId="c1" channelName="general" />);
    expect(screen.getByPlaceholderText("Message #general")).toBeTruthy();
  });

  it("send button label follows the pending file count", async () => {
    const { container } = render(() => <MessageInput channelId="c1" channelName="general" />);
    dropFiles(container.querySelector("form")!, [new File(["a"], "a.txt")]);
    expect(await screen.findByLabelText("Send 1 file")).toBeTruthy();
  });
});
