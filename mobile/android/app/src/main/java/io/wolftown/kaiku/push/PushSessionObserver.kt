package io.wolftown.kaiku.push

import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
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
 * to signed out or to another user. Runs without any Activity (started from
 * the Application). The fetch itself also clears notifications when it finds
 * its session ended ([WakeFetchProcessor]).
 */
@Singleton
class PushSessionObserver @Inject constructor(
    private val authState: AuthState,
    private val sink: NotificationSink,
    private val enqueuer: WakeWorkEnqueuer,
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
            authState.session.collect { session ->
                val current = (session as? AuthSession.LoggedIn)?.userId
                if (previous != null && previous != current) {
                    logger.info("session ended: clearing message notifications and wake work")
                    enqueuer.cancelAll()
                    sink.cancelAll()
                }
                previous = current
            }
        }
    }
}
