package io.wolftown.kaiku.data.auth

import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.AuthCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** "Use another account" removals of this phone that are running in this process right now. */
@Singleton
class RemovalsInProgress @Inject constructor() {
    private val count = AtomicInteger(0)

    /** Held by a running removal; closing it (once or more) ends it. */
    fun begin(): AutoCloseable {
        count.incrementAndGet()
        val closed = AtomicBoolean(false)
        return AutoCloseable { if (closed.compareAndSet(false, true)) count.decrementAndGet() }
    }

    fun any(): Boolean = count.get() > 0
}

/** What [UnfinishedRemoval.atActivityStart] found. */
enum class RemovalAtStart {
    /** No sign-in made only for a removal is left. */
    NONE,
    /** One was left (the app died, or its screen went away); its session is ended now. */
    ENDED,
    /** The removal still runs in this process (its screen survived, e.g. a rotation); it ends it itself. */
    RUNNING,
}

/**
 * "Use another account" on a signed-out key phone signs in with the key only to remove the phone.
 * That session must never become an ordinary sign-in the person did not ask for: a mark is stored
 * before the sign-in ([KeyDeviceRepository.markRemovalSignIn]) and cleared when the removal ends,
 * also when it is stopped half way. If the mark is still set at an activity start and no removal
 * runs in this process, [atActivityStart] ends that session before the app reads it: the stored
 * session and the mark are cleared at once, and only the server's sign-out runs afterwards (best
 * effort). The key stays; the phone shows "Confirm it's you".
 */
@Singleton
class UnfinishedRemoval @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    private val tokenStorage: TokenStorage,
    private val authState: AuthState,
    private val authApi: AuthApi,
    private val running: RemovalsInProgress,
    @AuthCoroutineScope private val scope: CoroutineScope,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("UnfinishedRemoval")
        const val SERVER_LOGOUT_TIMEOUT_MS = 10_000L
    }

    /**
     * At every activity start, before the auth state is set up from storage. Synchronous: when it
     * returns [RemovalAtStart.ENDED], no session is stored any more.
     */
    fun atActivityStart(): RemovalAtStart {
        if (!keyDevices.removalSignInPending()) return RemovalAtStart.NONE
        if (running.any()) return RemovalAtStart.RUNNING
        val serverUrl = tokenStorage.getServerUrl()
        val access = tokenStorage.getAccessToken()
        val refresh = tokenStorage.getRefreshToken()
        tokenStorage.clear()
        keyDevices.markRemovalSignIn(false)
        // A session still held in memory (the screen went away, the process stayed) ends too.
        authState.setLoggedOut()
        if (serverUrl != null && (access != null || refresh != null)) {
            scope.launch {
                try {
                    withTimeoutOrNull(SERVER_LOGOUT_TIMEOUT_MS) { authApi.logoutSession(serverUrl, access, refresh) }
                } catch (e: Exception) {
                    logger.warning("Server sign-out after an unfinished removal failed (best effort): ${e.javaClass.simpleName}")
                }
            }
        }
        return RemovalAtStart.ENDED
    }

    /** Whether the app opens at "Confirm it's you" instead of its usual start after [r]. */
    fun startsAtConfirm(r: RemovalAtStart): Boolean = r != RemovalAtStart.NONE && keyDevices.local() != null
}
