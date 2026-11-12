package io.wolftown.kaiku.service

import io.wolftown.kaiku.data.voice.VoiceLinkState
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
 * ("Reconnecting…") by being posted again under the same id.
 *
 * A call ends in steps that reach the main thread at different times: the
 * repository clears the call, then asks Android to stop the service (Android
 * takes the foreground notification away at once), and the service's
 * `onDestroy` runs later still. A notification posted in between would stay
 * behind: an ongoing "In voice channel" without a call. Therefore:
 * - an update is posted only while there is a call ([callIsOn], read at the
 *   moment of posting; the repository clears the call before it stops the
 *   service), and
 * - [destroy] ends the updates and removes whatever an update that was
 *   already on its way posted after the stop.
 */
internal class OngoingCallNotification(
    private val scope: CoroutineScope,
    private val callIsOn: () -> Boolean,
    private val linkState: Flow<VoiceLinkState>,
    private val post: (reconnecting: Boolean) -> Unit,
    private val cancel: () -> Unit,
) {
    private var updates: Job? = null

    /**
     * Follows the link from now on, starting with its current state. The
     * service calls it once it is in the foreground; calling it again changes
     * nothing.
     */
    fun follow() {
        if (updates?.isActive == true) return
        updates = scope.launch {
            linkState
                .map { it != VoiceLinkState.Stable }
                .distinctUntilChanged()
                .collect { reconnecting -> if (callIsOn()) post(reconnecting) }
        }
    }

    /** The service is being destroyed: no update from here on, and nothing stays posted. */
    fun destroy() {
        updates?.cancel()
        updates = null
        cancel()
    }
}
