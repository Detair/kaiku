package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

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

    fun post(e: SetupEntry) {
        _pending.value = e
    }

    fun take(): SetupEntry? {
        val e = _pending.value
        _pending.value = null
        return e
    }
}
