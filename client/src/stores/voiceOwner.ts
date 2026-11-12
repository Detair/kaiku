/**
 * Voice owner: the one place that joins, leaves and recovers voice.
 *
 * There is one voice adapter and one native voice state per window, so
 * there is one owner. It keeps
 *
 * - the INTENT: the voice session the user wants right now (a "wish":
 *   channel voice or the voice of a DM call), or none. User actions (join,
 *   leave, hang up, start or accept a call), `voice_removed` and the
 *   owner's own give-up only replace the intent, synchronously;
 * - the HELD session: what actually exists in the adapter, by handle.
 *
 * A single runner brings "held" to "intent", strictly one step after the
 * other: a held session that is no longer wanted is ended completely
 * (`leave`/`teardown` with its own handle) before anything is joined, and
 * only the latest intent is ever joined. The runner re-reads the intent
 * after every await; work for an intent that was replaced is dropped, and
 * what it acquired is released by handle. A join that hangs (microphone
 * prompt) does not hold the runner up: its session is ended by handle, the
 * adapter stops the late grant when the prompt resolves, and the runner
 * goes on at once.
 *
 * Recovery (C32 with the amendments) is a phase of the held session, run
 * by the same runner, never a second flow:
 *
 * - at most three attempts, spaced 1 s, 3 s and 9 s; an attempt drops the
 *   old connection without `voice_leave`, joins again and has succeeded
 *   when the publisher is connected; after 15 s it has failed;
 * - a socket loss during an attempt ends it without charging it; the next
 *   authenticated socket resumes with the remaining budget;
 * - the 60 s deadline measures the time without an authenticated socket
 *   that CARRIES this session's `voice_join`: it starts when the socket
 *   that carried the join is lost and is met only when a rejoin's
 *   `voice_join` went out. A socket that merely comes back does not reset
 *   it. Only a session that is NOT carried is ever measured: a carried
 *   one is never ended by a clock gap, a heartbeat gap or held-back timers.
 *   The time is real elapsed time including time the device was suspended,
 *   the larger of the wall-clock and the monotonic difference since the
 *   last proof that the session was carried. "Carried" is proven by an
 *   observation, never assumed from remembered state: while the owner runs
 *   without a gap, its heartbeat renews the proof as long as no loss was
 *   reported; after any gap in its own activity (the process did not run:
 *   suspension, throttling) it does NOT renew the proof from what it
 *   remembers but verifies: the publisher connection is read from the
 *   object itself and the socket is asked for a pong, and only a fresh
 *   inbound frame within 5 s together with a live publisher renews the
 *   proof. Otherwise the session is not carried since the last proof
 *   before the gap: more than 60 s ago means give up without a rejoin,
 *   less means normal recovery with the remaining time. While verifying,
 *   the session shows as reconnecting;
 * - a change of intent (leave, switch, hang-up, `voice_removed`) ends the
 *   recovery like any other work of a replaced intent.
 *
 * Hooks let the two kinds of session (channel voice in `voice.ts`, DM-call
 * voice in `call.ts`) do their part. Asynchronous hooks may only configure
 * the adapter (devices, mute, push-to-talk); what the user sees changes in
 * the synchronous hooks, which the owner calls only while that wish is
 * still the intent. The owner waits for a set-up or clean-up hook to
 * finish (so they normally never overlap with the next step), but for no
 * longer than `HOOK_LIMIT_MS`. Every asynchronous hook is handed `mine()`:
 * whether its session is still the one the owner holds. A hook works only
 * on what it created itself and checks `mine()` after each await before it
 * touches anything shared (mute, devices, what the user sees).
 *
 * The end of the signed-in session (`shutdown()`) ends everything at
 * once: no intent, the held session released synchronously, nothing left
 * that a later account could see or that could rejoin.
 */

import type { VoiceAdapter, VoiceConnectionState, VoiceError } from "@/lib/webrtc/types";
import {
  isJoinCancelled,
  isSameSession,
  nextVoiceToken,
  type VoiceSessionHandle,
} from "@/lib/webrtc/voiceSession";

/** Wait before each recovery attempt (C32). */
export const RECOVERY_DELAYS_MS = [1000, 3000, 9000] as const;
/** An attempt fails when the publisher is not connected after this long. */
export const RECOVERY_ATTEMPT_TIMEOUT_MS = 15_000;
/** Give up when no socket carried the rejoin this long after the loss. */
export const RECOVERY_OFFLINE_CAP_MS = 60_000;
/**
 * Longest the owner waits for a set-up or clean-up hook (devices,
 * push-to-talk, a settings write). They take milliseconds; one that hangs
 * must not keep a leave or the next join waiting.
 */
export const HOOK_LIMIT_MS = 3000;
/**
 * Longest the owner waits for the adapter to end a session. Ending
 * releases the local side at once and completes by itself (natively in
 * the background, in order with later joins); the owner only stops waiting.
 */
export const TEARDOWN_LIMIT_MS = 5000;
/** How often the owner notes that the session is still carried. */
export const HEARTBEAT_MS = 5000;
/**
 * A gap between two wake-ups of the owner longer than its tick plus this
 * means the process did not run in between (suspension, throttling).
 */
export const GAP_SLACK_MS = 5000;
/** How long the owner waits for a fresh inbound frame when it verifies. */
export const PROBE_LIMIT_MS = 5000;

export type VoiceKind = "channel" | "dm-call";

/** What a kind of voice session adds to the owner's work. */
export interface VoiceHooks {
  /**
   * The join may start once this resolves (DM call: the server accepted
   * the start or the accept). A rejection fails the wish.
   */
  ready?: Promise<unknown>;
  /** Recovery waits for `confirm()` after each socket change (C33). */
  needsConfirmation?: boolean;
  /** Before the first join: hand settings to the adapter. */
  prepare?(adapter: VoiceAdapter, mine: StillMine): Promise<void>;
  /** The first join succeeded: configure the adapter for the call. */
  established?(
    adapter: VoiceAdapter,
    session: VoiceSessionHandle,
    mine: StillMine,
  ): Promise<void>;
  /** The session is the user's call now (after `established`). */
  live?(adapter: VoiceAdapter, session: VoiceSessionHandle): void;
  /** Media state of the held connection (not while recovering). */
  media?(state: VoiceConnectionState): void;
  /** Recovery began (socket loss, ICE failure). */
  recovering?(): void;
  /** A recovery attempt dropped the old connection and joins again. */
  attemptStarts?(adapter: VoiceAdapter): void;
  /** A recovery attempt's `voice_join` went out. */
  rejoined?(
    adapter: VoiceAdapter,
    session: VoiceSessionHandle,
    mine: StillMine,
  ): Promise<void>;
  /** A recovery attempt connected: restore mute, deafen, devices. */
  restore?(
    adapter: VoiceAdapter,
    session: VoiceSessionHandle,
    mine: StillMine,
  ): Promise<void>;
  /** Recovery succeeded. */
  recovered?(adapter: VoiceAdapter, session: VoiceSessionHandle): void;
  /** After a gap the owner checks whether the session is still carried. */
  verifying?(): void;
  /** That check found the session carried: it goes on as it was. */
  verified?(): void;
  /** The first join failed; the wish is over. */
  failed?(error: VoiceError): void;
  /** Recovery gave up (third failure or the 60 s deadline); the wish is over. */
  gaveUp?(): void;
  /** Another wish replaced this one. */
  superseded?(): void;
  /**
   * What was held for this wish has been ended in the adapter. `mine()`
   * turns false once the owner holds something newer.
   */
  released?(adapter: VoiceAdapter | null, mine: StillMine): Promise<void>;
}

/**
 * Whether the session a hook was started for is still the one the owner
 * holds. Hooks ask it after every await before touching shared state.
 */
export type StillMine = () => boolean;

/** How a wish ended up. */
export type VoiceOutcome =
  | { type: "established" }
  | { type: "failed"; error: VoiceError }
  /** Replaced or dropped before it was established. */
  | { type: "superseded" };

/** One voice session the user asked for. */
export interface VoiceWish {
  readonly kind: VoiceKind;
  /** Channel (or DM channel) id. */
  readonly id: string;
  /** Token of the user action; the first join's session carries it. */
  readonly token: number;
  readonly hooks: VoiceHooks;
}

class Wish implements VoiceWish {
  /** Who was signed in when the wish was requested. */
  user: string | null = null;
  readonly outcome: Promise<VoiceOutcome>;
  readonly released: Promise<void>;
  private resolveOutcome!: (outcome: VoiceOutcome) => void;
  private resolveReleased!: () => void;

  constructor(
    readonly kind: VoiceKind,
    readonly id: string,
    readonly token: number,
    readonly hooks: VoiceHooks,
  ) {
    this.outcome = new Promise((resolve) => {
      this.resolveOutcome = resolve;
    });
    this.released = new Promise((resolve) => {
      this.resolveReleased = resolve;
    });
  }

  /** The first outcome counts. */
  settle(outcome: VoiceOutcome): void {
    this.resolveOutcome(outcome);
  }

  settleReleased(): void {
    this.resolveReleased();
  }
}

type Phase = "joining" | "live" | "recovering";

/** What the adapter holds for a wish. Only the runner changes it. */
interface Held {
  readonly wish: Wish;
  /** The adapter session that exists for the wish right now, if any. */
  session: VoiceSessionHandle | null;
  phase: Phase;
  /** End it without `voice_leave` (server removed the peer, give-up). */
  silent: boolean;
  /** The publisher of `session` reported connected. */
  connected: boolean;
  /** Recovery attempts charged so far. */
  used: number;
  /** Recovery may run (see `VoiceHooks.needsConfirmation`). */
  confirmed: boolean;
  /**
   * Last moment the session was provably carried: its join went out, or a
   * heartbeat ran while the socket was up and the publisher connected.
   */
  aliveAt: Stamp;
  /**
   * The owner is checking, after a gap in its own activity, whether the
   * session is still carried. Until that is answered the proof is not
   * renewed.
   */
  verifying: boolean;
  /**
   * The 60 s deadline applies (the socket that carried the join is gone).
   * The timer only wakes the owner; the deadline itself is `aliveAt`.
   */
  offline: { timer: ReturnType<typeof setTimeout> } | null;
  /** An end of the session started outside the runner's turn (`releaseNow`). */
  ending: Promise<unknown> | null;
}

/** A moment on both clocks. */
export interface Stamp {
  /** Wall clock (`Date.now()`): moves on while the device sleeps. */
  wall: number;
  /** Monotonic clock (`performance.now()`): never steps back. */
  mono: number;
}

interface Tick {
  promise: Promise<void>;
  resolve: () => void;
}

function newTick(): Tick {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function toVoiceError(err: unknown): VoiceError {
  return { type: "unknown", message: err instanceof Error ? err.message : String(err) };
}

type AttemptResult = "connected" | "failed" | "interrupted";

export class VoiceOwner {
  /** The latest intent. Replaced synchronously, read by the runner. */
  private intent: Wish | null = null;
  /** What exists in the adapter. Written by the runner only. */
  private held: Held | null = null;
  /** An authenticated, re-subscribed socket is there. */
  private socketUp = true;
  private running = false;
  private tick = newTick();
  private adapterProvider: () => Promise<VoiceAdapter> = async () =>
    (await import("@/lib/webrtc")).createVoiceAdapter();
  /** The adapter, once the owner has used it (for a synchronous release). */
  private adapter: VoiceAdapter | null = null;
  /** Who is signed in (a wish is only ever served for its own user). */
  private identity: () => string | null = () => null;
  private shutdownListeners: Array<() => void> = [];
  private heartbeat: ReturnType<typeof setInterval> | null = null;
  private clock: () => Stamp = () => ({ wall: Date.now(), mono: performance.now() });
  /** Ask the server for a pong on the signalling socket (see `verify`). */
  private ping: () => Promise<void> = async () => (await import("@/lib/tauri")).wsPing();
  /** Inbound frames seen on the signalling socket so far. */
  private inbound = 0;
  /** The last heartbeat or wake-up check. */
  private lastBeat: Stamp = this.clock();

  // --- intent (synchronous) -------------------------------------------------

  /** A wish for a voice session; nothing happens until it is requested. */
  wish(kind: VoiceKind, id: string, hooks: VoiceHooks, token = nextVoiceToken()): VoiceWish {
    return new Wish(kind, id, token, hooks);
  }

  /**
   * The user wants `wish` now. Whatever was wanted before is over. Resolves
   * when the wish is established, failed, or was replaced before that.
   */
  request(wish: VoiceWish): Promise<VoiceOutcome> {
    const next = wish as Wish;
    const previous = this.intent;
    if (previous === next) return next.outcome;
    next.user = this.identity();
    this.intent = next;
    if (previous) this.retire(previous, false, true);
    this.kick();
    return next.outcome;
  }

  /**
   * `wish` is no longer wanted (leave, hang-up, removal). Only the wish
   * named here is dropped, never a newer one. `silent`: end its session
   * without `voice_leave`. Resolves when nothing of it is held any more.
   */
  drop(wish: VoiceWish | null, opts: { silent?: boolean } = {}): Promise<void> {
    const gone = wish as Wish | null;
    if (!gone) return Promise.resolve();
    if (this.intent === gone) {
      this.intent = null;
      this.retire(gone, opts.silent ?? false, false);
      this.kick();
    }
    return gone.released;
  }

  private retire(wish: Wish, silent: boolean, superseded: boolean): void {
    if (this.held?.wish === wish) {
      if (silent) this.held.silent = true;
    } else {
      wish.settleReleased(); // nothing is held for it
    }
    wish.settle({ type: "superseded" });
    if (superseded) this.tell(() => wish.hooks.superseded?.());
  }

  /** Run a synchronous hook; a hook that throws must not stop the owner. */
  private tell(hook: () => void): void {
    try {
      hook();
    } catch (err) {
      console.error("[VoiceOwner] A voice session hook failed:", err);
    }
  }

  /**
   * The signed-in session ends or is replaced (sign-out, expiry,
   * revocation, account switch, the window closes). Synchronously: nothing
   * is wanted any more, the held session is ended in the adapter (local
   * media released before this returns, `voice_leave` best effort), and
   * whoever registered with `onShutdown` clears what the user sees. No
   * intent, recovery or rejoin offer outlives it.
   */
  shutdown(): void {
    const wish = this.intent;
    this.intent = null;
    if (wish) this.retire(wish, false, false);
    if (this.held) this.releaseNow(this.held);
    for (const listener of this.shutdownListeners) this.tell(listener);
    this.kick();
  }

  /** Run `listener` at every `shutdown()` (stores clear their screen state). */
  onShutdown(listener: () => void): void {
    this.shutdownListeners.push(listener);
  }

  /** Who is signed in right now (null: nobody). */
  useIdentity(identity: () => string | null): void {
    this.identity = identity;
  }

  /**
   * End the held session's adapter session right now, outside the runner's
   * turn (sign-out, give-up): the adapter releases the local side before
   * its first await. The runner then finds nothing left to end, and waits
   * for this end before it joins anything.
   */
  private releaseNow(held: Held): void {
    const session = held.session;
    const adapter = this.adapter;
    if (!session || !adapter) return;
    held.session = null;
    held.connected = false;
    try {
      const ending = held.silent ? adapter.teardown(session) : adapter.leave(session);
      held.ending = Promise.resolve(ending).catch((err: unknown) => {
        console.warn("[VoiceOwner] Ending the voice session failed:", err);
      });
    } catch (err) {
      console.warn("[VoiceOwner] Ending the voice session failed:", err);
    }
  }

  /**
   * A wish of someone who is no longer signed in must not be served: end
   * it like a shutdown (second line of defence behind `shutdown()`).
   */
  private endIfForeign(): boolean {
    const wish = this.intent ?? this.held?.wish ?? null;
    if (!wish || wish.user === this.identity()) return false;
    console.warn("[VoiceOwner] The signed-in user changed: ending the voice session");
    this.shutdown();
    return true;
  }

  /**
   * The server removed this socket's peer from `channelId` (C31): a session
   * held for that channel is ended without `voice_leave`, whatever is wanted
   * by now. The caller drops its wish itself.
   */
  serverRemoved(channelId: string): void {
    if (this.held?.wish.id === channelId) this.held.silent = true;
  }

  // --- signalling socket ----------------------------------------------------

  /** The signalling socket dropped. */
  socketLost(): void {
    if (!this.socketUp) return;
    this.socketUp = false;
    if (this.endIfForeign()) return;
    const held = this.held;
    if (held && held.wish === this.intent && held.phase !== "joining") {
      // The socket that carried this session's join is gone. Reported
      // right away (the heartbeat is recent) it carried the session until
      // now; reported after a silence (the device slept) nobody knows when
      // it went, and the last heartbeat stays the last proof.
      if (
        held.phase === "live" &&
        held.connected &&
        !held.verifying &&
        this.since(held.aliveAt) <= 2 * HEARTBEAT_MS
      ) {
        held.aliveAt = this.clock();
      }
      held.verifying = false; // answered: it is not carried
      this.armOffline(held);
      if (held.wish.hooks.needsConfirmation) held.confirmed = false;
      this.startRecovery(held);
      this.checkDeadline(held);
    }
    this.kick();
  }

  /**
   * The socket is open but not yet re-subscribed. Counts as up unless a
   * recovery waits for the re-subscribed socket (`socketReady`).
   */
  socketOpen(): void {
    if (this.held?.phase === "recovering") return;
    this.socketUp = true;
    this.kick();
  }

  /**
   * An authenticated socket is back and re-subscribed (C32 trigger). The
   * server dropped the peer with the old socket, so an established session
   * is recovered.
   */
  socketReady(): void {
    this.socketUp = true;
    if (this.endIfForeign()) return;
    const held = this.held;
    if (held && held.wish === this.intent && held.phase === "live") {
      // The socket that carried this session's join is gone, even if its
      // loss was never reported: the deadline runs since the last proof.
      this.armOffline(held);
      if (held.wish.hooks.needsConfirmation) held.confirmed = false;
      this.startRecovery(held);
    }
    if (held && held.wish === this.intent) this.checkDeadline(held);
    this.kick();
  }

  /**
   * Compare the clocks: called by the heartbeat, and by whoever notices
   * that the device may have been away (the page becomes visible, the
   * network is back). Timers do not run while a device sleeps.
   *
   * Without a gap since the last wake-up the proof is renewed as long as
   * nothing was reported lost. After a gap nothing remembered counts: the
   * owner verifies (`verify`). A session that is carried is never ended
   * here.
   */
  wake(): void {
    const now = this.clock();
    const gap = this.since(this.lastBeat);
    this.lastBeat = now;
    if (this.endIfForeign()) return;
    const held = this.held;
    if (!held || held.wish !== this.intent || held.phase === "joining") return;
    if (held.phase === "live") {
      if (gap > HEARTBEAT_MS + GAP_SLACK_MS) this.verify(held);
      else if (!held.verifying && this.socketUp && held.connected) held.aliveAt = now;
    } else if (gap >= RECOVERY_OFFLINE_CAP_MS) {
      // A recovery was under way when the device went to sleep.
      this.notCarried(held);
    }
    if (this.checkDeadline(held)) this.kick();
  }

  /**
   * After a gap in the owner's own activity: find out whether the session
   * is still carried, by observation. The publisher connection is read
   * from the object itself, the socket is asked for a pong; a fresh inbound
   * frame within `PROBE_LIMIT_MS` and a live publisher renew the proof.
   * Anything else means the session has not been carried since the last
   * proof before the gap: the 60 s rule decides between giving up and a
   * normal recovery with the time that is left.
   */
  private verify(held: Held): void {
    if (held.verifying) return;
    held.verifying = true;
    this.tell(() => held.wish.hooks.verifying?.());
    void this.observe(held).then((carried) => {
      // A loss reported meanwhile, or an end of the wish, already answered.
      if (this.held !== held || !held.verifying) return;
      held.verifying = false;
      if (held.wish !== this.intent || held.phase !== "live") return;
      if (carried && this.socketUp && held.connected) {
        held.aliveAt = this.clock();
        this.tell(() => held.wish.hooks.verified?.());
      } else {
        this.notCarried(held);
        this.checkDeadline(held);
      }
      this.kick();
    });
  }

  /** One observation of socket and publisher (see `verify`). Never throws. */
  private async observe(held: Held): Promise<boolean> {
    const session = held.session;
    const adapter = this.adapter;
    try {
      if (!session || !adapter || !this.socketUp || !held.connected) return false;
      // The publisher: the connection object's own state, where the adapter can read it.
      if (typeof adapter.probePublisher === "function") {
        if (!(await adapter.probePublisher(session))) return false;
      }
      // The socket: ask for a pong; only a frame that arrives now counts.
      const seen = this.inbound;
      const window = this.timer(PROBE_LIMIT_MS);
      void this.ping().catch(() => undefined);
      while (this.inbound === seen && !window.fired && held.verifying) await this.tick.promise;
      window.cancel();
      return this.inbound !== seen;
    } catch (err) {
      console.warn("[VoiceOwner] Checking the voice connections failed:", err);
      return false;
    }
  }

  /** A frame from the server arrived on the signalling socket (any frame). */
  noteInbound(): void {
    this.inbound += 1;
    if (this.held?.verifying) this.poke();
  }

  /** The session is not carried: the 60 s rule applies from the last proof. */
  private notCarried(held: Held): void {
    this.armOffline(held);
    this.startRecovery(held);
  }

  /** Real time elapsed since `stamp`: the larger of both clocks' differences. */
  private since(stamp: Stamp): number {
    const now = this.clock();
    return Math.max(now.wall - stamp.wall, now.mono - stamp.mono);
  }

  /** The clocks the owner reads (tests). */
  useClock(clock: () => Stamp): void {
    this.clock = clock;
    this.lastBeat = clock();
  }

  /** How to ask the server for a pong (tests). */
  usePing(ping: () => Promise<void>): void {
    this.ping = ping;
  }

  /** The recovery of `wish` may run on the current socket (C33 resync). */
  confirm(wish: VoiceWish | null): void {
    if (!wish || this.held?.wish !== wish) return;
    this.held.confirmed = true;
    this.kick();
  }

  // --- reading --------------------------------------------------------------

  /** Whether `wish` is the current intent. */
  wants(wish: VoiceWish | null): boolean {
    return !!wish && this.intent === wish;
  }

  /** The current intent. */
  get current(): VoiceWish | null {
    return this.intent;
  }

  /** The adapter session held for `wish` while it is the intent. */
  sessionOf(wish: VoiceWish | null): VoiceSessionHandle | null {
    const held = this.held;
    return held && wish && held.wish === wish && this.intent === wish ? held.session : null;
  }

  /** Whether the wanted session (or `wish`) is being recovered. */
  isRecovering(wish?: VoiceWish | null): boolean {
    const held = this.held;
    if (!held || held.phase !== "recovering" || held.wish !== this.intent) return false;
    return wish === undefined || held.wish === wish;
  }

  /** Attempts the running recovery has used up (tests). */
  get attemptsUsed(): number {
    return this.held?.phase === "recovering" ? this.held.used : 0;
  }

  /** Whether the socket is known to be down. */
  get signallingDown(): boolean {
    return !this.socketUp;
  }

  /** Resolves once no recovery is running (success, give-up or intent change). */
  async whenCalm(): Promise<void> {
    while (this.held?.phase === "recovering") await this.tick.promise;
  }

  /** Where the adapter comes from (tests). */
  useAdapter(provider: () => Promise<VoiceAdapter>): void {
    this.adapterProvider = provider;
    this.adapter = null;
  }

  private async getAdapter(): Promise<VoiceAdapter> {
    const adapter = await this.adapterProvider();
    this.adapter = adapter;
    return adapter;
  }

  // --- adapter events -------------------------------------------------------

  private listen(adapter: VoiceAdapter): void {
    adapter.setEventHandlers({
      onStateChange: (state, session) => this.mediaState(state, session),
      onConnectionLost: (session) => this.mediaLost(session),
    });
  }

  private mediaState(state: VoiceConnectionState, session: VoiceSessionHandle | null): void {
    const held = this.held;
    // A state of a session the owner does not hold (an ended one) is nothing.
    if (!held || !isSameSession(session, held.session)) return;
    held.connected = state === "connected";
    if (held.wish === this.intent && held.phase !== "recovering") {
      this.tell(() => held.wish.hooks.media?.(state));
    }
    if (!held.connected && held.verifying && held.phase === "live" && held.wish === this.intent) {
      // Being verified after a gap, and the publisher is not connected:
      // not carried since the last proof.
      held.verifying = false;
      this.notCarried(held);
      this.checkDeadline(held);
    }
    this.kick();
  }

  private mediaLost(session: VoiceSessionHandle | null): void {
    // The first thing noticed after a sleep may be the media failing.
    this.checkClock();
    const held = this.held;
    if (!held || !isSameSession(session, held.session) || held.wish !== this.intent) return;
    held.connected = false;
    if (held.verifying) {
      held.verifying = false;
      this.armOffline(held);
    }
    this.startRecovery(held);
    this.checkDeadline(held);
    this.kick();
  }

  /** If no heartbeat ran for a minute or more, look at the clocks now: see `wake()`. */
  private checkClock(): void {
    if (this.heartbeat && this.since(this.lastBeat) >= RECOVERY_OFFLINE_CAP_MS) this.wake();
  }

  // --- the runner -----------------------------------------------------------

  /** Something changed: wake the runner, start it if it is not running. */
  private kick(): void {
    this.poke();
    if (this.running) return;
    this.running = true;
    void this.runner();
  }

  /** Wake whoever waits for the next change. */
  private poke(): void {
    const wake = this.tick.resolve;
    this.tick = newTick();
    wake();
  }

  private async runner(): Promise<void> {
    let crashed = false;
    try {
      await this.run();
    } catch (err) {
      crashed = true;
      console.error("[VoiceOwner] Runner failed:", err);
    }
    this.running = false;
    this.poke();
    // An event that arrived while the runner was returning.
    if (!crashed && (this.intent || this.held)) this.kick();
  }

  private async run(): Promise<void> {
    for (;;) {
      this.poke();
      this.endIfForeign();
      this.checkClock();
      const held = this.held;
      if (held && held.wish !== this.intent) {
        // Not wanted any more: end it completely before anything else.
        await this.release(held);
        continue;
      }
      const wish = this.intent;
      if (!wish) return;
      if (!held) {
        await this.establish(wish);
        continue;
      }
      if (held.phase === "recovering") {
        await this.recover(held);
        continue;
      }
      await this.tick.promise;
    }
  }

  /**
   * Wait for `work`, but no longer than until `stop()` holds. The work is
   * not cancelled here: whoever stops early ends its session by handle.
   */
  private async until<T>(
    work: Promise<T>,
    stop: () => boolean,
  ): Promise<{ done: true; value: T } | { done: false }> {
    const box: { result?: { value: T } | { error: unknown } } = {};
    work.then(
      (value) => {
        box.result = { value };
        this.poke();
      },
      (error: unknown) => {
        box.result = { error };
        this.poke();
      },
    );
    while (!box.result && !stop()) await this.tick.promise;
    const result = box.result;
    if (!result) return { done: false };
    if ("error" in result) throw result.error;
    return { done: true, value: result.value };
  }

  /**
   * Wait for `work`, at most `ms`; afterwards go on without it (logged
   * once). Used for hooks, which only configure and are bound to their
   * session, and for the adapter's end of a session, which has released
   * the local side before its first await and completes by itself.
   */
  private async limited<T>(what: string, work: Promise<T>, ms: number): Promise<T | undefined> {
    let limit: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<undefined>((resolve) => {
      limit = setTimeout(() => {
        console.warn(`[VoiceOwner] ${what} did not finish within ${ms} ms; going on without it`);
        resolve(undefined);
      }, ms);
    });
    try {
      return await Promise.race([work, late]);
    } finally {
      clearTimeout(limit);
    }
  }

  /** Wait for a set-up or clean-up hook, at most `HOOK_LIMIT_MS`. */
  private async bounded(what: string, work: Promise<void> | undefined): Promise<void> {
    if (work) await this.limited(what, work, HOOK_LIMIT_MS);
  }

  private timer(ms: number): { fired: boolean; cancel: () => void } {
    const timer = {
      fired: false,
      cancel: () => clearTimeout(id),
    };
    const id = setTimeout(() => {
      timer.fired = true;
      this.poke();
    }, ms);
    return timer;
  }

  /** Join the wanted session for the first time. */
  private async establish(wish: Wish): Promise<void> {
    const held: Held = {
      wish,
      session: null,
      phase: "joining",
      silent: false,
      connected: false,
      used: 0,
      confirmed: true,
      aliveAt: this.clock(),
      verifying: false,
      offline: null,
      ending: null,
    };
    this.held = held;
    const hooks = wish.hooks;
    const gone = () => this.intent !== wish;
    /** For this wish's hooks: its session is still what the owner holds and wants. */
    const mine = () => this.held === held && this.intent === wish;

    let adapter: VoiceAdapter;
    try {
      if (hooks.ready) {
        const ready = await this.until(hooks.ready, gone);
        if (!ready.done || gone()) return;
      }
      adapter = await this.getAdapter();
      if (gone()) return;
      this.listen(adapter);
      await this.bounded("Preparing the join", hooks.prepare?.(adapter, mine));
      if (gone()) return;
    } catch (err) {
      if (gone()) return;
      this.fail(held, toVoiceError(err));
      return;
    }

    // From here on a session exists under this handle; whoever ends this
    // wish ends it by this handle, also while the join is still waiting.
    const session: VoiceSessionHandle = { token: wish.token, channelId: wish.id };
    held.session = session;
    const joined = await this.until(
      adapter.join(session).catch((err: unknown) => ({ ok: false as const, error: toVoiceError(err) })),
      gone,
    );
    if (!joined.done || gone()) return;
    if (!joined.value.ok) {
      await this.endSession(adapter, held);
      if (gone()) return;
      this.fail(held, joined.value.error);
      return;
    }

    held.phase = "live";
    held.aliveAt = this.clock(); // its `voice_join` went out just now
    this.startHeartbeat();
    try {
      // Wish-level `mine`: what `established` sets up (push-to-talk) stays
      // with the wish across recovery attempts.
      await this.bounded("Setting up the call", hooks.established?.(adapter, session, mine));
    } catch (err) {
      console.warn("[VoiceOwner] Setting up the call failed:", err);
    }
    if (gone()) return;
    this.tell(() => hooks.live?.(adapter, session));
    wish.settle({ type: "established" });
    if (!this.socketUp) {
      // The socket dropped while joining: the join it carried is lost.
      this.armOffline(held);
      this.startRecovery(held);
    }
  }

  /** The first join failed: nothing is held, the wish is over. */
  private fail(held: Held, error: VoiceError): void {
    this.held = null;
    this.stopHeartbeat();
    if (this.intent === held.wish) this.intent = null;
    this.tell(() => held.wish.hooks.failed?.(error));
    held.wish.settle({ type: "failed", error });
    held.wish.settleReleased();
  }

  /** End what is held for a wish that is no longer the intent. */
  private async release(held: Held): Promise<void> {
    this.clearOffline(held);
    const session = held.session;
    held.session = null;
    let adapter: VoiceAdapter | null = null;
    if (held.ending) {
      // Already ended outside the runner's turn (sign-out, give-up).
      await this.limited("Ending the voice session", held.ending, TEARDOWN_LIMIT_MS);
      held.ending = null;
    }
    if (session) {
      try {
        adapter = await this.getAdapter();
        const result = await this.limited(
          "Ending the voice session",
          held.silent ? adapter.teardown(session) : adapter.leave(session),
          TEARDOWN_LIMIT_MS,
        );
        if (result && !result.ok) {
          console.error("[VoiceOwner] Ending the voice session failed:", result.error);
        }
      } catch (err) {
        console.error("[VoiceOwner] Ending the voice session failed:", err);
      }
    }
    try {
      await this.bounded(
        "Cleaning up after the call",
        held.wish.hooks.released?.(adapter ?? this.adapter, () => this.held === held),
      );
    } catch (err) {
      console.warn("[VoiceOwner] Cleaning up after the voice session failed:", err);
    }
    if (this.held === held) {
      this.held = null;
      this.stopHeartbeat();
    }
    held.wish.settle({ type: "superseded" });
    held.wish.settleReleased();
  }

  /** Drop the held connection without `voice_leave` (between attempts). */
  private async endSession(adapter: VoiceAdapter, held: Held): Promise<void> {
    const session = held.session;
    if (!session) return;
    held.session = null;
    held.connected = false;
    try {
      const result = await this.limited(
        "Dropping the connection",
        adapter.teardown(session),
        TEARDOWN_LIMIT_MS,
      );
      if (result && !result.ok) {
        console.warn("[VoiceOwner] Dropping the connection failed:", result.error);
      }
    } catch (err) {
      console.warn("[VoiceOwner] Dropping the connection failed:", err);
    }
  }

  // --- the heartbeat and the deadline in real time ----------------------------

  private startHeartbeat(): void {
    if (this.heartbeat) return;
    this.lastBeat = this.clock();
    this.heartbeat = setInterval(() => this.wake(), HEARTBEAT_MS);
  }

  private stopHeartbeat(): void {
    if (!this.heartbeat) return;
    clearInterval(this.heartbeat);
    this.heartbeat = null;
  }

  /** The deadline applies and 60 s of real time have passed since the last proof. */
  private expired(held: Held): boolean {
    return !!held.offline && this.since(held.aliveAt) >= RECOVERY_OFFLINE_CAP_MS;
  }

  /** Give up if the deadline has passed. Safe to call from any event. */
  private checkDeadline(held: Held): boolean {
    if (this.intent !== held.wish || !this.expired(held)) return false;
    this.giveUp(held, "no socket carried the rejoin for 60 s");
    return true;
  }

  // --- recovery (a phase of the held session) --------------------------------

  private startRecovery(held: Held): void {
    if (held.phase !== "live") return;
    held.phase = "recovering";
    held.used = 0;
    this.tell(() => held.wish.hooks.recovering?.());
  }

  private armOffline(held: Held): void {
    // Runs from the last proof before the first loss; a later loss does not
    // restart it. The timer is only a wake-up for the case that the device
    // stays awake; the deadline itself is compared against the clock.
    if (held.offline) return;
    const remaining = Math.max(0, RECOVERY_OFFLINE_CAP_MS - this.since(held.aliveAt));
    held.offline = {
      timer: setTimeout(() => {
        if (this.held === held) this.checkDeadline(held);
        this.poke();
      }, remaining),
    };
  }

  private clearOffline(held: Held): void {
    if (!held.offline) return;
    clearTimeout(held.offline.timer);
    held.offline = null;
  }

  private label(held: Held): string {
    return `${held.wish.kind} ${held.wish.id}`;
  }

  private async recover(held: Held): Promise<void> {
    const wish = held.wish;
    const gone = () => this.intent !== wish;
    const waiting = () => !this.socketUp || !held.confirmed;
    while (!gone()) {
      this.checkClock();
      if (gone() || this.checkDeadline(held)) return;
      if (held.used >= RECOVERY_DELAYS_MS.length) return this.giveUp(held, "three attempts failed");
      if (waiting()) {
        // For the next authenticated socket (and the resync, for a call).
        await this.tick.promise;
        continue;
      }
      const pause = this.timer(RECOVERY_DELAYS_MS[held.used]);
      const interrupted = () => gone() || waiting() || this.expired(held);
      while (!pause.fired && !interrupted()) await this.tick.promise;
      pause.cancel();
      if (interrupted()) continue;

      const result = await this.attempt(held);
      if (result === "connected") return;
      if (result === "failed") {
        held.used += 1;
        console.warn(`[VoiceOwner] ${this.label(held)}: attempt ${held.used} failed`);
      } else {
        console.info(`[VoiceOwner] ${this.label(held)}: attempt interrupted, not counted`);
      }
    }
  }

  /**
   * Third failure or the 60 s deadline: the owner itself ends the wish.
   * The local media is released before this returns; the runner then finds
   * the wish over and finishes the clean-up.
   */
  private giveUp(held: Held, why: string): void {
    if (this.intent !== held.wish) return;
    console.warn(`[VoiceOwner] ${this.label(held)}: ${why}, giving up`);
    held.silent = true;
    this.intent = null;
    this.releaseNow(held);
    this.tell(() => held.wish.hooks.gaveUp?.());
    this.kick();
  }

  private async attempt(held: Held): Promise<AttemptResult> {
    const wish = held.wish;
    const gone = () => this.intent !== wish;
    const interrupted = () => gone() || !this.socketUp || this.expired(held);

    let adapter: VoiceAdapter;
    try {
      adapter = await this.getAdapter();
    } catch (err) {
      console.warn("[VoiceOwner] No voice adapter for the rejoin:", err);
      return "failed";
    }
    // Fresh connections: the old ones go without `voice_leave` (C32: the
    // server already removed the old peer; a leave could hit another device).
    await this.endSession(adapter, held);
    if (interrupted()) return "interrupted";
    this.listen(adapter);
    this.tell(() => wish.hooks.attemptStarts?.(adapter));

    const session: VoiceSessionHandle = { token: nextVoiceToken(), channelId: wish.id };
    held.session = session;
    held.connected = false;
    let result: AttemptResult;
    try {
      result = await this.runAttempt(adapter, held, session, interrupted);
    } catch (err) {
      console.warn(`[VoiceOwner] ${this.label(held)}: attempt threw:`, err);
      result = interrupted() ? "interrupted" : "failed";
    }
    if (result === "connected") return result;
    // The wish is over: the runner ends the attempt's session the way the
    // user's action asks for (with or without `voice_leave`).
    if (gone()) return "interrupted";
    await this.endSession(adapter, held);
    return result;
  }

  private async runAttempt(
    adapter: VoiceAdapter,
    held: Held,
    session: VoiceSessionHandle,
    interrupted: () => boolean,
  ): Promise<AttemptResult> {
    const hooks = held.wish.hooks;
    /** For this attempt's hooks: its session is still held and wanted. */
    const mine = () =>
      this.held === held && this.intent === held.wish && isSameSession(held.session, session);
    const clock = this.timer(RECOVERY_ATTEMPT_TIMEOUT_MS);
    const stop = () => interrupted() || clock.fired;
    const ended = (): AttemptResult => (interrupted() ? "interrupted" : "failed");
    try {
      const joined = await this.until(adapter.join(session), stop);
      if (!joined.done) return ended();
      if (!joined.value.ok) {
        if (!isJoinCancelled(joined.value.error)) {
          console.warn(`[VoiceOwner] ${this.label(held)}: rejoin failed:`, joined.value.error);
        }
        return ended();
      }
      if (interrupted()) return "interrupted";
      // This session's `voice_join` went out on an authenticated socket:
      // the deadline that runs since the loss is met.
      held.aliveAt = this.clock();
      held.verifying = false;
      this.clearOffline(held);
      if (hooks.rejoined) {
        await this.until(
          hooks.rejoined(adapter, session, mine).catch(() => undefined),
          stop,
        );
      }
      while (!held.connected && !stop()) await this.tick.promise;
      if (!held.connected || interrupted()) return ended();
    } finally {
      clock.cancel();
    }

    // Finalisation is part of the attempt: a socket loss or a change of
    // intent during it interrupts it, and success is declared only after
    // everything was checked again.
    const finishing = this.timer(RECOVERY_ATTEMPT_TIMEOUT_MS);
    try {
      if (hooks.restore) {
        await this.until(
          hooks.restore(adapter, session, mine).catch((err: unknown) => {
            console.warn(`[VoiceOwner] ${this.label(held)}: restore failed:`, err);
          }),
          () => interrupted() || finishing.fired,
        );
      }
    } finally {
      finishing.cancel();
    }
    if (interrupted()) return "interrupted";
    if (finishing.fired) return "failed";

    held.phase = "live";
    held.used = 0;
    held.aliveAt = this.clock();
    this.tell(() => hooks.recovered?.(adapter, session));
    return "connected";
  }
}

/** The owner of this window's voice session. */
export const voiceOwner = new VoiceOwner();
