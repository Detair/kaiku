package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceStore
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Earlier versions stored the session of a sign-in made only to remove this phone, with a flag
 * beside the key record while it ran. This version never stores such a session ([PhoneRemoval]).
 * A flag still found after an update means that version stopped half way: the stored session is
 * that left-over. It is removed from storage first, and only when it is gone is the flag removed;
 * the server's sign-out follows best effort. The key stays; the phone opens at "Confirm it's you".
 * Runs before anything reads the stored session (application start).
 */
@Singleton
class LegacyRemovalSession @Inject constructor(
    private val store: KeyDeviceStore,
    private val tokenStorage: TokenStorage,
    private val authApi: AuthApi,
    @AuthCoroutineScope private val scope: CoroutineScope,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("LegacyRemovalSession")
        const val SERVER_LOGOUT_TIMEOUT_MS = 10_000L
    }

    fun endIfLeft() {
        // Not readable now: nothing is decided; the storage gate runs this again once it is.
        if (store.legacyRemovalMark() != true) return
        val serverUrl = tokenStorage.getServerUrl()
        val access = tokenStorage.getAccessToken()
        val refresh = tokenStorage.getRefreshToken()
        if (!tokenStorage.clear()) {
            logger.warning("Left-over removal session not cleared; the next start tries again")
            return
        }
        store.clearLegacyRemovalMark()
        if (serverUrl != null && (access != null || refresh != null)) {
            scope.launch {
                try {
                    withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) { authApi.logoutSession(serverUrl, access, refresh) }
                } catch (e: Exception) {
                    logger.warning("Server sign-out of a left-over removal session failed (best effort): ${e.javaClass.simpleName}")
                }
            }
        }
    }
}

/**
 * While the key record cannot be read, the app decides nothing about a key or a session: it shows
 * one screen that says so and offers to try again. [retry] reads the store again; once it can be
 * read, what waited for it runs.
 */
@Singleton
class KeyStorageGate @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    private val legacy: LegacyRemovalSession,
    private val deviceIds: DeviceIdProvider,
) {
    fun unreadable(): Boolean = keyDevices.unreadable()

    /**
     * At every start with a readable record: a key record from before records named their install
     * id takes the id the app has now, so that from then on every question about that key device
     * names the id it was enrolled under, whatever becomes of the app's own id. Nothing is read or
     * made for a phone without a record.
     */
    fun bindInstallId() {
        val rec = keyDevices.record.value ?: return
        if (rec.installId != null) return
        if (!keyDevices.bindInstallId(deviceIds.id()).done) logger.warning("Install id not recorded with the key record; the next start tries again")
    }

    private companion object {
        val logger: Logger = Logger.getLogger("KeyStorageGate")
    }

    /** True when the store can be read now. */
    fun retry(): Boolean {
        if (!keyDevices.reload()) return false
        legacy.endIfLeft()
        return true
    }
}
