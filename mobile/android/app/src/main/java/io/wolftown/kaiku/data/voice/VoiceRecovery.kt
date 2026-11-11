package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.logging.Level
import java.util.logging.Logger

/** What the voice connection is doing, for the voice screen and the ongoing-call notification. */
sealed interface VoiceLinkState {
    /** No voice session, or the session is connected. */
    data object Stable : VoiceLinkState

    /**
     * The connection is degraded and nothing was torn down: ICE reported
     * DISCONNECTED (it may recover by itself), or the WebSocket is down and the
     * rejoin waits for it. A "reconnecting" indication only.
     */
    data object Interrupted : VoiceLinkState

    /** Rejoin attempt [attempt] (1-based) is pending or running. */
    data class Rejoining(val attempt: Int) : VoiceLinkState
}

/**
 * Contract C32 (with the 2026-11-11 amendments): rejoin a voice channel after
 * the connection was lost.
 *
 * Pure logic (no Android, no WebRTC) so it runs on virtual time in unit tests.
 * It decides *when* to rejoin and when to give up; *how* is [rejoin].
 *
 * - Triggers: a re-established, authenticated WebSocket ([onSocketReady]) or
 *   ICE `FAILED` on a live socket ([onIceFailed]), while a session ([begin])
 *   exists.
 * - Up to [delaysMs].size attempts, each preceded by its delay (1 s, 3 s, 9 s)
 *   and cancelled after [attemptTimeoutMs] (15 s).
 * - Socket loss ([onSocketLost]) cancels the running attempt (or its delay)
 *   without using it up; the same attempt runs again, with its delay, once the
 *   next authenticated socket is there.
 * - No authenticated socket within [offlineCapMs] (60 s) of the loss: give up.
 * - Give-up (after the last failure or the offline cap): [onGaveUp] runs once,
 *   while the state still says "reconnecting", then the session ends.
 * - [end] / [endAndJoin] (own leave, `voice_removed`, logout) cancel
 *   everything; nothing re-arms until the next [begin], so a removed or
 *   departed user is never pulled back in. [endAndJoin] also waits until the
 *   running attempt has finished cancelling, so the caller's teardown cannot
 *   interleave with it.
 *
 * ICE `DISCONNECTED` is an indication only ([VoiceLinkState.Interrupted]).
 *
 * [rejoin] returns true once the channel is connected again and false on
 * failure. Thread-safe: events arrive from the WebSocket collector, WebRTC
 * threads and the UI.
 */
class VoiceRecovery(
    private val scope: CoroutineScope,
    private val rejoin: suspend (channelId: String) -> Boolean,
    private val onGaveUp: suspend (channelId: String) -> Unit,
    private val delaysMs: List<Long> = DEFAULT_DELAYS_MS,
    private val attemptTimeoutMs: Long = ATTEMPT_TIMEOUT_MS,
    private val offlineCapMs: Long = OFFLINE_CAP_MS,
) {
    companion object {
        private val logger = Logger.getLogger("VoiceRecovery")

        val DEFAULT_DELAYS_MS = listOf(1_000L, 3_000L, 9_000L)
        const val ATTEMPT_TIMEOUT_MS = 15_000L
        const val OFFLINE_CAP_MS = 60_000L
    }

    private enum class Outcome { Connected, Failed, SocketLost }

    private val lock = Any()
    private val _state = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable)
    val state: StateFlow<VoiceLinkState> = _state.asStateFlow()

    private val socketUp = MutableStateFlow(true)

    // Guarded by [lock].
    private var session: String? = null
    private var iceDisconnected = false
    /** The rejoin loop, or the offline-cap give-up that replaced it. */
    private var loop: Job? = null
    /** The delay + attempt of the loop that is running now; socket loss cancels it. */
    private var segment: Job? = null
    private var offlineTimer: Job? = null

    /** A voice session in [channelId] started (a normal join, not a rejoin). */
    fun begin(channelId: String, socketUp: Boolean) {
        synchronized(lock) {
            cancelAllLocked()
            session = channelId
            iceDisconnected = false
            this.socketUp.value = socketUp
            if (!socketUp) startOfflineTimerLocked()
            refreshIdleStateLocked()
        }
    }

    /**
     * The session is over for any reason (own leave, `voice_removed`, logout).
     * Returns the cancelled jobs; [endAndJoin] waits for them.
     */
    fun end(): List<Job> = synchronized(lock) {
        val jobs = cancelAllLocked()
        session = null
        iceDisconnected = false
        _state.value = VoiceLinkState.Stable
        jobs
    }

    /**
     * [end], then waits until a running attempt has finished cancelling (its
     * clean-up ran). Called from inside the loop itself it does not wait for
     * itself.
     */
    suspend fun endAndJoin() {
        val self = currentCoroutineContext()[Job]
        for (job in end()) {
            if (job !== self) job.join()
        }
    }

    fun onSocketLost() {
        synchronized(lock) {
            socketUp.value = false
            // The running attempt (or its delay) is not used up; the loop waits.
            segment?.cancel(SocketLost())
            startOfflineTimerLocked()
            refreshIdleStateLocked()
        }
    }

    /** The WebSocket is re-established and authenticated. */
    fun onSocketReady() {
        synchronized(lock) {
            socketUp.value = true
            offlineTimer?.cancel()
            offlineTimer = null
            startLoopLocked()
        }
    }

    fun onIceDisconnected() {
        synchronized(lock) {
            iceDisconnected = true
            refreshIdleStateLocked()
        }
    }

    fun onIceConnected() {
        synchronized(lock) {
            iceDisconnected = false
            refreshIdleStateLocked()
        }
    }

    /** ICE `FAILED`. Ignored while the socket is down: the next [onSocketReady] handles it. */
    fun onIceFailed() {
        synchronized(lock) {
            if (!socketUp.value) return
            startLoopLocked()
        }
    }

    // -- Internal -------------------------------------------------------------

    private class SocketLost : CancellationException("socket lost during a rejoin attempt")

    private fun cancelAllLocked(): List<Job> {
        val jobs = listOfNotNull(loop, offlineTimer)
        jobs.forEach { it.cancel() }
        loop = null
        segment = null
        offlineTimer = null
        return jobs
    }

    private fun refreshIdleStateLocked() {
        if (loop != null) return
        _state.value = if (session != null && (!socketUp.value || iceDisconnected)) {
            VoiceLinkState.Interrupted
        } else {
            VoiceLinkState.Stable
        }
    }

    private fun startOfflineTimerLocked() {
        val channel = session ?: return
        if (offlineTimer != null) return
        offlineTimer = scope.launch {
            delay(offlineCapMs)
            giveUpOffline(channel)
        }
    }

    private fun startLoopLocked() {
        val channel = session ?: return
        if (loop != null) return
        _state.value = VoiceLinkState.Rejoining(1)
        // Started under the lock but not UNDISPATCHED, so [loop] is set before it runs.
        loop = scope.launch { runLoop(channel) }
    }

    private suspend fun runLoop(channel: String) {
        var index = 0
        while (index < delaysMs.size) {
            synchronized(lock) { if (session == channel) _state.value = VoiceLinkState.Rejoining(index + 1) }
            socketUp.first { it }
            when (runSegment(channel, delaysMs[index], index + 1)) {
                Outcome.Connected -> {
                    finish(channel) { /* session stays */ }
                    return
                }
                Outcome.Failed -> {
                    logger.info("voice rejoin attempt ${index + 1}/${delaysMs.size} failed")
                    index++
                }
                Outcome.SocketLost -> logger.info("voice rejoin attempt ${index + 1} interrupted by socket loss")
            }
        }
        onGaveUp(channel)
        finish(channel) { session = null }
    }

    /** The delay and one attempt; cancelled by socket loss ([Outcome.SocketLost]). */
    private suspend fun runSegment(channel: String, wait: Long, number: Int): Outcome = coroutineScope {
        val run = async {
            delay(wait)
            try {
                withTimeoutOrNull(attemptTimeoutMs) { rejoin(channel) } == true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "voice rejoin attempt $number threw", e)
                false
            }
        }
        val lost = synchronized(lock) {
            if (socketUp.value) {
                segment = run
                false
            } else {
                true
            }
        }
        if (lost) run.cancel(SocketLost())
        try {
            if (run.await()) Outcome.Connected else Outcome.Failed
        } catch (e: CancellationException) {
            // Our own cancellation (end, begin, offline cap) wins over socket loss.
            currentCoroutineContext().ensureActive()
            Outcome.SocketLost
        } finally {
            synchronized(lock) { if (segment === run) segment = null }
        }
    }

    /** The offline cap ran out: stop the loop (and wait for it), then give up. */
    private suspend fun giveUpOffline(channel: String) {
        val self = currentCoroutineContext()[Job]
        val previous = synchronized(lock) {
            if (offlineTimer !== self || session != channel || socketUp.value) return
            val running = loop
            running?.cancel()
            // From now on this job is the "loop": end() cancels and joins it.
            loop = self
            segment = null
            offlineTimer = null
            running
        }
        previous?.join()
        logger.info("voice rejoin gave up: no authenticated socket within ${offlineCapMs / 1000} s")
        onGaveUp(channel)
        finish(channel) { session = null }
    }

    /** Clears [loop] if it is still this run (not cancelled by [end]/[begin]). */
    private suspend fun finish(channel: String, update: () -> Unit) {
        val self = currentCoroutineContext()[Job]
        synchronized(lock) {
            if (loop !== self || session != channel) return
            loop = null
            update()
            if (session == null) {
                offlineTimer?.cancel()
                offlineTimer = null
            }
            iceDisconnected = false
            refreshIdleStateLocked()
        }
    }
}
