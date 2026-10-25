/**
 * Connection supervisor: reconnect backoff, watchdog and wake/online triggers
 * for the realtime connection.
 *
 * Transport-agnostic: the actual socket lives behind a {@link ConnectionDriver}.
 * - `mode: "managed"` (browser): the supervisor schedules retries itself.
 * - `mode: "external"` (native, Rust retries, pings and watches for silence):
 *   the supervisor only maps status, runs `onRecovered`, and turns
 *   `triggerNow` into `driver.reconnectNow()`.
 */

export type ConnectionState =
  | "disconnected"
  | "connecting"
  | "connected"
  | "reconnecting";

export interface ConnectionDriver {
  /** Open the connection; resolves once it is usable, rejects on failure. */
  open(): Promise<void>;
  /** Drop the current connection. The supervisor decides what happens next. */
  close(): void;
  /** Send an application-level ping so the server answers with a frame. */
  sendPing(): void;
  /** External mode: ask the owner of the retry loop to try right now. */
  reconnectNow?(): Promise<void>;
}

export interface SupervisorOptions {
  driver: ConnectionDriver;
  mode: "managed" | "external";
  onStatus(status: ConnectionState, attempt: number): void;
  /** Runs after a connection came back (not after the very first open). */
  onRecovered(): Promise<void>;
  /**
   * Resolves `false` when the session cannot be renewed. `force` asks for a
   * real refresh even if the current token still looks valid (the server just
   * refused it).
   */
  ensureFreshToken(force?: boolean): Promise<boolean>;
  /**
   * Managed mode: the server refused the token again right after a refresh,
   * so reconnecting cannot help. The supervisor has already stopped.
   */
  onAuthRejected?(): void;
  timings?: {
    pingEveryMs: number;
    silenceLimitMs: number;
    /** A connection this long up resets the one-refresh-per-streak allowance. */
    healthyAfterMs?: number;
  };
}

export interface ConnectionSupervisor {
  /**
   * Open the connection. Resolves `true` when this call opened it, `false`
   * when the supervisor was already running (opening, open or waiting to
   * retry) or the run was cancelled. Rejects when the first open fails (a
   * retry is then scheduled).
   */
  start(): Promise<boolean>;
  stop(): void;
  notifyOpened(): void;
  /** `code` is the WebSocket close code of an unintentional close, if known. */
  notifyClosed(intentional: boolean, code?: number): void;
  notifyFrame(): void;
  triggerNow(reason: "online" | "visible" | "user"): void;
}

const DEFAULT_TIMINGS: NonNullable<SupervisorOptions["timings"]> = {
  pingEveryMs: 20_000,
  silenceLimitMs: 45_000,
};
/** Same as the native client: a connection this long up counts as healthy. */
const HEALTHY_AFTER_MS = 60_000;
/** WebSocket close code "policy violation": the server refused the auth. */
const CLOSE_AUTH = 1008;

/** Close code carried by a rejected open (`closeCode`), if the socket closed. */
function closeCodeOf(err: unknown): number | undefined {
  const code = (err as { closeCode?: unknown } | null)?.closeCode;
  return typeof code === "number" ? code : undefined;
}
const BASE_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 30_000;
const JITTER = 0.4; // total spread, i.e. ±20 %

/**
 * Delay before retry number `attempt` (1-based): exponential from 1 s,
 * capped at 30 s, with ±20 % jitter so many clients don't retry in lockstep.
 */
export function nextBackoffMs(
  attempt: number,
  random: () => number = Math.random,
): number {
  const exponent = Math.max(attempt, 1) - 1;
  const base = Math.min(BASE_BACKOFF_MS * 2 ** exponent, MAX_BACKOFF_MS);
  return Math.round(base * (1 + (random() - 0.5) * JITTER));
}

/**
 * Internal lifecycle:
 * - idle: not started, or closed on purpose
 * - opening: an open attempt is in flight
 * - open: connected, watchdog running
 * - waiting: connection lost; a retry is scheduled (managed) or awaited (external)
 * - stopped: stop() was called
 */
type Phase = "idle" | "opening" | "open" | "waiting" | "stopped";

export function createConnectionSupervisor(
  opts: SupervisorOptions,
): ConnectionSupervisor {
  const timings = opts.timings ?? DEFAULT_TIMINGS;
  const { driver } = opts;

  let phase: Phase = "idle";
  let attempt = 0;
  // Bumped by stop()/start(): in-flight async work from an older run is ignored.
  let generation = 0;
  let lastFrameAt = 0;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let pingTimer: ReturnType<typeof setInterval> | null = null;
  let silenceTimer: ReturnType<typeof setTimeout> | null = null;
  let listening = false;
  // Managed mode, auth handling: at most one forced refresh per streak of
  // failures; a connection that stays up for `healthyAfterMs` ends the streak.
  let refreshedThisStreak = false;
  let refreshPending = false;
  let healthyTimer: ReturnType<typeof setTimeout> | null = null;

  // --- timers -------------------------------------------------------------

  function clearRetry(): void {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }

  function stopWatchdog(): void {
    if (pingTimer !== null) {
      clearInterval(pingTimer);
      pingTimer = null;
    }
    if (silenceTimer !== null) {
      clearTimeout(silenceTimer);
      silenceTimer = null;
    }
    if (healthyTimer !== null) {
      clearTimeout(healthyTimer);
      healthyTimer = null;
    }
  }

  function startWatchdog(): void {
    stopWatchdog();
    // External mode: the owner of the connection pings and detects silence.
    if (opts.mode === "external") return;
    lastFrameAt = Date.now();
    pingTimer = setInterval(() => {
      if (phase === "open") driver.sendPing();
    }, timings.pingEveryMs);
    armSilenceCheck(timings.silenceLimitMs);
  }

  // One timer per silence window instead of re-arming on every frame:
  // when it fires early (frames arrived meanwhile) it re-arms for the rest.
  function armSilenceCheck(delayMs: number): void {
    silenceTimer = setTimeout(() => {
      silenceTimer = null;
      if (phase !== "open") return;
      const silentFor = Date.now() - lastFrameAt;
      if (silentFor >= timings.silenceLimitMs) {
        console.warn(
          `[WS] No frame for ${Math.round(silentFor / 1000)}s, dropping connection`,
        );
        // Mark the loss first so the close notification from the driver is ignored.
        connectionLost();
        driver.close();
      } else {
        armSilenceCheck(timings.silenceLimitMs - silentFor);
      }
    }, delayMs);
  }

  // --- state transitions --------------------------------------------------

  /**
   * `code`: close code of the lost socket. `beforeOpen`: it never opened, so
   * a refused handshake (invisible to the browser, close 1006) is possible.
   */
  function connectionLost(code?: number, beforeOpen = false): void {
    stopWatchdog();
    if (opts.mode === "managed") {
      const authClose = code === CLOSE_AUTH;
      const maybeAuth = beforeOpen && code !== undefined;
      if (authClose || maybeAuth) {
        if (!refreshedThisStreak) {
          refreshedThisStreak = true;
          refreshPending = true;
        } else if (authClose) {
          stop();
          opts.onAuthRejected?.();
          return;
        }
      }
    }
    phase = "waiting";
    attempt += 1;
    opts.onStatus("reconnecting", attempt);
    if (opts.mode === "managed") {
      const gen = generation;
      retryTimer = setTimeout(() => {
        retryTimer = null;
        if (gen !== generation) return;
        attemptOpen().catch(() => {
          // Failure already scheduled the next retry.
        });
      }, nextBackoffMs(attempt));
    }
  }

  async function opened(): Promise<void> {
    clearRetry();
    const recovered = attempt > 0;
    phase = "open";
    attempt = 0;
    opts.onStatus("connected", 0);
    startWatchdog();
    if (opts.mode === "managed") {
      healthyTimer = setTimeout(() => {
        healthyTimer = null;
        refreshedThisStreak = false;
      }, timings.healthyAfterMs ?? HEALTHY_AFTER_MS);
    }
    if (recovered) {
      try {
        await opts.onRecovered();
      } catch (err) {
        console.error("[WS] Reconnect recovery failed:", err);
      }
    }
  }

  /** Resolves `true` only when this attempt ended in an open connection. */
  async function attemptOpen(): Promise<boolean> {
    clearRetry();
    const gen = generation;
    phase = "opening";
    if (attempt === 0) opts.onStatus("connecting", 0);

    if (attempt > 0) {
      let fresh: boolean;
      const force = refreshPending;
      refreshPending = false;
      try {
        fresh = await opts.ensureFreshToken(force);
      } catch (err) {
        // Treat as transient (e.g. network): back off and try again.
        console.warn("[WS] Token check failed before reconnect:", err);
        if (gen === generation) connectionLost();
        throw err;
      }
      if (gen !== generation) return false;
      if (!fresh) {
        stop();
        window.dispatchEvent(new CustomEvent("kaiku:session-expired"));
        return false;
      }
    }

    // External mode: the owner's status events (notifyOpened / notifyClosed)
    // are authoritative and may describe this attempt, and later ones, before
    // open() settles. A settlement that finds the phase moved on is stale and
    // must not overwrite the newer state; it still tells the caller how the
    // first attempt went.
    try {
      await driver.open();
    } catch (err) {
      if (gen === generation && phase === "opening") {
        connectionLost(closeCodeOf(err), true);
      }
      throw err;
    }

    if (gen !== generation) {
      // Stopped while the open was in flight. Managed: don't keep a stray
      // socket. External: the owner has one connection, which the stop or the
      // next open already replaced; closing now would kill that one.
      if (opts.mode === "managed") driver.close();
      return false;
    }
    if (phase !== "opening") return phase === "open";
    await opened();
    return true;
  }

  // --- wake / network triggers -------------------------------------------

  const onOnline = () => triggerNow("online");
  const onVisibilityChange = () => {
    if (document.visibilityState !== "visible") return;
    if (phase === "open") driver.sendPing();
    else triggerNow("visible");
  };

  function addListeners(): void {
    if (listening || typeof window === "undefined") return;
    window.addEventListener("online", onOnline);
    document.addEventListener("visibilitychange", onVisibilityChange);
    listening = true;
  }

  function removeListeners(): void {
    if (!listening) return;
    window.removeEventListener("online", onOnline);
    document.removeEventListener("visibilitychange", onVisibilityChange);
    listening = false;
  }

  // --- public API ---------------------------------------------------------

  async function start(): Promise<boolean> {
    if (phase === "opening" || phase === "open" || phase === "waiting") {
      return false;
    }
    generation += 1;
    attempt = 0;
    refreshedThisStreak = false;
    refreshPending = false;
    addListeners();
    return attemptOpen();
  }

  function stop(): void {
    const wasActive = phase !== "idle" && phase !== "stopped";
    generation += 1;
    clearRetry();
    stopWatchdog();
    removeListeners();
    phase = "stopped";
    attempt = 0;
    if (wasActive) opts.onStatus("disconnected", 0);
  }

  function notifyOpened(): void {
    if (phase === "open" || phase === "stopped" || phase === "idle") return;
    // Managed mode reports its own opens from attemptOpen().
    if (opts.mode === "managed" && phase === "opening") return;
    void opened();
  }

  function notifyClosed(intentional: boolean, code?: number): void {
    // Only a close of the live connection matters; failures of an attempt in
    // flight are handled by attemptOpen(), and a close after the watchdog
    // dropped the socket is already accounted for. External mode is the
    // exception: a loss reported while the first open is in flight means that
    // attempt failed and the owner is retrying.
    const firstAttemptFailed =
      opts.mode === "external" && phase === "opening" && !intentional;
    if (phase !== "open" && !firstAttemptFailed) return;
    if (intentional) {
      stopWatchdog();
      phase = "idle";
      attempt = 0;
      opts.onStatus("disconnected", 0);
      return;
    }
    connectionLost(code);
  }

  function notifyFrame(): void {
    lastFrameAt = Date.now();
  }

  function triggerNow(reason: "online" | "visible" | "user"): void {
    if (phase !== "waiting") return;
    if (opts.mode === "external") {
      driver.reconnectNow?.().catch((err) =>
        console.warn(`[WS] Reconnect (${reason}) failed:`, err),
      );
      return;
    }
    attemptOpen().catch(() => {
      // Failure already scheduled the next retry.
    });
  }

  return { start, stop, notifyOpened, notifyClosed, notifyFrame, triggerNow };
}
