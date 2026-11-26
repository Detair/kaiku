package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.getAndUpdate

/**
 * Setup links (L2-A15): `kaiku://setup/v1?o=&t=` (the "Open in Kaiku" intent of
 * the `/setup` page) and the App Link `https://<host>/setup#v=1&t=`. Sign-in
 * codes are never accepted from a link.
 */
object SetupLinks {
    fun parse(uri: String): SetupEntry? =
        (KaikuPayloads.parse(uri) as? ScanPayload.Setup)?.let { SetupEntry(it.origin, it.tokenB64u, SetupEntry.Source.LINK) }
}

/** A setup link MainActivity received, until the navigation takes it (L2-A13). In memory only. */
object PendingSetupLinks {
    private val _pending = MutableStateFlow<SetupEntry?>(null)
    val pending: StateFlow<SetupEntry?> = _pending.asStateFlow()

    /** A link the app was opened with while it could not start, until it can ([releaseKept]). */
    private val kept = MutableStateFlow<SetupEntry?>(null)

    fun post(e: SetupEntry) {
        _pending.value = e
    }

    /**
     * An opened link goes to the one setup first ([offer], the setup owner's, told which link is
     * already waiting for the setup screen): only a link it is to use is posted; any other was
     * noted there and goes nowhere. Judged and posted in one step, so a link that waits is never
     * replaced by a later one.
     */
    @Synchronized
    fun deliver(e: SetupEntry, offer: (SetupEntry, SetupEntry?) -> Boolean) {
        if (offer(e, _pending.value)) post(e)
    }

    /**
     * The app was opened with [uri] while it could not start (what it has saved about its key
     * cannot be read): a setup link is kept, in memory, until the app does start ([releaseKept]).
     * True when [uri] was a setup link. Nothing else of the link is looked at.
     */
    fun keep(uri: String?): Boolean {
        val e = uri?.let(SetupLinks::parse) ?: return false
        kept.value = e
        return true
    }

    /** Once the app has started: the kept link is delivered like any link opened now ([deliver]). */
    fun releaseKept(offer: (SetupEntry, SetupEntry?) -> Boolean) {
        kept.getAndUpdate { null }?.let { deliver(it, offer) }
    }

    /** Takes the pending link at most once, also with two takers at the same time. */
    fun take(): SetupEntry? = _pending.getAndUpdate { null }

    /** Clears [e] once its screen has it; a newer link posted meanwhile stays. */
    fun consume(e: SetupEntry) {
        _pending.compareAndSet(e, null)
    }

    fun clear() {
        _pending.value = null
        kept.value = null
    }
}
