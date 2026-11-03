package io.wolftown.kaiku.navigation

import io.wolftown.kaiku.domain.ScanPayload

/**
 * Hands a scanned sign-in code to the approval screen, in memory only: tokens
 * never go into a navigation route or saved state. Setup codes go through
 * [PendingSetupLinks].
 */
object ScanHandoff {
    @Volatile private var login: ScanPayload.Login? = null

    fun putLogin(p: ScanPayload.Login) { login = p }
    fun takeLogin(): ScanPayload.Login? = login.also { login = null }
}
