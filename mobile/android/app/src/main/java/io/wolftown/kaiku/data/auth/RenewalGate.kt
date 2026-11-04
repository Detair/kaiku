package io.wolftown.kaiku.data.auth

import java.time.Instant

/**
 * Remembers whether the current app open came from a notification (or a link)
 * and applies [RenewalPolicy]: the B.4 renewal prompt never follows a
 * notification tap (coordinator call 5). The intent is read before the
 * activity strips its consumed link, so the flag is recorded there.
 */
class RenewalGate {
    @Volatile
    private var fromNotification = false

    /** The intent this open came with; a later intent replaces an earlier one. */
    fun onIntent(fromNotification: Boolean) {
        this.fromNotification = fromNotification
    }

    /**
     * The app left the screen: a flag recorded while it was already in front
     * (a notification tapped there, onNewIntent without onStart) belongs to no
     * app open and must not suppress the next normal open.
     */
    fun onStopped() {
        fromNotification = false
    }

    /** Consumes the recorded open source. */
    fun shouldPrompt(now: Instant, absoluteExpiresAt: Instant?, lastPromptAt: Instant?): Boolean {
        val fromNotif = fromNotification
        fromNotification = false
        return RenewalPolicy.shouldRenew(now, absoluteExpiresAt, lastPromptAt, fromNotif)
    }
}
