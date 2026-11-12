/**
 * Voice recovery flow (C32 + "C32 amendments 2026-11-11 23:30").
 *
 * One cancellable flow for every kind of voice session: channel voice
 * (stores/voice.ts, both adapters) and native DM-call voice (stores/call.ts).
 *
 * - At most three attempts, spaced 1 s, 3 s and 9 s.
 * - An attempt succeeds when the publisher connection is connected; one that
 *   has not got there after 15 s has failed (the hanging join is cancelled).
 * - A socket loss during an attempt cancels it WITHOUT using up one of the
 *   three; the flow waits for the next authenticated socket and resumes
 *   with the remaining budget. Later reconnects are never ignored.
 * - Without an authenticated socket for 60 s after the loss, the flow gives
 *   up exactly as after the third failure.
 * - Leave and `voice_removed` win: `cancel()` stops the flow at once, and
 *   every attempt runs under its own voice session epoch, so a join still
 *   pending inside an adapter step can no longer send `voice_join` or keep
 *   anything it acquired.
 */

import { bumpVoiceEpoch, isVoiceEpochStale } from "@/lib/webrtc/voiceSession";

/** Wait before each attempt (C32). */
export const RECOVERY_DELAYS_MS = [1000, 3000, 9000] as const;
/** An attempt fails when the publisher is not connected after this long. */
export const RECOVERY_ATTEMPT_TIMEOUT_MS = 15_000;
/** Give up when no authenticated socket came back this long after the loss. */
export const RECOVERY_OFFLINE_CAP_MS = 60_000;

/** What a recovery flow does for one kind of voice session. */
export interface RecoverySession {
  /** For logs. */
  label: string;
  /** Token of the join that established the session this flow recovers. */
  owner?: number;
  /**
   * One attempt under `epoch`: drop the old connections without
   * `voice_leave`, join again and resolve once the publisher is connected
   * (true) or the attempt failed (false). It may never settle; the flow
   * times it out. It must not change visible state once `epoch` is stale.
   */
  attempt(epoch: number): Promise<boolean>;
  /** Drop what an interrupted attempt (timeout, socket loss) set up. */
  abandon(): Promise<void>;
  /**
   * The attempt under `epoch` connected: restore mute/deafen/devices. It
   * may be interrupted (socket loss, cancel) and must not change visible
   * state; `commit` does that once the flow has re-checked everything.
   */
  restore(epoch: number): Promise<void>;
  /** Declare the session recovered (synchronous, after `restore`). */
  commit(epoch: number): void;
  /** Third failure or offline cap: clear the session, offer a rejoin. */
  giveUp(): Promise<void>;
}

type Interrupt = "socket_lost" | "cancelled";

/** Resolves after `ms`, or early with the reason when interrupted. */
type Waiter = (reason: Interrupt | "socket_ready") => void;

export class VoiceRecovery {
  private session: RecoverySession | null = null;
  private generation = 0;
  private used = 0;
  private socketDown = false;
  private offlineTimer: ReturnType<typeof setTimeout> | null = null;
  private waiters = new Set<Waiter>();
  private flow: Promise<void> | null = null;

  /** Whether a flow is running (waiting, sleeping or attempting). */
  get running(): boolean {
    return this.session !== null;
  }

  /** Owner token of the running flow (`RecoverySession.owner`). */
  get owner(): number | undefined {
    return this.session?.owner;
  }

  /** Attempts used up by the running flow (tests). */
  get attemptsUsed(): number {
    return this.used;
  }

  /**
   * Start recovering `session` unless a flow already runs (then the running
   * one goes on). With `socketDown` the first attempt waits for
   * `socketReady()`; the 60 s offline cap runs from now.
   */
  start(session: RecoverySession, opts: { socketDown?: boolean } = {}): Promise<void> {
    if (opts.socketDown) this.socketLost();
    if (this.session && this.flow) return this.flow;
    this.session = session;
    this.used = 0;
    const generation = ++this.generation;
    if (this.socketDown) this.armOfflineCap(generation);
    this.flow = this.run(session, generation).finally(() => {
      if (generation === this.generation) {
        this.session = null;
        this.flow = null;
        this.clearOfflineCap();
      }
    });
    return this.flow;
  }

  /** Leave, `voice_removed`, own join: stop now, nothing more is attempted. */
  cancel(): void {
    if (!this.session) return;
    this.generation += 1;
    this.session = null;
    this.flow = null;
    this.clearOfflineCap();
    this.wakeAll("cancelled");
  }

  /** The signalling socket dropped (idempotent while it stays down). */
  socketLost(): void {
    const wasDown = this.socketDown;
    this.socketDown = true;
    if (wasDown) return;
    if (this.session) this.armOfflineCap(this.generation);
    this.wakeAll("socket_lost");
  }

  /** An authenticated socket is back and re-subscribed. */
  socketReady(): void {
    this.socketDown = false;
    this.clearOfflineCap();
    this.wakeAll("socket_ready");
  }

  /**
   * The socket is open (not yet re-subscribed). Without a running flow it
   * counts as up at once (first connect, no voice to recover); a running
   * flow waits for `socketReady()` after the re-subscription.
   */
  socketOpen(): void {
    if (!this.session) this.socketReady();
  }

  /** Whether the socket is known to be down (tests, callers). */
  get signallingDown(): boolean {
    return this.socketDown;
  }

  private current(generation: number): boolean {
    return generation === this.generation;
  }

  private wakeAll(reason: Interrupt | "socket_ready"): void {
    for (const wake of [...this.waiters]) wake(reason);
  }

  /**
   * Sleep `ms` (null = until woken). Ends early with the first wake reason
   * listed in `wakeOn`; other wakes are ignored.
   */
  private wait(
    ms: number | null,
    wakeOn: ReadonlyArray<Interrupt | "socket_ready">,
  ): { done: Promise<Interrupt | "socket_ready" | "elapsed">; release: () => void } {
    let finish!: (reason: Interrupt | "socket_ready" | "elapsed") => void;
    const done = new Promise<Interrupt | "socket_ready" | "elapsed">((resolve) => {
      finish = (reason) => {
        if (timer) clearTimeout(timer);
        this.waiters.delete(waiter);
        resolve(reason);
      };
    });
    const waiter: Waiter = (reason) => {
      if (wakeOn.includes(reason)) finish(reason);
    };
    const timer = ms === null ? null : setTimeout(() => finish("elapsed"), ms);
    this.waiters.add(waiter);
    return { done, release: () => finish("elapsed") };
  }

  private armOfflineCap(generation: number): void {
    if (this.offlineTimer) return;
    this.offlineTimer = setTimeout(() => {
      this.offlineTimer = null;
      if (!this.current(generation) || !this.session || !this.socketDown) return;
      const session = this.session;
      console.warn(`[VoiceRecovery] ${session.label}: offline for 60 s, giving up`);
      // Stop the flow, then give up exactly as after the third failure.
      this.generation += 1;
      this.session = null;
      this.flow = null;
      this.wakeAll("cancelled");
      bumpVoiceEpoch();
      void session.giveUp();
    }, RECOVERY_OFFLINE_CAP_MS);
  }

  private clearOfflineCap(): void {
    if (this.offlineTimer) {
      clearTimeout(this.offlineTimer);
      this.offlineTimer = null;
    }
  }

  private async run(session: RecoverySession, generation: number): Promise<void> {
    while (this.used < RECOVERY_DELAYS_MS.length) {
      if (this.socketDown) {
        // Wait for the next authenticated socket (or a cancel / the cap).
        while (this.socketDown) {
          await this.wait(null, ["socket_ready", "cancelled"]).done;
          if (!this.current(generation)) return;
        }
      }

      const slept = await this.wait(RECOVERY_DELAYS_MS[this.used], [
        "socket_lost",
        "cancelled",
      ]).done;
      if (!this.current(generation)) return;
      if (slept !== "elapsed" || this.socketDown) continue; // wait for the socket

      const epoch = bumpVoiceEpoch();
      const interrupted = this.wait(RECOVERY_ATTEMPT_TIMEOUT_MS, [
        "socket_lost",
        "cancelled",
      ]);
      const outcome = await Promise.race([
        session.attempt(epoch).then(
          (ok): "connected" | "failed" => (ok ? "connected" : "failed"),
          (err: unknown) => {
            console.warn(`[VoiceRecovery] ${session.label}: attempt threw:`, err);
            return "failed" as const;
          },
        ),
        interrupted.done,
      ]);
      interrupted.release();
      if (!this.current(generation)) return;

      let result: "connected" | "failed" | Interrupt | "socket_ready" | "elapsed" = outcome;
      if (outcome === "connected" && !isVoiceEpochStale(epoch)) {
        // Finalisation is part of the attempt: a socket loss or cancel
        // during it interrupts it, and success is only declared after the
        // socket, the epoch and the flow have been checked again.
        const finishing = this.wait(RECOVERY_ATTEMPT_TIMEOUT_MS, ["socket_lost", "cancelled"]);
        const restored = await Promise.race([
          session.restore(epoch).then(
            () => "restored" as const,
            (err: unknown) => {
              console.warn(`[VoiceRecovery] ${session.label}: restore failed:`, err);
              return "restored" as const;
            },
          ),
          finishing.done,
        ]);
        finishing.release();
        if (!this.current(generation)) return;
        if (restored === "restored" && !this.socketDown && !isVoiceEpochStale(epoch)) {
          session.commit(epoch);
          return;
        }
        result = restored === "restored" ? "socket_lost" : restored;
      }

      // An attempt whose epoch someone else ended (not this flow) is
      // interrupted, not failed (W1-N5).
      const foreign = result === "failed" && isVoiceEpochStale(epoch);
      // Anything but a plain failure ends this attempt's session: a still
      // pending join stops at its next check, and what it set up is
      // dropped silently.
      if (result !== "failed" || foreign) {
        bumpVoiceEpoch();
        await session.abandon();
        if (!this.current(generation)) return;
      }
      if (foreign) {
        console.info(`[VoiceRecovery] ${session.label}: attempt interrupted, not counted`);
        continue;
      }
      if (result === "socket_lost" || this.socketDown) {
        console.info(`[VoiceRecovery] ${session.label}: socket lost during attempt, not counted`);
        continue;
      }
      this.used += 1;
      console.warn(
        `[VoiceRecovery] ${session.label}: attempt ${this.used} failed (${result === "elapsed" ? "timeout" : result})`,
      );
    }

    console.warn(`[VoiceRecovery] ${session.label}: three attempts failed, giving up`);
    bumpVoiceEpoch();
    this.generation += 1;
    this.session = null;
    this.flow = null;
    this.clearOfflineCap();
    await session.giveUp();
  }
}
