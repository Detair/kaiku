package io.wolftown.kaiku.service

import io.wolftown.kaiku.data.voice.VoiceLinkState
import io.wolftown.kaiku.data.voice.statusText
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * When the ongoing-call notification of [VoiceCallService] is updated and
 * when it has to be gone. The Android calls themselves are [post] and
 * [cancel]; everything here runs on the service's (main) thread.
 *
 * While the call is on, the notification follows the voice link
 * ("Connecting…", "Reconnecting…") by being posted again under the same id.
 *
 * A call ends in steps that reach the main thread at different times: the
 * repository clears the call, then asks Android to stop the service (Android
 * takes the foreground notification away at once), and the service's
 * `onDestroy` runs later still. A notification posted in between would stay
 * behind: an ongoing "In voice channel" without a call. Therefore:
 * - an update is posted only while there is a call ([callIsOn], read at the
 *   moment of posting; the repository clears the call before it stops the
 *   service) and only through the [gate], which the stop request closes
 *   first: check and post are one step under the gate's lock, so once the
 *   stop was requested nothing is posted any more, and
 * - [destroy] ends the updates and removes whatever an update that was
 *   already on its way posted after the stop.
 */
internal class OngoingCallNotification(
    private val scope: CoroutineScope,
    private val callIsOn: () -> Boolean,
    private val gate: CallNotificationGate,
    private val linkState: Flow<VoiceLinkState>,
    /** Posts the notification with the link's status line ([statusText]; null: connected). */
    private val post: (status: Int?) -> Unit,
    private val cancel: () -> Unit,
) {
    private var updates: Job? = null
    private var following = -1L

    /**
     * Follows the link from now on, starting with its current state, for the
     * start [generation] (from [CallNotificationGate.open]). The service calls
     * it once it is in the foreground; again for the same start changes
     * nothing, a newer start replaces the updates.
     */
    fun follow(generation: Long) {
        if (updates?.isActive == true && following == generation) return
        updates?.cancel()
        following = generation
        updates = scope.launch {
            linkState
                .map { it.statusText() }
                .distinctUntilChanged()
                .collect { status -> gate.ifOpen(generation) { if (callIsOn()) post(status) } }
        }
    }

    /** The service is being destroyed: no update from here on, and nothing stays posted. */
    fun destroy() {
        updates?.cancel()
        updates = null
        cancel()
    }
}

/**
 * The order of a start and a stop of the call service, used by
 * `VoiceCallService.start`/`stop`: the gate opens before the start is
 * requested and closes before the stop is requested.
 */
internal class ServiceStartStop(private val gate: CallNotificationGate) {
    /** [requestStart] gets the start's generation, which the service's updates must carry. */
    fun start(requestStart: (generation: Long) -> Unit) {
        requestStart(gate.open())
    }

    fun stop(requestStop: () -> Unit) {
        gate.retire()
        requestStop()
    }
}

/**
 * Orders "post the ongoing-call notification" against "stop the service".
 * [open] when a call starts the service (a new generation per start);
 * [retire] before the stop is requested. [ifOpen] runs its block only while
 * open for its own start, under the same lock, so no post can slip in
 * between the retire and the stop, nor come from an earlier start.
 */
internal class CallNotificationGate {
    private val lock = Any()
    private var open = false
    private var generation = 0L

    /** Opens the gate for a new start and returns that start's generation. */
    fun open(): Long = synchronized(lock) {
        open = true
        ++generation
    }

    fun retire() = synchronized(lock) { open = false }

    /**
     * Runs [block] only while the gate is open for the start [generation]: an
     * update of an earlier, stopped start never posts, even after a newer
     * start opened the gate again.
     */
    fun ifOpen(generation: Long, block: () -> Unit) = synchronized(lock) {
        if (open && generation == this.generation) block()
    }

    /**
     * Whether an action of the ongoing-call notification posted for the start [generation]
     * applies now: only while that start is the open one. A tap on the notification of an
     * earlier start (another call, or one that ended) does nothing.
     */
    fun actionApplies(generation: Long): Boolean = synchronized(lock) { open && generation == this.generation }
}
