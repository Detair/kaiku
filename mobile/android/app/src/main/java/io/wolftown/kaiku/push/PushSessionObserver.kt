package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Removes the posted message notifications and the queued wake fetches when
 * the signed-in account goes away: on every transition from a signed-in user
 * to signed out or to another user, and at process start when no session is
 * stored at all. The departing account's remembered calls are forgotten
 * with them. Wakes carry no account, so a late call wake meant for a previous
 * account could still post under the account signed in now; accepted, since
 * logout deletes the FCM token / UnifiedPush registration. Runs without any Activity (started from
 * the Application). The fetch itself also clears notifications when it finds
 * its session ended ([WakeFetchProcessor]).
 */
@Singleton
class PushSessionObserver @Inject constructor(
    private val authState: AuthState,
    private val tokenStorage: TokenStorage,
    private val sink: NotificationSink,
    private val enqueuer: WakeWorkEnqueuer,
    private val callStore: CallNotificationStore,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("PushSessionObserver")
    }

    private val started = AtomicBoolean(false)

    fun start() {
        if (!started.compareAndSet(false, true)) return
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            var previous: String? = null
            var first = true
            authState.session.collect { session ->
                val current = (session as? AuthSession.LoggedIn)?.userId
                // AuthState starts signed out in every process (also one started
                // by a wake) until MainActivity restores it, so a first "signed
                // out" only counts when no session is stored at all.
                val coldStartSignedOut = first && current == null && tokenStorage.getRefreshToken() == null
                if (coldStartSignedOut || (previous != null && previous != current)) {
                    logger.info("session ended: clearing message notifications and wake work")
                    enqueuer.cancelAll()
                    sink.cancelAll()
                    // The departed account's remembered calls go too (ids only,
                    // nothing of it stays). A late `ended` with nothing stored
                    // still shows a missed call (C20 addendum), see the KDoc.
                    val departed = previous
                    if (departed != null) callStore.clearUser(departed) else callStore.clearAll()
                }
                previous = current
                first = false
            }
        }
    }
}
