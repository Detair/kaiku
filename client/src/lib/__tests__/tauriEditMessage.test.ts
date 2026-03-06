import { beforeEach, describe, expect, it, vi } from "vitest";
import { editMessage } from "../tauri";

describe("editMessage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("sends PATCH request with content in browser mode", async () => {
    const updatedMessage = {
      id: "msg-1",
      channel_id: "ch-1",
      content: "updated content",
      edited_at: "2026-03-06T12:00:00Z",
    };

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: vi.fn().mockResolvedValue(JSON.stringify(updatedMessage)),
      }),
    );

    const result = await editMessage("msg-1", "updated content");

    expect(result).toEqual(updatedMessage);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringMatching(/\/api\/messages\/msg-1$/),
      expect.objectContaining({
        method: "PATCH",
        body: JSON.stringify({ content: "updated content" }),
      }),
    );
  });

  it("throws on HTTP error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 403,
        statusText: "Forbidden",
        text: vi.fn().mockResolvedValue("CONTENT_FILTERED"),
      }),
    );

    await expect(editMessage("msg-1", "bad content")).rejects.toThrow();
  });
});
