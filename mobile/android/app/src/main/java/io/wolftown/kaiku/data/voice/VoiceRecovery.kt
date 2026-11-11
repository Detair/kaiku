package io.wolftown.kaiku.data.voice

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
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
 * Contract C32: rejoin a voice channel after the connection was lost.
 *
 * Pure logic (no Android, no WebRTC) so it runs on virtual time in unit tests.
 * It decides *when* to rejoin and when to give up; *how* is [rejoin].
 *
 * Triggers: a re-established WebSocket ([onSocketReady]) or ICE `FAILED` on a
 * live socket ([onIceFailed]), while a session ([begin]) exists. Up to
 * [delaysMs].size attempts, each preceded by its delay (1 s, 3 s, 9 s). An
 * attempt waits for the socket without being used up. After the last failure
 * [onGaveUp] is called once and the session ends. [end] (own leave,
 * `voice_removed`, logout) cancels everything; nothing re-arms until the next
 * [begin], so a removed or departed user is never pulled back in.
 *
 * ICE `DISCONNECTED` is an indication only ([VoiceLinkState.Interrupted]).
 *
 * [rejoin] must return true once the channel is connected again and false on
 * failure; it is cancelled after [attemptTimeoutMs]. Thread-safe: events arrive
 * from the WebSocket collector, WebRTC threads and the UI.
 */
class VoiceRecovery(
    private val scope: CoroutineScope,
    private val rejoin: suspend (channelId: String) -> Boolean,
    private val onGaveUp: (channelId: String) -> Unit,
    private val delaysMs: List<Long> = DEFAULT_DELAYS_MS,
    private val attemptTimeoutMs: Long = ATTEMPT_TIMEOUT_MS,
) {
    companion object {
        private val logger = Logger.getLogger("VoiceRecovery")

        val DEFAULT_DELAYS_MS = listOf(1_000L, 3_000L, 9_000L)
        const val ATTEMPT_TIMEOUT_MS = 10_000L
    }

    private val lock = Any()
    private val _state = MutableStateFlow<VoiceLinkState>(VoiceLinkState.Stable)
    val state: StateFlow<VoiceLinkState> = _state.asStateFlow()

    private val socketUp = MutableStateFlow(true)

    // Guarded by [lock].
    private var session: String? = null
    private var iceDisconnected = false
    private var loop: Job? = null

    /** A voice session in [channelId] started (a normal join, not a rejoin). */
    fun begin(channelId: String, socketUp: Boolean) {
        synchronized(lock) {
            cancelLoopLocked()
            session = channelId
            iceDisconnected = false
            this.socketUp.value = socketUp
            refreshIdleStateLocked()
        }
    }

    /** The session is over for any reason (own leave, `voice_removed`, logout, give-up). */
    fun end() {
        synchronized(lock) {
            cancelLoopLocked()
            session = null
            iceDisconnected = false
            _state.value = VoiceLinkState.Stable
        }
    }

    fun onSocketLost() {
        synchronized(lock) {
            socketUp.value = false
            refreshIdleStateLocked()
        }
    }

    /** The WebSocket is re-established and the text subscriptions were re-sent. */
    fun onSocketReady() {
        synchronized(lock) {
            socketUp.value = true
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

    private fun cancelLoopLocked() {
        loop?.cancel()
        loop = null
    }

    private fun refreshIdleStateLocked() {
        if (loop != null) return
        _state.value = if (session != null && (!socketUp.value || iceDisconnected)) {
            VoiceLinkState.Interrupted
        } else {
            VoiceLinkState.Stable
        }
    }

    private fun startLoopLocked() {
        val channel = session ?: return
        if (loop != null) return
        _state.value = VoiceLinkState.Rejoining(1)
        // Started under the lock but not UNDISPATCHED, so [loop] is set before it can finish.
        val job = scope.launch { runLoop(channel) }
        loop = job
    }

    private suspend fun runLoop(channel: String) {
        for ((index, wait) in delaysMs.withIndex()) {
            synchronized(lock) { if (session == channel) _state.value = VoiceLinkState.Rejoining(index + 1) }
            delay(wait)
            // Waiting for the socket does not use up an attempt.
            socketUp.first { it }
            val ok = try {
                withTimeoutOrNull(attemptTimeoutMs) { rejoin(channel) } == true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.log(Level.WARNING, "voice rejoin attempt ${index + 1} threw", e)
                false
            }
            if (ok) {
                finishLoop { /* session stays; back to stable */ }
                return
            }
            logger.info("voice rejoin attempt ${index + 1}/${delaysMs.size} failed")
        }
        val gaveUp = finishLoop { session = null; iceDisconnected = false }
        if (gaveUp) onGaveUp(channel)
    }

    /** Clears [loop] if it is still this run (not cancelled by [end]/[begin]); returns whether it was. */
    private suspend fun finishLoop(update: () -> Unit): Boolean {
        val self = kotlin.coroutines.coroutineContext[Job]
        synchronized(lock) {
            if (loop !== self) return false
            loop = null
            update()
            iceDisconnected = false
            refreshIdleStateLocked()
            return true
        }
    }
}
