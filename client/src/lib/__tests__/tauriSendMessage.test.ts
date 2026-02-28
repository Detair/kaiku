import { beforeEach, describe, expect, it, vi } from "vitest";

import { sendMessageWithStatus, updateCustomStatus } from "../tauri";

describe("sendMessageWithStatus", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("parses plain-text error body without double-reading response", async () => {
    const textMock = vi.fn().mockResolvedValue("Command is ambiguous");
    const jsonMock = vi.fn().mockRejectedValue(new Error("body already used"));

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 400,
        statusText: "Bad Request",
        text: textMock,
        json: jsonMock,
      }),
    );

    await expect(sendMessageWithStatus("channel-1", "hello")).rejects.toThrow(
      "Command is ambiguous",
    );

    expect(textMock).toHaveBeenCalledTimes(1);
    expect(jsonMock).not.toHaveBeenCalled();
  });
});

describe("updateCustomStatus", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("sends status_message to /auth/me in browser mode", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(""),
    });
    vi.stubGlobal("fetch", fetchMock);

    await updateCustomStatus({ text: "In queue", emoji: "X" });

    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringMatching(/\/auth\/me$/),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ status_message: "X In queue" }),
      }),
    );
  });

  it("sends null status_message when clearing custom status", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(""),
    });
    vi.stubGlobal("fetch", fetchMock);

    await updateCustomStatus(null);

    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringMatching(/\/auth\/me$/),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ status_message: null }),
      }),
    );
  });

  it("includes display_name when provided for compatibility", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(""),
    });
    vi.stubGlobal("fetch", fetchMock);

    await updateCustomStatus({ text: "In queue", emoji: "X" }, "Me");

    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringMatching(/\/auth\/me$/),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          status_message: "X In queue",
          display_name: "Me",
        }),
      }),
    );
  });

  it("retries clear with empty string on legacy null handling", async () => {
    const firstErrorText = JSON.stringify({
      message: "Validation failed: No fields to update",
    });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: "Bad Request",
        text: vi.fn().mockResolvedValue(firstErrorText),
      })
      .mockResolvedValueOnce({
        ok: true,
        text: vi.fn().mockResolvedValue(""),
      });
    vi.stubGlobal("fetch", fetchMock);

    await updateCustomStatus(null);

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      expect.stringMatching(/\/auth\/me$/),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ status_message: null }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      expect.stringMatching(/\/auth\/me$/),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ status_message: "" }),
      }),
    );
  });
});
