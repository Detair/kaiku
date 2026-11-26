package io.wolftown.kaiku.data.voice

/**
 * Tells the realtime connection that the voice session found the
 * authenticated socket dead (no fresh frame after its ping), so a new socket
 * comes now and not only from the heartbeat watchdog.
 */
fun interface DeadSocketReporter {
    fun report()

    companion object {
        val NONE = DeadSocketReporter {}
    }
}
