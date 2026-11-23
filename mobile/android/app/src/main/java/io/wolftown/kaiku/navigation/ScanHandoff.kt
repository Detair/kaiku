package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.domain.ScanPayload

/**
 * Hands a scanned sign-in code to the approval screen, in memory only: tokens
 * never go into a navigation route or saved state. Setup codes go through
 * [PendingSetupLinks].
 */
object ScanHandoff {
    private val login = java.util.concurrent.atomic.AtomicReference<ScanPayload.Login?>(null)

    private val pcSetup = java.util.concurrent.atomic.AtomicReference<ScanPayload.PcSetup?>(null)

    fun putPcSetup(p: ScanPayload.PcSetup) { pcSetup.set(p) }

    /** At most once, like [takeLogin]. */
    fun takePcSetup(): ScanPayload.PcSetup? = pcSetup.getAndSet(null)

    fun putLogin(p: ScanPayload.Login) { login.set(p) }

    /** At most once, also with two takers at the same time. */
    fun takeLogin(): ScanPayload.Login? = login.getAndSet(null)

    /** A sign-out drops a scanned code nobody took. */
    fun clear() { login.set(null); pcSetup.set(null) }
}
