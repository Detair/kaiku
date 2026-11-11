/**
 * C24: attachment URLs never carry the access token.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

vi.mock("@/lib/tauri/common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri/common")>()),
  getAuthContext: vi.fn(async () => ({ baseUrl: "https://chat.example", accessToken: "SECRET.JWT" })),
}));

vi.mock("@/lib/tauri/messages", () => ({
  getSignedUrl: vi.fn(),
}));

import { getAttachmentUrl, clearAttachmentUrls } from "../attachmentUrls";
import { getSignedUrl } from "@/lib/tauri/messages";

const fetchMock = vi.fn();
const created: string[] = [];
const revoked: string[] = [];

describe("attachment URLs (C24)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearAttachmentUrls();
    created.length = 0;
    revoked.length = 0;
    vi.stubGlobal("fetch", fetchMock);
    let n = 0;
    URL.createObjectURL = vi.fn(() => {
      const url = `blob:test/${++n}`;
      created.push(url);
      return url;
    });
    URL.revokeObjectURL = vi.fn((url: string) => {
      revoked.push(url);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("uses the presigned URL and caches it until shortly before expiry", async () => {
    vi.useFakeTimers();
    vi.mocked(getSignedUrl).mockResolvedValue({ url: "https://s3.example/a?sig=1", expires_in: 300 });

    expect(await getAttachmentUrl("a1", "thumbnail")).toBe("https://s3.example/a?sig=1");
    expect(await getAttachmentUrl("a1", "thumbnail")).toBe("https://s3.example/a?sig=1");
    expect(getSignedUrl).toHaveBeenCalledTimes(1);
    expect(getSignedUrl).toHaveBeenCalledWith("a1", "thumbnail");

    // Another variant is its own entry.
    await getAttachmentUrl("a1");
    expect(getSignedUrl).toHaveBeenCalledTimes(2);

    // One minute before expiry it asks again.
    vi.advanceTimersByTime(240_000);
    vi.mocked(getSignedUrl).mockResolvedValue({ url: "https://s3.example/a?sig=2", expires_in: 300 });
    expect(await getAttachmentUrl("a1", "thumbnail")).toBe("https://s3.example/a?sig=2");
    expect(getSignedUrl).toHaveBeenCalledTimes(3);
  });

  it("shares one request between concurrent callers", async () => {
    vi.mocked(getSignedUrl).mockResolvedValue({ url: "https://s3.example/b", expires_in: 300 });
    const [x, y] = await Promise.all([getAttachmentUrl("b1"), getAttachmentUrl("b1")]);
    expect(x).toBe(y);
    expect(getSignedUrl).toHaveBeenCalledTimes(1);
  });

  it("falls back to an authorized fetch and a blob URL, without a token in the URL", async () => {
    vi.mocked(getSignedUrl).mockRejectedValue(new Error("presign failed"));
    fetchMock.mockResolvedValue({ ok: true, blob: async () => new Blob(["x"]) });

    const url = await getAttachmentUrl("c1", "thumbnail");
    expect(url).toBe("blob:test/1");
    const [requestUrl, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(requestUrl).toBe("https://chat.example/api/messages/attachments/c1/download?variant=thumbnail");
    expect(requestUrl).not.toMatch(/token=/);
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer SECRET.JWT");

    // Cached: no second download.
    expect(await getAttachmentUrl("c1", "thumbnail")).toBe("blob:test/1");
    expect(fetchMock).toHaveBeenCalledTimes(1);

    clearAttachmentUrls();
    expect(revoked).toEqual(["blob:test/1"]);
  });

  it("reports a failed fallback download", async () => {
    vi.mocked(getSignedUrl).mockRejectedValue(new Error("presign failed"));
    fetchMock.mockResolvedValue({ ok: false, status: 403 });
    await expect(getAttachmentUrl("d1")).rejects.toThrow(/403/);
  });

  it("no request URL built for attachments contains the token", async () => {
    vi.mocked(getSignedUrl).mockResolvedValueOnce({ url: "https://s3.example/e", expires_in: 300 });
    vi.mocked(getSignedUrl).mockRejectedValueOnce(new Error("presign failed"));
    fetchMock.mockResolvedValue({ ok: true, blob: async () => new Blob(["x"]) });
    const urls = [await getAttachmentUrl("e1"), await getAttachmentUrl("e2", "medium")];
    const requested = fetchMock.mock.calls.map((c) => String(c[0]));
    for (const u of [...urls, ...requested]) {
      expect(u).not.toMatch(/token=/);
      expect(u).not.toContain("SECRET.JWT");
    }
  });
});

describe("no token in URLs anywhere in the client (C24)", () => {
  const SRC = join(__dirname, "..", "..");

  function sourceFiles(dir: string): string[] {
    const out: string[] = [];
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) {
        if (name === "__tests__" || name === "node_modules") continue;
        out.push(...sourceFiles(path));
      } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) {
        out.push(path);
      }
    }
    return out;
  }

  it("no source file builds a `token=` query parameter", () => {
    const offenders: string[] = [];
    for (const file of sourceFiles(SRC)) {
      const text = readFileSync(file, "utf8");
      if (/[?&]token=|\.set\(\s*["']token["']|\.append\(\s*["']token["']/.test(text)) {
        offenders.push(relative(SRC, file));
      }
    }
    expect(offenders).toEqual([]);
  });
});
