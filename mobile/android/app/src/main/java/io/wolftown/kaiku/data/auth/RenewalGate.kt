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

    fun onIntent(fromNotification: Boolean) {
        if (fromNotification) this.fromNotification = true
    }

    /** Consumes the recorded open source. */
    fun shouldPrompt(now: Instant, absoluteExpiresAt: Instant?, lastPromptAt: Instant?): Boolean {
        val fromNotif = fromNotification
        fromNotification = false
        return RenewalPolicy.shouldRenew(now, absoluteExpiresAt, lastPromptAt, fromNotif)
    }
}
