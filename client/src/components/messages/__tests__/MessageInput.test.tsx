import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { HttpError } from "@/lib/tauri/common";
import { createSignal } from "solid-js";

const sendMessage = vi.fn();
const uploadMessageWithFile = vi.fn();
const validateFileSize = vi.fn((): string | null => null);
const clearDraft = vi.fn();
const drafts: Record<string, string> = {};
const addMessage = vi.fn();

vi.mock("@/stores/messages", () => ({
  sendMessage: (...a: unknown[]) => sendMessage(...a),
  messagesState: { error: null, sendError: null },
  addMessage: (...a: unknown[]) => addMessage(...a),
}));
vi.mock("@/stores/websocket", () => ({ stopTyping: vi.fn(), sendTyping: vi.fn() }));
vi.mock("@/lib/tauri", () => ({
  uploadMessageWithFile: (...a: unknown[]) => uploadMessageWithFile(...a),
  validateFileSize: () => validateFileSize(),
  getUploadLimitText: () => "10 MB",
  isUploadAborted: (e: unknown) => (e as { name?: string })?.name === "AbortError",
}));
const showToastMock = vi.fn();
vi.mock("@/components/ui/Toast", () => ({ showToast: (...a: unknown[]) => showToastMock(...a) }));
vi.mock("@/stores/drafts", () => ({
  getDraft: (id: string) => drafts[id] ?? "",
  saveDraft: (id: string, text: string) => {
    if (text.trim()) drafts[id] = text;
    else delete drafts[id];
  },
  clearDraft: (id: string) => {
    delete drafts[id];
    clearDraft(id);
  },
}));
vi.mock("@/stores/guilds", () => ({ guildsState: { members: {}, guilds: [] } }));
vi.mock("@/stores/channels", () => ({ channelsState: { channels: [] } }));
vi.mock("@/lib/api/bots", () => ({ listGuildCommands: vi.fn(async () => []) }));
vi.mock("@/components/emoji/PositionedEmojiPicker", () => ({ default: () => null }));
vi.mock("../AutocompletePopup", () => ({
  default: (p: { query: string; type: string }) => (
    <div data-testid="autocomplete">{`${p.type}:${p.query}`}</div>
  ),
}));

import MessageInput from "../MessageInput";

const dropFiles = (form: Element, files: File[]) => {
  const ev = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(ev, "dataTransfer", { value: { files } });
  form.dispatchEvent(ev);
};

beforeEach(() => {
  vi.clearAllMocks();
  for (const k of Object.keys(drafts)) delete drafts[k];
  drafts.c1 = "hello draft";
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
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    expect(screen.getByPlaceholderText("Message #general")).toBeTruthy();
  });

  it("send button label follows the pending file count", async () => {
    const { container } = render(() => <MessageInput channelId="c1" channelName="general" />);
    dropFiles(container.querySelector("form")!, [new File(["a"], "a.txt")]);
    expect(await screen.findByLabelText("Send 1 file")).toBeTruthy();
  });

  it("Enter with an autocomplete trigger and no matches sends the message", async () => {
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    ta.value = "hello @zzzq";
    ta.setSelectionRange(ta.value.length, ta.value.length);
    fireEvent.input(ta);
    fireEvent.keyDown(ta, { key: "Enter" });
    expect(sendMessage).toHaveBeenCalledTimes(1);
  });

  it("keeps the text and the draft when sending fails", async () => {
    sendMessage.mockResolvedValueOnce(null);
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    fireEvent.keyDown(ta, { key: "Enter" });
    await waitFor(() => expect(sendMessage).toHaveBeenCalledWith("c1", "hello draft"));
    await Promise.resolve();
    expect(ta.value).toBe("hello draft");
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("clears the text and the draft once the message is sent", async () => {
    sendMessage.mockResolvedValueOnce({ id: "m1" });
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    fireEvent.keyDown(ta, { key: "Enter" });
    await waitFor(() => expect(clearDraft).toHaveBeenCalledWith("c1"));
    expect(ta.value).toBe("");
  });

  it("does not lock the text box while sending", async () => {
    sendMessage.mockReturnValueOnce(new Promise(() => {}));
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    fireEvent.keyDown(ta, { key: "Enter" });
    await waitFor(() => expect(sendMessage).toHaveBeenCalled());
    expect(ta.disabled).toBe(false);
  });

  it("names the reason for a rejected file and keeps it, without resending sent files", async () => {
    uploadMessageWithFile
      .mockResolvedValueOnce({ id: "m1" })
      .mockRejectedValueOnce(new HttpError(415, "Invalid file type", "INVALID_MIME_TYPE"));
    const { container } = render(() => (
      <MessageInput channelId="c1" channelName="general" guildId="g1" />
    ));
    const form = container.querySelector("form")!;
    dropFiles(form, [new File(["a"], "a.png", { type: "image/png" }), new File(["b"], "b.heic")]);
    fireEvent.submit(form);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("b.heic was not sent.");
    expect(alert.textContent).toContain("This kind of file can't be sent.");
    // a.png went out (with the text) and left the tray; b.heic stays.
    expect(uploadMessageWithFile.mock.calls[0][2]).toBe("hello draft");
    expect(uploadMessageWithFile.mock.calls[1][2]).toBeUndefined();
    const tiles = screen.getAllByTestId("pending-file");
    expect(tiles).toHaveLength(1);
    expect(tiles[0].getAttribute("title")).toContain("This kind of file can't be sent.");
    expect(addMessage).toHaveBeenCalledTimes(1);
    expect(clearDraft).toHaveBeenCalledWith("c1");

    // Sending again only retries the rejected file.
    uploadMessageWithFile.mockResolvedValueOnce({ id: "m2" });
    fireEvent.submit(form);
    await waitFor(() => expect(uploadMessageWithFile).toHaveBeenCalledTimes(3));
    expect((uploadMessageWithFile.mock.calls[2][1] as File).name).toBe("b.heic");
  });

  it("keeps the text when every file fails", async () => {
    uploadMessageWithFile.mockRejectedValueOnce(new HttpError(413, "too big", "FILE_TOO_LARGE"));
    const { container } = render(() => (
      <MessageInput channelId="c1" channelName="general" guildId="g1" />
    ));
    const form = container.querySelector("form")!;
    dropFiles(form, [new File(["a"], "a.mp4", { type: "video/mp4" })]);
    fireEvent.submit(form);
    expect((await screen.findByRole("alert")).textContent).toContain("This file is too big to send.");
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    expect(ta.value).toBe("hello draft");
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("shows progress and cancels an upload", async () => {
    let signal: AbortSignal | undefined;
    let progress: ((l: number, t: number) => void) | undefined;
    uploadMessageWithFile.mockImplementationOnce(
      (_c: string, _f: File, _t: string, opts: { signal: AbortSignal; onProgress: (l: number, t: number) => void }) =>
        new Promise((_, reject) => {
          signal = opts.signal;
          progress = opts.onProgress;
          opts.signal.addEventListener("abort", () =>
            reject(new DOMException("Upload cancelled", "AbortError")),
          );
        }),
    );
    const { container } = render(() => (
      <MessageInput channelId="c1" channelName="general" guildId="g1" />
    ));
    const form = container.querySelector("form")!;
    dropFiles(form, [new File(["a"], "a.mp4", { type: "video/mp4" })]);
    fireEvent.submit(form);
    await waitFor(() => expect(progress).toBeDefined());
    progress!(50, 100);
    const bar = await screen.findByRole("progressbar");
    expect(bar.getAttribute("aria-valuenow")).toBe("50");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(signal!.aborted).toBe(true);
    await waitFor(() => expect(screen.queryByRole("progressbar")).toBeNull());
    expect(screen.getAllByTestId("pending-file")).toHaveLength(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("names the allowed file types next to the files", async () => {
    const { container } = render(() => <MessageInput channelId="c1" channelName="general" />);
    dropFiles(container.querySelector("form")!, [new File(["a"], "a.txt")]);
    expect(await screen.findByText(/Files that usually work: photos \(JPG/)).toBeTruthy();
  });

  it("an oversized file is refused with a translated sentence", async () => {
    validateFileSize.mockReturnValue("File too large");
    const { container } = render(() => <MessageInput channelId="c1" channelName="general" />);
    dropFiles(container.querySelector("form")!, [new File(["a"], "huge.mov")]);
    expect((await screen.findByRole("alert")).textContent).toBe(
      "huge.mov is too big. Files can be up to 10 MB.",
    );
  });

  it("pasting an image adds it as an attachment", async () => {
    render(() => <MessageInput channelId="c1" channelName="general" />);
    const ta = screen.getByPlaceholderText("Message general") as HTMLTextAreaElement;
    const ev = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(ev, "clipboardData", {
      value: { files: [new File(["x"], "image.png", { type: "image/png" })], types: ["Files"], getData: () => "" },
    });
    ta.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect(await screen.findByText("1 file ready to send")).toBeTruthy();
  });

  it("pasting text only leaves the paste alone", () => {
    render(() => <MessageInput channelId="c1" channelName="general" />);
    const ta = screen.getByPlaceholderText("Message general") as HTMLTextAreaElement;
    const ev = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(ev, "clipboardData", { value: { files: [] } });
    ta.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });

  it("direct messages get a placeholder without the channel sign", () => {
    render(() => <MessageInput channelId="c1" channelName="Oma" />);
    expect(screen.getByPlaceholderText("Message Oma")).toBeTruthy();
  });

  it("@ mentions with umlauts keep the suggestions open", async () => {
    render(() => <MessageInput channelId="c1" channelName="general" guildId="g1" />);
    const ta = screen.getByPlaceholderText("Message #general") as HTMLTextAreaElement;
    ta.value = "hi @Jö";
    ta.setSelectionRange(ta.value.length, ta.value.length);
    fireEvent.input(ta);
    expect((await screen.findByTestId("autocomplete")).textContent).toBe("user:Jö");
  });

  describe("fix round 1", () => {
    const deferred = <T,>() => {
      let resolve!: (v: T) => void;
      const promise = new Promise<T>((r) => (resolve = r));
      return { promise, resolve };
    };
    const box = () => screen.getByRole("textbox") as HTMLTextAreaElement;
    const type = (text: string) => {
      const ta = box();
      ta.value = text;
      ta.setSelectionRange(text.length, text.length);
      fireEvent.input(ta);
    };

    it("W1: switching channel mid-send clears the sent channel's draft, not the visible box", async () => {
      drafts.c1 = "Hallo";
      drafts.c2 = "Text in B";
      const sending = deferred<unknown>();
      sendMessage.mockReturnValueOnce(sending.promise);
      const [channel, setChannel] = createSignal("c1");
      render(() => <MessageInput channelId={channel()} channelName="x" guildId="g1" />);
      fireEvent.keyDown(box(), { key: "Enter" });
      await waitFor(() => expect(sendMessage).toHaveBeenCalledWith("c1", "Hallo"));
      setChannel("c2");
      await waitFor(() => expect(box().value).toBe("Text in B"));
      sending.resolve({ id: "m1" });
      await waitFor(() => expect(drafts.c1).toBeUndefined());
      expect(box().value).toBe("Text in B");
      expect(drafts.c2).toBe("Text in B");
      setChannel("c1");
      await waitFor(() => expect(box().value).toBe(""));
    });

    it("W2: text typed during the send stays, the sent part goes", async () => {
      drafts.c1 = "Hallo";
      const sending = deferred<unknown>();
      sendMessage.mockReturnValueOnce(sending.promise);
      render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      await waitFor(() => expect(box().value).toBe("Hallo"));
      fireEvent.keyDown(box(), { key: "Enter" });
      await waitFor(() => expect(sendMessage).toHaveBeenCalledTimes(1));
      type("Hallo wie geht's");
      sending.resolve({ id: "m1" });
      await waitFor(() => expect(box().value).toBe("wie geht's"));
      expect(drafts.c1).toBe("wie geht's");
    });

    it("W3: a file removed from the tray during the send is not uploaded", async () => {
      const first = deferred<unknown>();
      uploadMessageWithFile.mockReturnValueOnce(first.promise);
      const { container } = render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const form = container.querySelector("form")!;
      dropFiles(form, [new File(["a"], "a.png", { type: "image/png" }), new File(["b"], "wrong.png", { type: "image/png" })]);
      fireEvent.submit(form);
      await waitFor(() => expect(uploadMessageWithFile).toHaveBeenCalledTimes(1));
      const tiles = screen.getAllByTestId("pending-file");
      fireEvent.click(tiles[1].querySelector("button")!);
      expect(screen.getAllByTestId("pending-file")).toHaveLength(1);
      first.resolve({ id: "m1" });
      await waitFor(() => expect(screen.queryAllByTestId("pending-file")).toHaveLength(0));
      expect(uploadMessageWithFile).toHaveBeenCalledTimes(1);
    });

    it("K5: files stay with the channel they were added in", async () => {
      const [channel, setChannel] = createSignal("c1");
      const { container } = render(() => <MessageInput channelId={channel()} channelName="x" guildId="g1" />);
      dropFiles(container.querySelector("form")!, [new File(["a"], "a.heic")]);
      expect(screen.getAllByTestId("pending-file")).toHaveLength(1);
      setChannel("c2");
      await waitFor(() => expect(screen.queryAllByTestId("pending-file")).toHaveLength(0));
      setChannel("c1");
      await waitFor(() => expect(screen.getAllByTestId("pending-file")).toHaveLength(1));
    });

    it("K4: pasting text with a picture of it (spreadsheets) pastes the text", () => {
      render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", {
        value: {
          files: [new File(["x"], "image.png", { type: "image/png" })],
          types: ["Files", "text/plain", "text/html"],
          getData: (type: string) => (type === "text/plain" ? "A1\tB1" : ""),
        },
      });
      box().dispatchEvent(ev);
      expect(ev.defaultPrevented).toBe(false);
      expect(screen.queryAllByTestId("pending-file")).toHaveLength(0);
    });

    it("K3: a parser message from the upload route is not shown", async () => {
      uploadMessageWithFile.mockRejectedValueOnce(
        new HttpError(400, "Error parsing `multipart/form-data` request", "VALIDATION_ERROR"),
      );
      const { container } = render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const form = container.querySelector("form")!;
      dropFiles(form, [new File(["a"], "a.png", { type: "image/png" })]);
      fireEvent.submit(form);
      const alert = await screen.findByRole("alert");
      expect(alert.textContent).toBe("a.png was not sent. Please try again.");
    });
  });

  describe("fix round 2", () => {
    const deferred = <T,>() => {
      let resolve!: (v: T) => void;
      let reject!: (e: unknown) => void;
      const promise = new Promise<T>((r, j) => {
        resolve = r;
        reject = j;
      });
      return { promise, resolve, reject };
    };
    const paste = (data: { types: string[]; plain?: string; html?: string }) => {
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", {
        value: {
          files: [new File(["x"], "a.png", { type: "image/png" })],
          types: data.types,
          getData: (type: string) =>
            type === "text/plain" ? data.plain ?? "" : type === "text/html" ? data.html ?? "" : "",
        },
      });
      (screen.getByRole("textbox") as HTMLTextAreaElement).dispatchEvent(ev);
      return ev;
    };

    it("paste: a file from the file manager (path as plain text) is attached", () => {
      render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const ev = paste({ types: ["Files", "text/plain"], plain: "/home/oma/Bilder/a.png" });
      expect(ev.defaultPrevented).toBe(true);
      expect(screen.getAllByTestId("pending-file")).toHaveLength(1);
    });

    it("paste: a browser's copied image (HTML without text) is attached", () => {
      render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const ev = paste({ types: ["Files", "text/html"], html: '<img src="x">' });
      expect(ev.defaultPrevented).toBe(true);
      expect(screen.getAllByTestId("pending-file")).toHaveLength(1);
    });

    it("paste: spreadsheet cells (formatted text plus picture) paste as text", () => {
      render(() => <MessageInput channelId="c1" channelName="x" guildId="g1" />);
      const ev = paste({ types: ["Files", "text/plain", "text/rtf"], plain: "A1" });
      expect(ev.defaultPrevented).toBe(false);
      expect(screen.queryAllByTestId("pending-file")).toHaveLength(0);
    });

    it("N4: Cancel and errors belong to the channel that is sending", async () => {
      const upload = deferred<unknown>();
      uploadMessageWithFile.mockImplementationOnce(
        (_c: string, _f: File, _t: string, opts: { signal: AbortSignal }) => {
          opts.signal.addEventListener("abort", () => upload.reject(new DOMException("x", "AbortError")));
          return upload.promise;
        },
      );
      const [channel, setChannel] = createSignal("c1");
      const { container } = render(() => <MessageInput channelId={channel()} channelName={channel() === "c1" ? "A" : "B"} guildId="g1" />);
      const form = container.querySelector("form")!;
      dropFiles(form, [new File(["a"], "a.heic")]);
      fireEvent.submit(form);
      await waitFor(() => expect(uploadMessageWithFile).toHaveBeenCalledTimes(1));

      setChannel("c2");
      dropFiles(form, [new File(["b"], "b.png")]);
      // B is not sending: no Cancel for A's upload here.
      expect(await screen.findByText("1 file ready to send")).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();

      upload.reject(new HttpError(415, "Invalid file type", "INVALID_MIME_TYPE"));
      await waitFor(() => expect(showToastMock).toHaveBeenCalled());
      expect(showToastMock).toHaveBeenLastCalledWith(expect.objectContaining({ title: "Not sent in A" }));
      expect(screen.queryByRole("alert")).toBeNull();

      setChannel("c1");
      expect((await screen.findByRole("alert")).textContent).toContain("a.heic was not sent.");
    });

    it("N4: sending in another channel does not block this one", async () => {
      const first = deferred<unknown>();
      sendMessage.mockReturnValueOnce(first.promise).mockResolvedValueOnce({ id: "m2" });
      drafts.c2 = "Hallo B";
      const [channel, setChannel] = createSignal("c1");
      render(() => <MessageInput channelId={channel()} channelName="x" guildId="g1" />);
      fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
      await waitFor(() => expect(sendMessage).toHaveBeenCalledTimes(1));
      setChannel("c2");
      await waitFor(() => expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Hallo B"));
      fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
      await waitFor(() => expect(sendMessage).toHaveBeenLastCalledWith("c2", "Hallo B"));
      first.resolve({ id: "m1" });
    });

    it("N5: the file hint goes when the rejected file is removed", async () => {
      uploadMessageWithFile.mockRejectedValueOnce(new HttpError(415, "x", "INVALID_MIME_TYPE"));
      const { container } = render(() => (
        <MessageInput channelId="c1" channelName="general" guildId="g1" />
      ));
      const form = container.querySelector("form")!;
      dropFiles(form, [new File(["a"], "a.heic", { type: "image/heic" })]);
      fireEvent.submit(form);
      expect((await screen.findByRole("alert")).textContent).toContain("a.heic was not sent.");
      fireEvent.click(screen.getByRole("button", { name: "Remove file" }));
      await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    });

    it("N5: the file hint goes when the next send works", async () => {
      uploadMessageWithFile
        .mockRejectedValueOnce(new HttpError(503, "x", "STORAGE_NOT_CONFIGURED"))
        .mockResolvedValueOnce({ id: "m1" });
      const { container } = render(() => (
        <MessageInput channelId="c1" channelName="general" guildId="g1" />
      ));
      const form = container.querySelector("form")!;
      dropFiles(form, [new File(["a"], "a.png", { type: "image/png" })]);
      fireEvent.submit(form);
      expect((await screen.findByRole("alert")).textContent).toContain("a.png was not sent.");
      fireEvent.submit(form);
      await waitFor(() => expect(uploadMessageWithFile).toHaveBeenCalledTimes(2));
      await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    });
  });
});
