import { beforeEach, describe, expect, it, vi } from "vitest";

import { sendMessageWithStatus } from "../tauri";

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
