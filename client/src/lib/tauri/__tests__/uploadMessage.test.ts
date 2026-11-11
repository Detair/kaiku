import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const validateFileSize = vi.fn((): string | null => null);
const refreshAccessToken = vi.fn(async () => false);
const auth = { token: "tok" };
vi.mock("../common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../common")>()),
  getUploadAuth: async () => ({ token: auth.token, baseUrl: "https://chat.example" }),
  validateFileSize: () => validateFileSize(),
  refreshAccessToken: () => refreshAccessToken(),
}));

import { uploadMessageWithFile, isUploadAborted } from "../messages";
import { HttpError } from "../common";
import { errorText, CHAT_SEND_ERRORS } from "@/i18n/errors";

class FakeXHR {
  static last: FakeXHR;
  method = "";
  url = "";
  headers: Record<string, string> = {};
  status = 0;
  statusText = "";
  responseText = "";
  upload: { onprogress: ((e: ProgressEvent) => void) | null } = { onprogress: null };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  sent: unknown;
  constructor() {
    FakeXHR.last = this;
  }
  open(method: string, url: string) {
    this.method = method;
    this.url = url;
  }
  setRequestHeader(k: string, v: string) {
    this.headers[k] = v;
  }
  send(body: unknown) {
    this.sent = body;
  }
  abort() {
    this.onabort?.();
  }
  respond(status: number, body: string) {
    this.status = status;
    this.responseText = body;
    this.onload?.();
  }
}

const file = () => new File(["abc"], "photo.heic", { type: "image/heic" });
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  validateFileSize.mockReturnValue(null);
  refreshAccessToken.mockReset();
  refreshAccessToken.mockResolvedValue(false);
  auth.token = "tok";
  vi.stubGlobal("XMLHttpRequest", FakeXHR);
});
afterEach(() => vi.unstubAllGlobals());

describe("uploadMessageWithFile", () => {
  it("posts the file with the token and resolves the message", async () => {
    const p = uploadMessageWithFile("c1", file(), "hi");
    await flush();
    const xhr = FakeXHR.last;
    expect(xhr.method).toBe("POST");
    expect(xhr.url).toBe("https://chat.example/api/messages/channel/c1/upload");
    expect(xhr.headers.Authorization).toBe("Bearer tok");
    expect((xhr.sent as FormData).get("content")).toBe("hi");
    xhr.respond(201, JSON.stringify({ id: "m1" }));
    await expect(p).resolves.toEqual({ id: "m1" });
  });

  it("rejects a refused type with the server code, which names the reason", async () => {
    const p = uploadMessageWithFile("c1", file());
    await flush();
    FakeXHR.last.respond(
      415,
      JSON.stringify({ error: "INVALID_MIME_TYPE", message: "Invalid file type: image/heic" }),
    );
    const err = await p.catch((e) => e);
    expect(err).toBeInstanceOf(HttpError);
    expect((err as HttpError).code).toBe("INVALID_MIME_TYPE");
    expect(errorText(err, "chat.send.fileFailed", CHAT_SEND_ERRORS.byCode, CHAT_SEND_ERRORS.byStatus))
      .toMatch(/^This kind of file can't be sent\./);
  });

  it("a plain-text 413 from a proxy still reads as too big", async () => {
    const p = uploadMessageWithFile("c1", file());
    await flush();
    FakeXHR.last.respond(413, "<html>Request Entity Too Large</html>");
    const err = await p.catch((e) => e);
    expect(errorText(err, "chat.send.fileFailed", CHAT_SEND_ERRORS.byCode, CHAT_SEND_ERRORS.byStatus))
      .toBe("This file is too big to send.");
  });

  it("refuses an oversized file before uploading", async () => {
    validateFileSize.mockReturnValue("too big");
    const err = await uploadMessageWithFile("c1", file()).catch((e) => e);
    expect((err as HttpError).code).toBe("FILE_TOO_LARGE");
  });

  it("reports progress", async () => {
    const onProgress = vi.fn();
    const p = uploadMessageWithFile("c1", file(), undefined, { onProgress });
    await flush();
    FakeXHR.last.upload.onprogress?.({ loaded: 5, total: 10, lengthComputable: true } as ProgressEvent);
    expect(onProgress).toHaveBeenCalledWith(5, 10);
    FakeXHR.last.respond(200, "{}");
    await p;
  });

  it("cancels through the signal", async () => {
    const controller = new AbortController();
    const p = uploadMessageWithFile("c1", file(), undefined, { signal: controller.signal });
    await flush();
    controller.abort();
    const err = await p.catch((e) => e);
    expect(isUploadAborted(err)).toBe(true);
  });

  it("a lost connection reads as a network problem", async () => {
    const p = uploadMessageWithFile("c1", file());
    await flush();
    FakeXHR.last.onerror?.();
    const err = await p.catch((e) => e);
    expect(err).toBeInstanceOf(TypeError);
  });

  it("K2: renews an expired session once and sends again", async () => {
    refreshAccessToken.mockImplementationOnce(async () => {
      auth.token = "fresh";
      return true;
    });
    const p = uploadMessageWithFile("c1", file(), "hi");
    await flush();
    const first = FakeXHR.last;
    first.respond(401, JSON.stringify({ error: "INVALID_TOKEN", message: "expired" }));
    await flush();
    await flush();
    const second = FakeXHR.last;
    expect(second).not.toBe(first);
    expect(second.headers.Authorization).toBe("Bearer fresh");
    second.respond(201, JSON.stringify({ id: "m1" }));
    await expect(p).resolves.toEqual({ id: "m1" });
    expect(refreshAccessToken).toHaveBeenCalledTimes(1);
  });

  it("K2: a session that cannot be renewed fails without looping", async () => {
    const p = uploadMessageWithFile("c1", file());
    await flush();
    FakeXHR.last.respond(401, JSON.stringify({ error: "INVALID_TOKEN" }));
    const err = await p.catch((e) => e);
    expect((err as HttpError).status).toBe(401);
    expect(refreshAccessToken).toHaveBeenCalledTimes(1);
  });
});
