import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { watchSsoPopup, reloadAfterSsoPopup, CLOSE_GRACE_MS } from "../oidcPopup";
import { isSessionRestoreBlocked, setSessionRestoreBlocked } from "../tauri/common";

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

describe("reloadAfterSsoPopup", () => {
  it("lifts the logout's restore block, then reloads", () => {
    setSessionRestoreBlocked(true);
    const reload = vi.fn(() => expect(isSessionRestoreBlocked()).toBe(false));
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, reload } });
    try {
      reloadAfterSsoPopup();
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
      setSessionRestoreBlocked(false);
    }
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
