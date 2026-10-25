import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createConnectionSupervisor,
  nextBackoffMs,
  type ConnectionDriver,
  type SupervisorOptions,
} from "../websocket/connection";

function makeDriver() {
  return {
    open: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    close: vi.fn<() => void>(),
    sendPing: vi.fn<() => void>(),
    reconnectNow: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
  } satisfies ConnectionDriver;
}

function makeSupervisor(overrides: Partial<SupervisorOptions> = {}) {
  const driver = makeDriver();
  const opts = {
    driver,
    mode: "managed" as SupervisorOptions["mode"],
    onStatus: vi.fn<SupervisorOptions["onStatus"]>(),
    onRecovered: vi.fn<SupervisorOptions["onRecovered"]>().mockResolvedValue(undefined),
    ensureFreshToken: vi
      .fn<SupervisorOptions["ensureFreshToken"]>()
      .mockResolvedValue(true),
    onAuthRejected: vi.fn<() => void>(),
  };
  const supervisor = createConnectionSupervisor({ ...opts, ...overrides });
  return { supervisor, driver, opts };
}

/** What the browser driver rejects with when the socket closed before it opened. */
const closedBeforeOpen = (code: number) =>
  Object.assign(new Error("WebSocket closed before it opened"), {
    closeCode: code,
  });

describe("connection supervisor", () => {
  let active: { stop(): void } | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    // Neutral jitter so backoff delays are exact.
    vi.spyOn(Math, "random").mockReturnValue(0.5);
  });

  afterEach(() => {
    active?.stop();
    active = null;
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("nextBackoffMs grows to a 30s cap", () => {
    expect([1, 2, 3, 4, 5, 6, 7].map((a) => nextBackoffMs(a, () => 0.5))).toEqual([
      1000, 2000, 4000, 8000, 16000, 30000, 30000,
    ]);
  });

  it("nextBackoffMs applies at most ±20 % jitter", () => {
    expect(nextBackoffMs(1, () => 0)).toBe(800);
    expect(nextBackoffMs(1, () => 1)).toBe(1200);
  });

  it("reconnects after an unintentional close", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    supervisor.notifyClosed(false);
    await vi.advanceTimersByTimeAsync(1000);

    expect(driver.open).toHaveBeenCalledTimes(2);
    const calls = opts.onStatus.mock.calls.map((c) => `${c[0]}:${c[1]}`);
    const reconnectingIdx = calls.indexOf("reconnecting:1");
    expect(reconnectingIdx).toBeGreaterThanOrEqual(0);
    expect(calls.slice(reconnectingIdx + 1)).toContain("connected:0");
    expect(opts.onRecovered).toHaveBeenCalledTimes(1);
  });

  it("start() reports whether it opened the connection", async () => {
    const { supervisor, driver } = makeSupervisor();
    active = supervisor;

    expect(await supervisor.start()).toBe(true);
    expect(await supervisor.start()).toBe(false);

    supervisor.notifyClosed(false);
    expect(await supervisor.start()).toBe(false);
    expect(driver.open).toHaveBeenCalledTimes(1);
  });

  it("does not reconnect after an intentional close", async () => {
    const { supervisor, driver } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    supervisor.notifyClosed(true);
    await vi.advanceTimersByTimeAsync(60_000);

    expect(driver.open).toHaveBeenCalledTimes(1);
  });

  it("online event triggers an immediate attempt", async () => {
    const { supervisor, driver } = makeSupervisor();
    active = supervisor;
    await supervisor.start();
    supervisor.notifyClosed(false);

    window.dispatchEvent(new Event("online"));
    await vi.advanceTimersByTimeAsync(0);

    expect(driver.open).toHaveBeenCalledTimes(2);
  });

  it("visibility change pings an open socket", async () => {
    const { supervisor, driver } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    document.dispatchEvent(new Event("visibilitychange"));

    expect(driver.sendPing).toHaveBeenCalledTimes(1);
    expect(driver.open).toHaveBeenCalledTimes(1);
  });

  it("watchdog pings every 20 s and closes a silent socket", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    await vi.advanceTimersByTimeAsync(20_000);
    expect(driver.sendPing).toHaveBeenCalledTimes(1);
    expect(driver.close).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(25_000);
    expect(driver.close).toHaveBeenCalledTimes(1);
    expect(opts.onStatus).toHaveBeenLastCalledWith("reconnecting", 1);

    await vi.advanceTimersByTimeAsync(1000);
    expect(driver.open).toHaveBeenCalledTimes(2);
  });

  it("watchdog keeps a socket that receives frames", async () => {
    const { supervisor, driver } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    for (let i = 0; i < 6; i++) {
      await vi.advanceTimersByTimeAsync(15_000);
      supervisor.notifyFrame();
    }

    expect(driver.close).not.toHaveBeenCalled();
  });

  it("reconnect refreshes an expired token before reopening", async () => {
    const order: string[] = [];
    const ensureFreshToken = vi.fn(async () => {
      order.push("token");
      return true;
    });
    const { supervisor, driver } = makeSupervisor({ ensureFreshToken });
    active = supervisor;
    driver.open.mockImplementation(async () => {
      order.push("open");
    });
    await supervisor.start();

    supervisor.notifyClosed(false);
    await vi.advanceTimersByTimeAsync(1000);

    expect(ensureFreshToken).toHaveBeenCalledTimes(1);
    expect(order).toEqual(["open", "token", "open"]);
  });

  it("reconnect stops and reports session expiry when the token cannot be refreshed", async () => {
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    try {
      const { supervisor, driver } = makeSupervisor({
        ensureFreshToken: vi.fn(async () => false),
      });
      active = supervisor;
      await supervisor.start();

      supervisor.notifyClosed(false);
      await vi.advanceTimersByTimeAsync(60_000);

      expect(driver.open).toHaveBeenCalledTimes(1);
      expect(expired).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });

  it("backs off further after a failed attempt", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    await supervisor.start();
    driver.open.mockRejectedValueOnce(new Error("down"));

    supervisor.notifyClosed(false);
    await vi.advanceTimersByTimeAsync(1000);
    expect(driver.open).toHaveBeenCalledTimes(2);
    expect(opts.onStatus).toHaveBeenLastCalledWith("reconnecting", 2);

    await vi.advanceTimersByTimeAsync(1999);
    expect(driver.open).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(driver.open).toHaveBeenCalledTimes(3);
  });

  it("stop cancels a scheduled retry", async () => {
    const { supervisor, driver } = makeSupervisor();
    await supervisor.start();
    supervisor.notifyClosed(false);

    supervisor.stop();
    await vi.advanceTimersByTimeAsync(60_000);
    window.dispatchEvent(new Event("online"));
    await vi.advanceTimersByTimeAsync(0);

    expect(driver.open).toHaveBeenCalledTimes(1);
  });

  it("external mode leaves pings and silence detection to the owner", async () => {
    const { supervisor, driver, opts } = makeSupervisor({ mode: "external" });
    active = supervisor;
    await supervisor.start();

    await vi.advanceTimersByTimeAsync(120_000);

    expect(driver.sendPing).not.toHaveBeenCalled();
    expect(driver.close).not.toHaveBeenCalled();
    expect(opts.onStatus).toHaveBeenLastCalledWith("connected", 0);
  });

  it("external mode never schedules retries itself", async () => {
    const { supervisor, driver, opts } = makeSupervisor({ mode: "external" });
    active = supervisor;
    await supervisor.start();

    supervisor.notifyClosed(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(driver.open).toHaveBeenCalledTimes(1);

    supervisor.triggerNow("online");
    expect(driver.reconnectNow).toHaveBeenCalledTimes(1);

    supervisor.notifyOpened();
    await vi.advanceTimersByTimeAsync(0);
    expect(opts.onStatus).toHaveBeenLastCalledWith("connected", 0);
    expect(opts.onRecovered).toHaveBeenCalledTimes(1);
  });

  // External mode: the owner's status events are authoritative. The open()
  // promise (the `ws_connect` command) can settle long after the events that
  // describe the same attempt arrived, so a late settlement must not undo
  // newer state.
  describe("external mode: late open() settlement", () => {
    function deferred() {
      let resolve!: () => void;
      let reject!: (err: Error) => void;
      const promise = new Promise<void>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      return { promise, resolve, reject };
    }

    it("a late success after the connection was lost does not fake a recovery", async () => {
      const { supervisor, driver, opts } = makeSupervisor({ mode: "external" });
      active = supervisor;
      const open = deferred();
      driver.open.mockReturnValueOnce(open.promise);
      const started = supervisor.start();

      // ws:connected, then ws:reconnecting, before ws_connect settles.
      supervisor.notifyOpened();
      await vi.advanceTimersByTimeAsync(0);
      supervisor.notifyClosed(false);
      expect(opts.onStatus).toHaveBeenLastCalledWith("reconnecting", 1);

      open.resolve();
      await started;

      expect(opts.onStatus).toHaveBeenLastCalledWith("reconnecting", 1);
      expect(opts.onRecovered).not.toHaveBeenCalled();

      // Still waiting: the real ws:connected is what recovers.
      supervisor.notifyOpened();
      await vi.advanceTimersByTimeAsync(0);
      expect(opts.onStatus).toHaveBeenLastCalledWith("connected", 0);
      expect(opts.onRecovered).toHaveBeenCalledTimes(1);
    });

    it("a late failure after the connection recovered does not report a loss", async () => {
      const { supervisor, driver, opts } = makeSupervisor({ mode: "external" });
      active = supervisor;
      const open = deferred();
      driver.open.mockReturnValueOnce(open.promise);
      const started = supervisor.start();

      // The first attempt failed (ws:reconnecting) and Rust's retry connected
      // (ws:connected) before the ws_connect rejection arrives.
      supervisor.notifyClosed(false);
      expect(opts.onStatus).toHaveBeenLastCalledWith("reconnecting", 1);
      supervisor.notifyOpened();
      await vi.advanceTimersByTimeAsync(0);
      expect(opts.onStatus).toHaveBeenLastCalledWith("connected", 0);
      expect(opts.onRecovered).toHaveBeenCalledTimes(1);

      open.reject(new Error("WebSocket connection failed; retrying"));
      await expect(started).rejects.toThrow("retrying");

      expect(opts.onStatus).toHaveBeenLastCalledWith("connected", 0);
      const statuses = opts.onStatus.mock.calls.map((c) => c[0]);
      expect(statuses.filter((s) => s === "reconnecting")).toHaveLength(1);
    });

    it("a late success after stop()/start() does not close the new connection", async () => {
      const { supervisor, driver } = makeSupervisor({ mode: "external" });
      active = supervisor;
      const open = deferred();
      driver.open.mockReturnValueOnce(open.promise);
      const first = supervisor.start();

      supervisor.stop();
      // The owner replaced its connection with this one.
      expect(await supervisor.start()).toBe(true);

      open.resolve();
      expect(await first).toBe(false);
      expect(driver.close).not.toHaveBeenCalled();
    });
  });
});

describe("connection supervisor close codes (C11/C25)", () => {
  let active: { stop(): void } | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(0.5);
  });

  afterEach(() => {
    active?.stop();
    active = null;
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("1008 forces one token refresh, then reconnects", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    await supervisor.start();

    supervisor.notifyClosed(false, 1008);
    await vi.advanceTimersByTimeAsync(1000);

    expect(opts.ensureFreshToken).toHaveBeenCalledTimes(1);
    expect(opts.ensureFreshToken).toHaveBeenLastCalledWith(true);
    expect(driver.open).toHaveBeenCalledTimes(2);
    expect(opts.onAuthRejected).not.toHaveBeenCalled();
  });

  it("a second 1008 stops reconnecting and reports the auth rejection", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    await supervisor.start();
    supervisor.notifyClosed(false, 1008);
    await vi.advanceTimersByTimeAsync(1000);
    expect(driver.open).toHaveBeenCalledTimes(2);

    supervisor.notifyClosed(false, 1008);
    await vi.advanceTimersByTimeAsync(120_000);

    expect(opts.onAuthRejected).toHaveBeenCalledTimes(1);
    expect(driver.open).toHaveBeenCalledTimes(2);
    expect(opts.ensureFreshToken).toHaveBeenCalledTimes(1);
  });

  it("a rejected refresh after 1008 ends the session", async () => {
    const { supervisor, driver, opts } = makeSupervisor({
      ensureFreshToken: vi.fn().mockResolvedValue(false),
    });
    active = supervisor;
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    await supervisor.start();

    supervisor.notifyClosed(false, 1008);
    await vi.advanceTimersByTimeAsync(120_000);
    window.removeEventListener("kaiku:session-expired", expired);

    expect(driver.open).toHaveBeenCalledTimes(1);
    expect(expired).toHaveBeenCalledTimes(1);
    expect(opts.onAuthRejected).not.toHaveBeenCalled();
  });

  it.each([1011, 1013])(
    "%i backs off and catches up without a forced refresh",
    async (code) => {
      const { supervisor, driver, opts } = makeSupervisor();
      active = supervisor;
      await supervisor.start();

      supervisor.notifyClosed(false, code);
      await vi.advanceTimersByTimeAsync(1000);

      expect(driver.open).toHaveBeenCalledTimes(2);
      expect(opts.ensureFreshToken).toHaveBeenCalledTimes(1);
      expect(opts.ensureFreshToken).toHaveBeenLastCalledWith(false);
      expect(opts.onRecovered).toHaveBeenCalledTimes(1);
      expect(opts.onAuthRejected).not.toHaveBeenCalled();
    },
  );

  it("close-before-open refreshes once per streak, then plain backoff", async () => {
    const { supervisor, driver, opts } = makeSupervisor();
    active = supervisor;
    driver.open.mockRejectedValue(closedBeforeOpen(1006));
    await supervisor.start().catch(() => {});

    await vi.advanceTimersByTimeAsync(1000);
    await vi.advanceTimersByTimeAsync(2000);
    await vi.advanceTimersByTimeAsync(4000);

    expect(driver.open).toHaveBeenCalledTimes(4);
    expect(opts.ensureFreshToken.mock.calls.map((c) => c[0])).toEqual([
      true,
      false,
      false,
    ]);
    expect(opts.onAuthRejected).not.toHaveBeenCalled();
  });

  it("the refresh allowance returns once a connection stayed up for 60 s", async () => {
    const { supervisor, driver, opts } = makeSupervisor({
      // Keep the silence watchdog out of the 60 s window.
      timings: { pingEveryMs: 20_000, silenceLimitMs: 600_000 },
    });
    active = supervisor;
    driver.open.mockRejectedValueOnce(closedBeforeOpen(1006));
    await supervisor.start().catch(() => {});
    await vi.advanceTimersByTimeAsync(1000); // forced refresh, opens
    await vi.advanceTimersByTimeAsync(60_000); // healthy

    supervisor.notifyClosed(false, 1008);
    await vi.advanceTimersByTimeAsync(1000);

    expect(opts.ensureFreshToken.mock.calls.map((c) => c[0])).toEqual([
      true,
      true,
    ]);
    expect(opts.onAuthRejected).not.toHaveBeenCalled();
    expect(driver.open).toHaveBeenCalledTimes(3);
  });
});
