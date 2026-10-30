import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { watchSsoPopup, waitForSessionRestore, CLOSE_GRACE_MS, RESTORE_POLL_MS, RESTORE_WAIT_CAP_MS } from "../oidcPopup";

const ORIGIN = window.location.origin;
const msg = (source: unknown) =>
  new MessageEvent("message", {
    origin: ORIGIN,
    source: source as MessageEventSource,
    data: { type: "oidc-callback", access_token: "tok", setup_required: true },
  });

describe("watchSsoPopup", () => {
  let popup: { closed: boolean };
  let onResult: ReturnType<typeof vi.fn<(d: unknown) => void>>;
  let onClosed: ReturnType<typeof vi.fn<() => void>>;
  let dispose: () => void;
  beforeEach(() => {
    vi.useFakeTimers();
    popup = { closed: false };
    onResult = vi.fn();
    onClosed = vi.fn();
    dispose = watchSsoPopup({
      popup: popup as unknown as Window,
      expectedOrigin: ORIGIN,
      onResult,
      onClosedWithoutResult: onClosed,
    });
  });
  afterEach(() => {
    dispose();
    vi.useRealTimers();
  });

  it("ignores a forged message from another same-origin window", () => {
    window.dispatchEvent(msg({}));
    expect(onResult).not.toHaveBeenCalled();
  });

  it("accepts the message from the popup", () => {
    window.dispatchEvent(msg(popup));
    expect(onResult).toHaveBeenCalledTimes(1);
  });

  it("still delivers a message that arrives just after the popup closed", () => {
    popup.closed = true;
    vi.advanceTimersByTime(600);
    window.dispatchEvent(msg(popup));
    vi.advanceTimersByTime(CLOSE_GRACE_MS * 2);
    expect(onResult).toHaveBeenCalledTimes(1);
    expect(onClosed).not.toHaveBeenCalled();
  });

  it("reports closed-without-result once after the grace period", () => {
    popup.closed = true;
    vi.advanceTimersByTime(600);
    expect(onClosed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(CLOSE_GRACE_MS + 100);
    expect(onClosed).toHaveBeenCalledTimes(1);
    window.dispatchEvent(msg(popup));
    expect(onResult).not.toHaveBeenCalled();
  });

  it("stops everything when disposed", () => {
    dispose();
    popup.closed = true;
    vi.advanceTimersByTime(5000);
    window.dispatchEvent(msg(popup));
    expect(onClosed).not.toHaveBeenCalled();
    expect(onResult).not.toHaveBeenCalled();
  });
});

describe("waitForSessionRestore", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("retries on interval and focus until it works, then stops", async () => {
    const restore = vi.fn<() => Promise<boolean>>().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValue(true);
    const onDone = vi.fn();
    waitForSessionRestore({ restore, onDone });
    await vi.advanceTimersByTimeAsync(0);
    expect(restore).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(restore).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(RESTORE_POLL_MS);
    expect(onDone).toHaveBeenCalledWith(true);
    const calls = restore.mock.calls.length;
    await vi.advanceTimersByTimeAsync(RESTORE_POLL_MS * 3);
    expect(restore.mock.calls.length).toBe(calls);
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("gives up at the cap", async () => {
    const restore = vi.fn<() => Promise<boolean>>().mockResolvedValue(false);
    const onDone = vi.fn();
    waitForSessionRestore({ restore, onDone });
    await vi.advanceTimersByTimeAsync(RESTORE_WAIT_CAP_MS + 10);
    expect(onDone).toHaveBeenCalledExactlyOnceWith(false);
    const calls = restore.mock.calls.length;
    await vi.advanceTimersByTimeAsync(RESTORE_POLL_MS * 3);
    expect(restore.mock.calls.length).toBe(calls);
  });

  it("dispose stops retries without calling onDone", async () => {
    const restore = vi.fn<() => Promise<boolean>>().mockResolvedValue(false);
    const onDone = vi.fn();
    const stop = waitForSessionRestore({ restore, onDone });
    stop();
    const calls = restore.mock.calls.length;
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(RESTORE_WAIT_CAP_MS + 10);
    expect(restore.mock.calls.length).toBe(calls);
    expect(onDone).not.toHaveBeenCalled();
  });
});
