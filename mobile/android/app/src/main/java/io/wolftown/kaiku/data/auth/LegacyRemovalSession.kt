package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceStore
import io.wolftown.kaiku.data.keydevice.KeyStorage
import io.wolftown.kaiku.data.keydevice.KeyStorageCheck
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
 * Runs before anything reads the stored session (application start, [KeyStorageGate.start]), and
 * its one result ([endIfLeft]) is what the stored session waits for.
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

    /**
     * True when what an earlier version may have left is settled: nothing was left, or the
     * left-over session and then its flag are durably gone. False when anything about it cannot be
     * read (the flag, or the record beside it) or a clear did not reach storage (each is tried
     * twice; a failed clear counts only once the store was written through, see
     * [KeyStorageGate.retry]): then nothing may read the stored session yet, and nobody can sign in, so a flag that
     * stays behind can only ever clear the same left-over session again.
     */
    fun endIfLeft(): Boolean {
        // Not readable now: nothing is decided; the storage gate runs this again once it is.
        if (store.load() is KeyStorage.Unreadable) return false
        val mark = store.legacyRemovalMark() ?: return false
        if (!mark) return true
        val serverUrl = tokenStorage.getServerUrl()
        val access = tokenStorage.getAccessToken()
        val refresh = tokenStorage.getRefreshToken()
        if (!(tokenStorage.clear() || tokenStorage.clear())) {
            logger.warning("Left-over removal session not cleared; the next start tries again")
            return false
        }
        if (!(store.clearLegacyRemovalMark() || store.clearLegacyRemovalMark())) {
            logger.warning("Left-over removal flag not cleared; the next start tries again")
            return false
        }
        if (serverUrl != null && (access != null || refresh != null)) {
            scope.launch {
                try {
                    withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) { authApi.logoutSession(serverUrl, access, refresh) }
                } catch (e: Exception) {
                    logger.warning("Server sign-out of a left-over removal session failed (best effort): ${e.javaClass.simpleName}")
                }
            }
        }
        return true
    }
}

/**
 * While the key record cannot be read, or what an earlier version left is not settled
 * ([LegacyRemovalSession.endIfLeft]), the app decides nothing about a key or a session: the start
 * shows one screen that says so and offers to try again, and no background work reads the stored
 * session ([KeyStorageCheck]). [retry] reads the store again; once it can be read, what waited for
 * it runs.
 */
@Singleton
class KeyStorageGate @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    private val legacy: LegacyRemovalSession,
    private val deviceIds: DeviceIdProvider,
) : KeyStorageCheck {
    /** What an earlier version left is settled; false until [start] or [retry] found it so. */
    @Volatile
    private var settled = false

    override fun unreadable(): Boolean = keyDevices.unreadable() || !settled

    /** At the start of every process, before anything reads the stored session. True when readable. */
    fun start(): Boolean {
        val left = legacy.endIfLeft()
        // Here, where every process starts and before anything (push) can make an install id. A
        // binding that did not reach storage keeps everything closed: an id made meanwhile would
        // otherwise be taken for the key's at the next start.
        settled = left && !keyDevices.unreadable() && bindInstallId()
        return !unreadable()
    }

    /**
     * At every start with a readable record: a key record from before records named their install
     * id takes the id the app has kept, so that from then on every question about that key device
     * names the id it was enrolled under, whatever becomes of the app's own id. When the app has
     * kept none (a new one would have to be made), the record notes that its id is lost: no
     * answer about "this install" then forgets that key. Nothing is read or made for a phone
     * without a record.
     */
    fun bindInstallId(): Boolean {
        val rec = keyDevices.record.value ?: return true
        if (rec.installId != null || rec.installIdLost) return true
        if (keyDevices.bindInstallId(deviceIds.storedId()).done) return true
        logger.warning("Install id not recorded with the key record; the app stays closed until it is")
        return false
    }

    private companion object {
        val logger: Logger = Logger.getLogger("KeyStorageGate")
    }

    /**
     * True when the store can be read now. A write of the start that failed (the install id's
     * binding, the clear of an earlier version's flag) may still show in this process as if it
     * had been made: the store is first written through, and nothing opens until that succeeded.
     */
    fun retry(): Boolean {
        if (!keyDevices.flush()) return false
        if (!keyDevices.reload()) return false
        return start()
    }
}
