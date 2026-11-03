package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.domain.ScanPayload

/**
 * Hands a scanned code to the screen that handles it, in memory only: sign-in
 * and setup tokens never go into a navigation route or saved state.
 */
object ScanHandoff {
    @Volatile private var login: ScanPayload.Login? = null
    @Volatile private var setup: ScanPayload.Setup? = null

    fun putLogin(p: ScanPayload.Login) { login = p }
    fun takeLogin(): ScanPayload.Login? = login.also { login = null }

    fun putSetup(p: ScanPayload.Setup) { setup = p }
    fun takeSetup(): ScanPayload.Setup? = setup.also { setup = null }
}
