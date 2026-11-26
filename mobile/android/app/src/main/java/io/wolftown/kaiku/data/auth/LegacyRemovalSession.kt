package io.wolftown.kaiku.data.auth

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
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
    fun endIfLeft(): Boolean = outcome() == LeftOver.SETTLED

    /** What [endIfLeft] found: settled, not readable, or a clear that did not reach storage. */
    internal enum class LeftOver { SETTLED, UNREADABLE, NOT_WRITTEN }

    internal fun outcome(): LeftOver {
        // Not readable now: nothing is decided; the storage gate runs this again once it is.
        if (store.load() is KeyStorage.Unreadable) return LeftOver.UNREADABLE
        val mark = store.legacyRemovalMark() ?: return LeftOver.UNREADABLE
        if (!mark) return LeftOver.SETTLED
        val serverUrl = tokenStorage.getServerUrl()
        val access = tokenStorage.getAccessToken()
        val refresh = tokenStorage.getRefreshToken()
        if (!(tokenStorage.clear() || tokenStorage.clear())) {
            logger.warning("Left-over removal session not cleared; the next start tries again")
            return LeftOver.NOT_WRITTEN
        }
        if (!(store.clearLegacyRemovalMark() || store.clearLegacyRemovalMark())) {
            logger.warning("Left-over removal flag not cleared; the next start tries again")
            return LeftOver.NOT_WRITTEN
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
        return LeftOver.SETTLED
    }
}

/** Why the storage gate is closed: what the phone saved cannot be read, or a write did not reach storage. */
enum class KeyStorageProblem { CANNOT_READ, CANNOT_WRITE }

/**
 * What the closed gate's screen says for [p]: the notice, the button (named by the notice), and the
 * last resort, which only data that cannot be read gets. A write that failed never leads to
 * clearing the app's storage: that would delete this phone's key, and freeing up space is enough.
 */
data class KeyStorageTexts(@StringRes val notice: Int, @StringRes val button: Int, @StringRes val lastResort: Int?)

fun keyStorageTexts(p: KeyStorageProblem): KeyStorageTexts = when (p) {
    KeyStorageProblem.CANNOT_READ -> KeyStorageTexts(R.string.security_key_storage_unreadable, R.string.common_try_again, R.string.security_clear_storage_hint)
    KeyStorageProblem.CANNOT_WRITE -> KeyStorageTexts(R.string.security_key_storage_not_saved, R.string.common_try_again, null)
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

    /** The gate is closed because a write did not reach storage (not because something cannot be read). */
    @Volatile
    private var notWritten = false

    /** Why the gate is closed now (meaningful while [unreadable]). */
    fun problem(): KeyStorageProblem = if (notWritten) KeyStorageProblem.CANNOT_WRITE else KeyStorageProblem.CANNOT_READ

    override fun unreadable(): Boolean = keyDevices.unreadable() || !settled

    /** At the start of every process, before anything reads the stored session. True when readable. */
    fun start(): Boolean {
        val left = legacy.outcome()
        // Here, where every process starts and before anything (push) can make an install id. A
        // binding that did not reach storage keeps everything closed: an id made meanwhile would
        // otherwise be taken for the key's at the next start.
        val readable = left == LegacyRemovalSession.LeftOver.SETTLED && !keyDevices.unreadable()
        settled = readable && bindInstallId()
        // Readable, and still not settled: a clear or the binding did not reach storage.
        notWritten = left == LegacyRemovalSession.LeftOver.NOT_WRITTEN || (readable && !settled)
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
        if (!keyDevices.flush()) {
            notWritten = true
            return false
        }
        if (!keyDevices.reload()) {
            notWritten = false
            return false
        }
        return start()
    }
}
