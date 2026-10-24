/**
 * Push-to-Talk / Push-to-Mute state management.
 *
 * `resolveState` is a pure function that derives the mute flag from config
 * and current key states.  `PttController` wraps it with key tracking,
 * release-delay timers, and a callback into the audio subsystem.
 */

import { isTauri } from "@/lib/tauri/common";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Minimal config slice needed by resolveState. */
export interface PttConfig {
  pttEnabled: boolean;
  ptmEnabled: boolean;
}

/** Full runtime config used by PttController. */
export interface PttFullConfig {
  pttEnabled: boolean;
  pttKey: string | null;
  pttReleaseDelay: number; // ms, default 200
  ptmEnabled: boolean;
  ptmKey: string | null;
  ptmReleaseDelay: number; // ms, default 200
}

// ---------------------------------------------------------------------------
// Pure state resolution
// ---------------------------------------------------------------------------

/**
 * Determine whether the mic should be muted given the current config and
 * key states.
 *
 * Truth table (muted = true):
 *  - PTT only:  rest=muted,   PTT held=unmuted
 *  - PTM only:  rest=unmuted, PTM held=muted
 *  - Both:      rest=muted (PTT defines rest), PTT held=unmuted, PTM held=muted
 *  - Both held: muted (mute wins — safety first)
 *  - Neither:   unmuted (VAD mode)
 */
export function resolveState(
  config: PttConfig,
  pttHeld: boolean,
  ptmHeld: boolean,
): boolean {
  // PTM held always wins — muting takes priority for safety.
  if (config.ptmEnabled && ptmHeld) return true;

  // PTT mode: muted when key is NOT held.
  if (config.pttEnabled) return !pttHeld;

  // Neither PTT nor PTM enabled → unmuted (VAD / open-mic mode).
  return false;
}

// ---------------------------------------------------------------------------
// Key bindings
// ---------------------------------------------------------------------------

/**
 * A push-to-talk key plus modifiers. Stored as a string such as `KeyV`,
 * `Ctrl+KeyV` or `F13` (modifiers in the order Ctrl, Alt, Shift, Meta,
 * then the browser `event.code`); a bare code is a plain key.
 */
export interface KeyBinding {
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"] as const;

/** Parse a stored binding string (`Ctrl+KeyV`, `Space`, ...). */
export function parseKeyBinding(stored: string): KeyBinding {
  const parts = stored.split("+");
  const code = parts.pop() ?? "";
  return {
    code,
    ctrl: parts.includes("Ctrl"),
    alt: parts.includes("Alt"),
    shift: parts.includes("Shift"),
    meta: parts.includes("Meta"),
  };
}

/** Serialise a binding to its stored string form. */
export function formatKeyBinding(b: KeyBinding): string {
  const flags = [b.ctrl, b.alt, b.shift, b.meta];
  const mods = MODIFIERS.filter((_, i) => flags[i]);
  return [...mods, b.code].join("+");
}

/** Build a binding from a keydown event. */
export function bindingFromEvent(e: KeyboardEvent): KeyBinding {
  return { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey };
}

export type BindingCheck = { ok: true } | { ok: false; reason: "plainKey" };

/**
 * Whether a binding may be registered as a global (system-wide) shortcut.
 * The OS hands a registered shortcut to Kaiku only, so a plain key would be
 * taken away from games; allow F13–F24 or any key with Ctrl, Alt or Meta.
 */
export function checkGlobalPttBinding(b: KeyBinding): BindingCheck {
  if (b.ctrl || b.alt || b.meta) return { ok: true };
  const fn = b.code.match(/^F(\d{1,2})$/);
  if (fn && Number(fn[1]) >= 13 && Number(fn[1]) <= 24) return { ok: true };
  return { ok: false, reason: "plainKey" };
}

/** True when the event target is a text field, where typing must not transmit. */
function isTextEntryTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return true;
  return target.isContentEditable || target.closest("[contenteditable='true'], [contenteditable='']") !== null;
}

/** Whether a keydown event carries every modifier the binding asks for. */
function modifiersHeld(b: KeyBinding, e: KeyboardEvent): boolean {
  return (!b.ctrl || e.ctrlKey) && (!b.alt || e.altKey) && (!b.shift || e.shiftKey) && (!b.meta || e.metaKey);
}

// ---------------------------------------------------------------------------
// Key code mapping
// ---------------------------------------------------------------------------

/** Convert a stored binding (`Ctrl+KeyV`) or `event.code` to a label. */
export function keyCodeToLabel(stored: string): string {
  if (stored.includes("+")) {
    const b = parseKeyBinding(stored);
    const flags = [b.ctrl, b.alt, b.shift, b.meta];
    const mods = MODIFIERS.filter((_, i) => flags[i]);
    return [...mods, codeLabel(b.code)].join("+");
  }
  return codeLabel(stored);
}

function codeLabel(code: string): string {
  const map: Record<string, string> = {
    Space: "Space", CapsLock: "Caps Lock", Backquote: "~", Tab: "Tab",
    Escape: "Esc", ShiftLeft: "Left Shift", ShiftRight: "Right Shift",
    ControlLeft: "Left Ctrl", ControlRight: "Right Ctrl",
    AltLeft: "Left Alt", AltRight: "Right Alt",
  };
  if (map[code]) return map[code];
  const letterMatch = code.match(/^Key([A-Z])$/);
  if (letterMatch) return letterMatch[1];
  const digitMatch = code.match(/^Digit(\d)$/);
  if (digitMatch) return digitMatch[1];
  const fnMatch = code.match(/^(F\d{1,2})$/);
  if (fnMatch) return fnMatch[1];
  const numpadMatch = code.match(/^Numpad(\d)$/);
  if (numpadMatch) return `Numpad ${numpadMatch[1]}`;
  return code;
}

/** Map a browser `event.code` to a Tauri accelerator string, or `null`. */
export function mapCodeToTauriShortcut(code: string): string | null {
  const map: Record<string, string> = {
    Space: "Space", CapsLock: "CapsLock", Backquote: "`", Tab: "Tab",
  };
  if (map[code]) return map[code];
  const letterMatch = code.match(/^Key([A-Z])$/);
  if (letterMatch) return letterMatch[1];
  const digitMatch = code.match(/^Digit(\d)$/);
  if (digitMatch) return digitMatch[1];
  const fnMatch = code.match(/^(F\d{1,2})$/);
  if (fnMatch) return fnMatch[1];
  const numpadMatch = code.match(/^Numpad(\d)$/);
  if (numpadMatch) return `Num${numpadMatch[1]}`;
  console.warn(`[PTT] Cannot map key code "${code}" to Tauri shortcut`);
  return null;
}

/** Map a binding to a Tauri accelerator such as `Control+V`, or `null`. */
export function bindingToTauriShortcut(b: KeyBinding): string | null {
  const key = mapCodeToTauriShortcut(b.code);
  if (!key) return null;
  const mods: string[] = [];
  if (b.ctrl) mods.push("Control");
  if (b.alt) mods.push("Alt");
  if (b.shift) mods.push("Shift");
  if (b.meta) mods.push("Super");
  return [...mods, key].join("+");
}

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

export class PttController {
  private readonly setMute: (muted: boolean) => void | Promise<void>;

  private config: PttFullConfig | null = null;
  private pttHeld = false;
  private ptmHeld = false;
  private releaseTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(setMuteFn: (muted: boolean) => void | Promise<void>) {
    this.setMute = setMuteFn;
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  /** Register config and apply the initial mute state. */
  activate(config: PttFullConfig): void {
    this.config = config;
    this.pttHeld = false;
    this.ptmHeld = false;
    this.clearTimer();
    this.applyState();
  }

  /** Clear all state and cancel pending timers. */
  deactivate(): void {
    this.config = null;
    this.pttHeld = false;
    this.ptmHeld = false;
    this.clearTimer();
  }

  // -----------------------------------------------------------------------
  // Key events
  // -----------------------------------------------------------------------

  /** Track a key press. Ignores repeats and unrelated keys. */
  handleKeyDown(code: string): void {
    if (!this.config) return;

    const role = this.roleForKey(code);
    if (role === null) return;

    if (role === "ptt") {
      if (this.pttHeld) return; // ignore repeat
      this.pttHeld = true;
    } else {
      if (this.ptmHeld) return; // ignore repeat
      this.ptmHeld = true;
    }

    // Key press cancels any pending release-delay timer and applies
    // the new state immediately.
    this.clearTimer();
    this.applyState();
  }

  /** Start a release-delay timer for the given key. */
  handleKeyUp(code: string): void {
    if (!this.config) return;

    const role = this.roleForKey(code);
    if (role === null) return;

    // Ignore duplicate release (e.g. both browser and Tauri fire keyup)
    if (role === "ptt" && !this.pttHeld) return;
    if (role === "ptm" && !this.ptmHeld) return;

    const delay =
      role === "ptt"
        ? this.config.pttReleaseDelay
        : this.config.ptmReleaseDelay;

    // Mark the key as released, but defer the state change by the
    // configured release delay so the user's last syllable isn't clipped.
    if (role === "ptt") {
      this.pttHeld = false;
    } else {
      this.ptmHeld = false;
    }

    this.clearTimer();
    this.releaseTimer = setTimeout(() => {
      this.releaseTimer = null;
      this.applyState();
    }, delay);
  }

  /** Immediately release all held keys (e.g. on window blur). */
  releaseAll(): void {
    if (!this.config) return;
    this.pttHeld = false;
    this.ptmHeld = false;
    this.clearTimer();
    this.applyState();
  }

  // -----------------------------------------------------------------------
  // Queries
  // -----------------------------------------------------------------------

  /** The binding whose main key is `code`, if it is a PTT/PTM key. */
  bindingForCode(code: string): KeyBinding | null {
    if (!this.config) return null;
    const role = this.roleForKey(code);
    if (role === null) return null;
    const stored = role === "ptt" ? this.config.pttKey : this.config.ptmKey;
    return stored ? parseKeyBinding(stored) : null;
  }

  /** Whether the controller has an active config. */
  isActive(): boolean {
    return this.config !== null;
  }

  /** Whether PTT or PTM is enabled in the current config. */
  isPttOrPtmEnabled(): boolean {
    if (!this.config) return false;
    return this.config.pttEnabled || this.config.ptmEnabled;
  }

  // -----------------------------------------------------------------------
  // Internal helpers
  // -----------------------------------------------------------------------

  private roleForKey(code: string): "ptt" | "ptm" | null {
    if (!this.config) return null;
    if (this.config.pttEnabled && this.config.pttKey && parseKeyBinding(this.config.pttKey).code === code) return "ptt";
    if (this.config.ptmEnabled && this.config.ptmKey && parseKeyBinding(this.config.ptmKey).code === code) return "ptm";
    return null;
  }

  private applyState(): void {
    if (!this.config) return;
    const muted = resolveState(this.config, this.pttHeld, this.ptmHeld);
    this.setMute(muted);
  }

  private clearTimer(): void {
    if (this.releaseTimer !== null) {
      clearTimeout(this.releaseTimer);
      this.releaseTimer = null;
    }
  }
}

// ---------------------------------------------------------------------------
// Browser event listeners
// ---------------------------------------------------------------------------

/**
 * Wire browser `keydown` / `keyup` / `blur` events to a {@link PttController}.
 * These only fire while Kaiku is focused. A keydown in a text field is
 * ignored so typing never transmits; keyup is always handled so a held key
 * is released. Returns a cleanup function that removes all listeners.
 */
export function createBrowserPttListeners(controller: PttController): () => void {
  const onKeyDown = (e: KeyboardEvent) => {
    if (isTextEntryTarget(e.target)) return;
    const binding = controller.bindingForCode(e.code);
    if (!binding || !modifiersHeld(binding, e)) return;
    controller.handleKeyDown(e.code);
  };
  const onKeyUp = (e: KeyboardEvent) => { controller.handleKeyUp(e.code); };
  const onBlur = () => { controller.releaseAll(); };

  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("blur", onBlur);

  return () => {
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("keyup", onKeyUp);
    window.removeEventListener("blur", onBlur);
  };
}

// ---------------------------------------------------------------------------
// Tauri global-shortcut listeners
// ---------------------------------------------------------------------------

let fallbackNoticeShown = false;

/** Allow the focus-only notice to show again (tests). */
export function resetPttFallbackNotice(): void {
  fallbackNoticeShown = false;
}

/** Tell the user once per session that a plain key only works while focused. */
function showFocusOnlyNotice(): void {
  if (fallbackNoticeShown) return;
  fallbackNoticeShown = true;
  showToast({
    type: "info",
    id: "ptt-focus-only",
    title: t("voice.ptt.focusOnlyTitle"),
    message: t("voice.ptt.focusOnly"),
    duration: 10000,
  });
}

/**
 * Register Tauri global shortcuts for PTT/PTM keys (works even when the
 * window is not focused) and fall back to browser-only listeners when
 * running outside of Tauri or when registration fails.
 *
 * A plain key (see {@link checkGlobalPttBinding}) is never registered
 * globally; it keeps working only while Kaiku is focused, and the user is
 * told once.
 */
export async function createTauriPttListeners(
  controller: PttController,
  config: PttFullConfig,
): Promise<() => void> {
  const cleanupBrowser = createBrowserPttListeners(controller);
  if (!isTauri()) return cleanupBrowser;

  const wanted: string[] = [];
  if (config.pttEnabled && config.pttKey) wanted.push(config.pttKey);
  if (config.ptmEnabled && config.ptmKey) wanted.push(config.ptmKey);

  const global: KeyBinding[] = [];
  for (const stored of wanted) {
    const binding = parseKeyBinding(stored);
    if (checkGlobalPttBinding(binding).ok) global.push(binding);
    else showFocusOnlyNotice();
  }
  if (global.length === 0) return cleanupBrowser;

  const registeredKeys: string[] = [];
  let unregisterFn: ((key: string) => Promise<void>) | null = null;
  const unregisterAll = async () => {
    for (const key of registeredKeys.splice(0)) {
      try { await unregisterFn?.(key); } catch { /* ignore cleanup errors */ }
    }
  };

  try {
    const { register, unregister } = await import("@tauri-apps/plugin-global-shortcut");
    unregisterFn = unregister;

    for (const binding of global) {
      const tauriKey = bindingToTauriShortcut(binding);
      if (!tauriKey) continue;
      await register(tauriKey, (event) => {
        if (event.state === "Pressed") {
          // While Kaiku is focused and the user types in a text field, the
          // shortcut must not transmit (the browser listener applies the
          // same rule for focused key presses).
          if (document.hasFocus() && isTextEntryTarget(document.activeElement)) return;
          controller.handleKeyDown(binding.code);
        }
        if (event.state === "Released") controller.handleKeyUp(binding.code);
      });
      registeredKeys.push(tauriKey);
    }

    return async () => {
      cleanupBrowser();
      await unregisterAll();
    };
  } catch (err) {
    console.warn("[PTT] Failed to register global shortcuts, using browser-only:", err);
    await unregisterAll();
    return cleanupBrowser;
  }
}
