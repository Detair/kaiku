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
    // Importing tauri/common fires a session-restore fetch in browser mode.
    // Left real, it fails with ECONNREFUSED and logs after the file finished
    // (vitest "Closing rpc while onUserConsoleLog was pending"). A fetch that
    // never settles keeps it silent and deterministic.
    vi.stubGlobal("fetch", () => new Promise<Response>(() => {}));
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(async () => {
    // Detach any socket a test left open so nothing reports after teardown.
    await loaded?.wsDisconnect();
    loaded = null;
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  let loaded: typeof import("../tauri/websocket") | null = null;

  async function load() {
    const common = await import("../tauri/common");
    common.browserState.accessToken = "tok";
    const ws = await import("../tauri/websocket");
    const onClose = vi.fn();
    ws.setBrowserSocketHooks({ onClose, onFrame: vi.fn() });
    loaded = ws;
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

  it("a replaced socket closing late stays silent", async () => {
    const { ws, onClose } = await load();
    const first = ws.wsConnect();
    const stale = FakeSocket.instances[0];
    await ws.wsDisconnect();
    onClose.mockClear();

    stale.closeWith(1006);

    const settled = vi.fn();
    void first.then(settled, settled);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
