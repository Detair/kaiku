import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mockSetMute = vi.fn().mockResolvedValue(undefined);

const hoisted = vi.hoisted(() => ({
  native: false,
  register: vi.fn().mockResolvedValue(undefined),
  unregister: vi.fn().mockResolvedValue(undefined),
  showToast: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => hoisted.native }));
vi.mock("@tauri-apps/plugin-global-shortcut", () => ({
  register: hoisted.register,
  unregister: hoisted.unregister,
}));
vi.mock("@/components/ui/Toast", () => ({ showToast: hoisted.showToast }));

import {
  resolveState,
  PttController,
  mapCodeToTauriShortcut,
  keyCodeToLabel,
  checkGlobalPttBinding,
  parseKeyBinding,
  createBrowserPttListeners,
  createTauriPttListeners,
  resetPttFallbackNotice,
  type PttFullConfig,
} from "@/lib/pttManager";

describe("resolveState", () => {
  it("returns muted when PTT enabled and no keys held", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: false }, false, false)).toBe(true);
  });

  it("returns unmuted when PTT key held", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: false }, true, false)).toBe(false);
  });

  it("returns unmuted when only PTM enabled and no keys held", () => {
    expect(resolveState({ pttEnabled: false, ptmEnabled: true }, false, false)).toBe(false);
  });

  it("returns muted when PTM key held", () => {
    expect(resolveState({ pttEnabled: false, ptmEnabled: true }, false, true)).toBe(true);
  });

  it("returns muted when both PTT and PTM enabled, no keys held", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: true }, false, false)).toBe(true);
  });

  it("returns unmuted when both enabled and PTT held", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: true }, true, false)).toBe(false);
  });

  it("returns muted when both enabled and PTM held (mute wins)", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: true }, false, true)).toBe(true);
  });

  it("returns muted when both keys held (mute wins)", () => {
    expect(resolveState({ pttEnabled: true, ptmEnabled: true }, true, true)).toBe(true);
  });

  it("returns unmuted when neither PTT nor PTM enabled", () => {
    expect(resolveState({ pttEnabled: false, ptmEnabled: false }, false, false)).toBe(false);
  });
});

describe("PttController", () => {
  let controller: PttController;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    controller = new PttController(mockSetMute);
  });

  afterEach(() => {
    controller.deactivate();
    vi.useRealTimers();
  });

  it("mutes immediately on activate with PTT", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });

  it("unmutes on PTT key press", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    mockSetMute.mockClear();
    controller.handleKeyDown("Space");
    expect(mockSetMute).toHaveBeenCalledWith(false);
  });

  it("re-mutes after release delay on PTT key release", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    mockSetMute.mockClear();
    controller.handleKeyUp("Space");
    expect(mockSetMute).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });

  it("ignores duplicate keyUp (e.g. browser + Tauri both fire)", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    mockSetMute.mockClear();
    controller.handleKeyUp("Space");
    // Second keyUp should be ignored — timer should NOT restart
    vi.advanceTimersByTime(100);
    controller.handleKeyUp("Space");
    // Original timer fires at 200ms from first keyUp, not reset
    vi.advanceTimersByTime(100);
    expect(mockSetMute).toHaveBeenCalledTimes(1);
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });

  it("cancels release timer if key pressed again", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    controller.handleKeyUp("Space");
    mockSetMute.mockClear();
    vi.advanceTimersByTime(100);
    controller.handleKeyDown("Space");
    expect(mockSetMute).toHaveBeenCalledWith(false);
    vi.advanceTimersByTime(200);
    expect(mockSetMute).toHaveBeenCalledTimes(1);
  });

  it("ignores key repeat (duplicate keydown)", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    mockSetMute.mockClear();
    controller.handleKeyDown("Space");
    controller.handleKeyDown("Space");
    expect(mockSetMute).not.toHaveBeenCalled();
  });

  it("PTM mutes on key press", () => {
    controller.activate({
      pttEnabled: false, pttKey: null, pttReleaseDelay: 200,
      ptmEnabled: true, ptmKey: "KeyM", ptmReleaseDelay: 200,
    });
    mockSetMute.mockClear();
    controller.handleKeyDown("KeyM");
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });

  it("PTM unmutes after release delay", () => {
    controller.activate({
      pttEnabled: false, pttKey: null, pttReleaseDelay: 200,
      ptmEnabled: true, ptmKey: "KeyM", ptmReleaseDelay: 300,
    });
    controller.handleKeyDown("KeyM");
    mockSetMute.mockClear();
    controller.handleKeyUp("KeyM");
    expect(mockSetMute).not.toHaveBeenCalled();
    vi.advanceTimersByTime(300);
    expect(mockSetMute).toHaveBeenCalledWith(false);
  });

  it("mute wins when both PTT and PTM held", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: true, ptmKey: "KeyM", ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    mockSetMute.mockClear();
    controller.handleKeyDown("KeyM");
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });

  it("deactivate clears state and cancels timers", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    controller.handleKeyUp("Space");
    mockSetMute.mockClear();
    controller.deactivate();
    vi.advanceTimersByTime(500);
    expect(mockSetMute).not.toHaveBeenCalled();
  });

  it("ignores unrelated keys", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    mockSetMute.mockClear();
    controller.handleKeyDown("KeyA");
    controller.handleKeyUp("KeyA");
    expect(mockSetMute).not.toHaveBeenCalled();
  });

  it("releases all keys on releaseAll", () => {
    controller.activate({
      pttEnabled: true, pttKey: "Space", pttReleaseDelay: 200,
      ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 200,
    });
    controller.handleKeyDown("Space");
    mockSetMute.mockClear();
    controller.releaseAll();
    expect(mockSetMute).toHaveBeenCalledWith(true);
  });
});

describe("mapCodeToTauriShortcut", () => {
  it("maps Space", () => expect(mapCodeToTauriShortcut("Space")).toBe("Space"));
  it("maps letter keys", () => expect(mapCodeToTauriShortcut("KeyV")).toBe("V"));
  it("maps digit keys", () => expect(mapCodeToTauriShortcut("Digit5")).toBe("5"));
  it("maps function keys", () => expect(mapCodeToTauriShortcut("F5")).toBe("F5"));
  it("maps CapsLock", () => expect(mapCodeToTauriShortcut("CapsLock")).toBe("CapsLock"));
  it("maps Backquote", () => expect(mapCodeToTauriShortcut("Backquote")).toBe("`"));
  it("returns null for unmappable keys", () => expect(mapCodeToTauriShortcut("ContextMenu")).toBeNull());
});

describe("keyCodeToLabel", () => {
  it("maps Space", () => expect(keyCodeToLabel("Space")).toBe("Space"));
  it("maps letter keys", () => expect(keyCodeToLabel("KeyV")).toBe("V"));
  it("maps CapsLock", () => expect(keyCodeToLabel("CapsLock")).toBe("Caps Lock"));
  it("maps Backquote to ~", () => expect(keyCodeToLabel("Backquote")).toBe("~"));
  it("maps digit keys", () => expect(keyCodeToLabel("Digit3")).toBe("3"));
  it("maps function keys", () => expect(keyCodeToLabel("F12")).toBe("F12"));
  it("maps numpad keys", () => expect(keyCodeToLabel("Numpad5")).toBe("Numpad 5"));
  it("returns raw code for unknown keys", () => expect(keyCodeToLabel("ContextMenu")).toBe("ContextMenu"));
});

function pttConfig(pttKey: string): PttFullConfig {
  return {
    pttEnabled: true, pttKey, pttReleaseDelay: 0,
    ptmEnabled: false, ptmKey: null, ptmReleaseDelay: 0,
  };
}

describe("window-focused push-to-talk", () => {
  let cleanup: (() => void) | null = null;

  afterEach(() => {
    cleanup?.();
    cleanup = null;
    document.body.innerHTML = "";
    vi.clearAllMocks();
  });

  it("keydown in a text field does not transmit", () => {
    const controller = new PttController(mockSetMute);
    controller.activate(pttConfig("Space"));
    cleanup = createBrowserPttListeners(controller);
    mockSetMute.mockClear();

    for (const html of ["<input />", "<textarea></textarea>", '<div contenteditable="true"></div>']) {
      document.body.innerHTML = html;
      const field = document.body.firstElementChild as HTMLElement;
      field.dispatchEvent(new KeyboardEvent("keydown", { code: "Space", bubbles: true }));
    }
    expect(mockSetMute).not.toHaveBeenCalledWith(false);

    // The same key outside a text field still transmits.
    document.body.innerHTML = "<button></button>";
    document.body.firstElementChild!.dispatchEvent(
      new KeyboardEvent("keydown", { code: "Space", bubbles: true }),
    );
    expect(mockSetMute).toHaveBeenLastCalledWith(false);
    controller.deactivate();
  });

  it("modifier binding needs its modifier in the window", () => {
    const controller = new PttController(mockSetMute);
    controller.activate(pttConfig("Ctrl+KeyV"));
    cleanup = createBrowserPttListeners(controller);
    mockSetMute.mockClear();

    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyV" }));
    expect(mockSetMute).not.toHaveBeenCalled();
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyV", ctrlKey: true }));
    expect(mockSetMute).toHaveBeenLastCalledWith(false);
    controller.deactivate();
  });
});

describe("checkGlobalPttBinding", () => {
  it("plain V refused for global binding", () => {
    expect(checkGlobalPttBinding(parseKeyBinding("KeyV"))).toEqual({ ok: false, reason: "plainKey" });
    expect(checkGlobalPttBinding(parseKeyBinding("Space"))).toEqual({ ok: false, reason: "plainKey" });
    expect(checkGlobalPttBinding(parseKeyBinding("Shift+KeyV"))).toEqual({ ok: false, reason: "plainKey" });
    expect(checkGlobalPttBinding(parseKeyBinding("F12"))).toEqual({ ok: false, reason: "plainKey" });
  });

  it("Ctrl+V and F13 accepted", () => {
    expect(checkGlobalPttBinding(parseKeyBinding("Ctrl+KeyV"))).toEqual({ ok: true });
    expect(checkGlobalPttBinding(parseKeyBinding("Alt+Space"))).toEqual({ ok: true });
    expect(checkGlobalPttBinding(parseKeyBinding("Meta+KeyV"))).toEqual({ ok: true });
    expect(checkGlobalPttBinding(parseKeyBinding("F13"))).toEqual({ ok: true });
    expect(checkGlobalPttBinding(parseKeyBinding("F24"))).toEqual({ ok: true });
  });

  it("labels modifier bindings", () => {
    expect(keyCodeToLabel("Ctrl+KeyV")).toBe("Ctrl+V");
    expect(keyCodeToLabel("F13")).toBe("F13");
  });
});

describe("createTauriPttListeners", () => {
  let cleanup: (() => void | Promise<void>) | null = null;

  beforeEach(() => {
    hoisted.native = true;
    resetPttFallbackNotice();
  });

  afterEach(async () => {
    await cleanup?.();
    cleanup = null;
    hoisted.native = false;
    vi.clearAllMocks();
  });

  it("stored plain binding falls back to focus-only", async () => {
    const controller = new PttController(mockSetMute);
    const config = pttConfig("KeyV");
    controller.activate(config);
    cleanup = await createTauriPttListeners(controller, config);

    expect(hoisted.register).not.toHaveBeenCalled();
    expect(hoisted.showToast).toHaveBeenCalledTimes(1);

    mockSetMute.mockClear();
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyV" }));
    expect(mockSetMute).toHaveBeenLastCalledWith(false);
    window.dispatchEvent(new KeyboardEvent("keyup", { code: "KeyV" }));

    // Re-activating (e.g. joining another channel) does not toast again.
    await cleanup();
    cleanup = await createTauriPttListeners(controller, config);
    expect(hoisted.showToast).toHaveBeenCalledTimes(1);
    controller.deactivate();
  });

  it("registers a modifier binding as a global shortcut", async () => {
    const controller = new PttController(mockSetMute);
    const config = pttConfig("Ctrl+KeyV");
    controller.activate(config);
    cleanup = await createTauriPttListeners(controller, config);

    expect(hoisted.register).toHaveBeenCalledWith("Control+V", expect.any(Function));
    expect(hoisted.showToast).not.toHaveBeenCalled();

    const handler = hoisted.register.mock.calls[0][1] as (e: { state: string }) => void;
    mockSetMute.mockClear();
    handler({ state: "Pressed" });
    expect(mockSetMute).toHaveBeenLastCalledWith(false);
    controller.deactivate();
  });
});
