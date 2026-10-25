import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  onclose: ((e: { code: number }) => void) | null = null;
  constructor(
    public url: string,
    public protocols: string[],
  ) {
    FakeSocket.instances.push(this);
  }
  addEventListener() {}
  removeEventListener() {}
  send() {}
  close() {}
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  closeWith(code: number) {
    this.readyState = 3;
    this.onclose?.({ code });
  }
}

describe("browser WebSocket close reporting", () => {
  beforeEach(() => {
    FakeSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeSocket);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  async function load() {
    const common = await import("../tauri/common");
    common.browserState.accessToken = "tok";
    const ws = await import("../tauri/websocket");
    const onClose = vi.fn();
    ws.setBrowserSocketHooks({ onClose, onFrame: vi.fn() });
    return { ws, onClose };
  }

  it("passes the close code of an open socket to the supervisor hook", async () => {
    const { ws, onClose } = await load();
    const connected = ws.wsConnect();
    FakeSocket.instances[0].open();
    await connected;

    FakeSocket.instances[0].closeWith(1008);

    expect(onClose).toHaveBeenCalledWith(false, 1008);
  });

  it("a close before open rejects with the close code", async () => {
    const { ws } = await load();
    const connected = ws.wsConnect();

    FakeSocket.instances[0].onerror?.(new Event("error"));
    FakeSocket.instances[0].closeWith(1006);

    await expect(connected).rejects.toMatchObject({ closeCode: 1006 });
  });
});
