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
  };
  const supervisor = createConnectionSupervisor({ ...opts, ...overrides });
  return { supervisor, driver, opts };
}

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
});
