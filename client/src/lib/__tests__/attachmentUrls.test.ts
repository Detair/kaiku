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

import {
  getAttachmentUrl,
  clearAttachmentUrls,
  invalidateAttachmentUrl,
  isAttachmentTooLarge,
  BLOB_FALLBACK_MAX_BYTES,
} from "../attachmentUrls";
import { HttpError } from "@/lib/tauri/common";
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

describe("attachment URL cache is bound to the session", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getSignedUrl).mockReset();
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
    vi.unstubAllGlobals();
  });

  function deferred<T>() {
    let resolve!: (v: T) => void;
    const promise = new Promise<T>((r) => (resolve = r));
    return { promise, resolve };
  }

  it("a presign that completes after sign-out is dropped, not cached", async () => {
    const signed = deferred<{ url: string; expires_in: number }>();
    vi.mocked(getSignedUrl).mockReturnValueOnce(signed.promise);
    const old = getAttachmentUrl("a1");
    clearAttachmentUrls(); // sign-out
    signed.resolve({ url: "https://s3.example/account-a", expires_in: 300 });
    await expect(old).rejects.toThrow(/ended session/);

    vi.mocked(getSignedUrl).mockResolvedValueOnce({ url: "https://s3.example/account-b", expires_in: 300 });
    expect(await getAttachmentUrl("a1")).toBe("https://s3.example/account-b");
  });

  it("a blob fallback that completes after sign-out never creates a blob URL", async () => {
    vi.mocked(getSignedUrl).mockRejectedValueOnce(new Error("presign failed"));
    const body = deferred<Blob>();
    fetchMock.mockResolvedValueOnce({ ok: true, headers: new Headers(), blob: () => body.promise });
    const old = getAttachmentUrl("b1");
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled());
    clearAttachmentUrls();
    body.resolve(new Blob(["account A"]));
    await expect(old).rejects.toThrow(/ended session/);
    // Nothing of the old session stays alive (a blob would be revoked).
    expect(created.filter((u) => !revoked.includes(u))).toEqual([]);
  });

  it("evicts the least recently used entry, not the oldest", async () => {
    vi.mocked(getSignedUrl).mockRejectedValue(new Error("presign failed"));
    fetchMock.mockImplementation(async () => ({
      ok: true,
      headers: new Headers(),
      blob: async () => new Blob(["x"]),
    }));
    const first = await getAttachmentUrl("id-0");
    for (let i = 1; i < 200; i++) await getAttachmentUrl(`id-${i}`);
    // id-0 is still on screen and used again: it must survive the next one.
    expect(await getAttachmentUrl("id-0")).toBe(first);
    await getAttachmentUrl("id-200");
    expect(revoked).not.toContain(first);
    expect(revoked).toEqual(["blob:test/2"]); // id-1 was least recent
  });

  it("caches 403/404 from the presign endpoint as gone for the session", async () => {
    vi.mocked(getSignedUrl).mockRejectedValue(new HttpError(404, "not found"));
    await expect(getAttachmentUrl("gone1")).rejects.toThrow(/404/);
    await expect(getAttachmentUrl("gone1")).rejects.toThrow(/404/);
    expect(getSignedUrl).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
    // A new session asks again.
    clearAttachmentUrls();
    await expect(getAttachmentUrl("gone1")).rejects.toThrow(/404/);
    expect(getSignedUrl).toHaveBeenCalledTimes(2);
  });

  it("forgets 'gone' after five minutes (access may be granted later)", async () => {
    vi.useFakeTimers();
    vi.mocked(getSignedUrl).mockRejectedValueOnce(new HttpError(403, "forbidden"));
    await expect(getAttachmentUrl("later1")).rejects.toThrow(/403/);
    vi.advanceTimersByTime(5 * 60_000 + 1);
    vi.mocked(getSignedUrl).mockResolvedValueOnce({ url: "https://s3.example/now", expires_in: 300 });
    expect(await getAttachmentUrl("later1")).toBe("https://s3.example/now");
    vi.useRealTimers();
  });

  it("does not load a file above the byte limit into a blob", async () => {
    vi.mocked(getSignedUrl).mockRejectedValue(new Error("presign failed"));
    fetchMock.mockResolvedValueOnce({
      ok: true,
      headers: new Headers({ "content-length": String(BLOB_FALLBACK_MAX_BYTES + 1) }),
      body: { cancel: vi.fn().mockResolvedValue(undefined) },
      blob: vi.fn(),
    });
    const err = await getAttachmentUrl("big1").catch((e: unknown) => e);
    expect(isAttachmentTooLarge(err)).toBe(true);
    expect(created).toEqual([]);

    // With the size known up front it does not even download.
    fetchMock.mockClear();
    const err2 = await getAttachmentUrl("big2", undefined, {
      sizeHint: BLOB_FALLBACK_MAX_BYTES + 1,
    }).catch((e: unknown) => e);
    expect(isAttachmentTooLarge(err2)).toBe(true);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("invalidate makes the next request ask for a fresh URL", async () => {
    vi.mocked(getSignedUrl)
      .mockResolvedValueOnce({ url: "https://s3.example/old", expires_in: 3600 })
      .mockResolvedValueOnce({ url: "https://s3.example/new", expires_in: 3600 });
    expect(await getAttachmentUrl("r1")).toBe("https://s3.example/old");
    invalidateAttachmentUrl("r1");
    expect(await getAttachmentUrl("r1")).toBe("https://s3.example/new");
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
