package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.PushApi
import io.wolftown.kaiku.data.api.PushRegistration
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.di.ChatCoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** Result of one registration pass. */
enum class RefreshOutcome {
    /** The server accepted a new or changed registration. */
    REGISTERED,

    /** The server already has this registration and it is recent. */
    UNCHANGED,

    /** Push is off (no transport or no permission); nothing registered. */
    OFF,

    /** The transport delivers its endpoint later (UnifiedPush). */
    PENDING,
    NOT_LOGGED_IN,

    /** Network or server trouble; worth another attempt. */
    RETRY,

    /** Rejected or broken; retrying the same request cannot help. */
    FAILED,
}

/**
 * Picks the push transport (C18) and registers this installation with the
 * server (C19), keyed by the per-install [DeviceIdStore] id.
 *
 * Triggers: a session starting ([start]), notifications becoming allowed
 * ([onPermissionChanged]), an FCM token refresh ([onNewFcmToken]) and the
 * retry worker ([PushRegistrationWorker]). Every pass runs under one lock;
 * a registration the server already holds (same account, provider and
 * endpoint, younger than [STALE_AFTER_MS]) is not sent again, so repeated
 * triggers cost one config request at most.
 *
 * Logs carry the provider and HTTP status/code only, never an endpoint.
 */
@Singleton
class PushRegistrar @Inject constructor(
    private val pushApi: PushApi,
    private val authState: AuthState,
    private val tokenStorage: TokenStorage,
    private val deviceIdStore: DeviceIdStore,
    private val platform: PushPlatform,
    private val transports: Set<@JvmSuppressWildcards PushTransport>,
    private val store: PushRegistrationStore,
    private val scheduler: PushRegistrationScheduler,
    @ChatCoroutineScope private val scope: CoroutineScope,
) {
    companion object {
        private val logger = Logger.getLogger("PushRegistrar")

        /** A registration older than this is sent again on the next pass. */
        const val STALE_AFTER_MS = 24L * 60 * 60 * 1000

        /** Logout waits at most this long for a running pass to finish. */
        const val LOGOUT_LOCK_WAIT_MS = 1_000L

        /** Logout gives the device DELETE at most this long. */
        const val LOGOUT_DELETE_TIMEOUT_MS = 2_000L
    }

    /** The account a pass works for (no tokens). */
    private data class Account(val userId: String, val serverUrl: String?)

    internal var now: () -> Long = System::currentTimeMillis

    private val _status = MutableStateFlow<PushStatus>(PushStatus.Unknown)
    val status: StateFlow<PushStatus> = _status.asStateFlow()

    private val mutex = Mutex()
    private val started = AtomicBoolean(false)

    /** Set while a triggered pass waits for the lock: further triggers join it. */
    private val refreshQueued = AtomicBoolean(false)

    /** Bumped by every logout; a pass that started before must not register. */
    private val epoch = AtomicLong(0)

    /** Parent of the passes launched by triggers, cancelled on logout. */
    private val triggered = SupervisorJob(scope.coroutineContext[Job])

    /** Transport of the last choice in this process. */
    @Volatile
    private var lastKind: TransportKind? = null

    /** Registers whenever a session starts (login, or restore on app start). */
    fun start() {
        if (!started.compareAndSet(false, true)) return
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            authState.session
                .map { (it as? AuthSession.LoggedIn)?.userId }
                .distinctUntilChanged()
                .collect { userId ->
                    if (userId != null) {
                        launchRefresh()
                    } else {
                        triggered.cancelChildren()
                        _status.value = PushStatus.Unknown
                    }
                }
        }
    }

    /** Notifications became allowed (permission prompt): register now. */
    fun onPermissionChanged() {
        launchRefresh()
    }

    /**
     * FCM rotated the token. Called from the messaging service, so it only
     * hands off: no network and no disk access on the caller's thread.
     */
    fun onNewFcmToken(token: String) {
        if (lastKind == TransportKind.FCM) {
            launchGuarded { onEndpoint(TransportKind.FCM, token) }
        } else {
            // Process started for the token (no pass ran yet): the worker
            // survives process death and re-runs the whole selection.
            scheduler.schedule()
        }
    }

    private fun launchRefresh() {
        if (!refreshQueued.compareAndSet(false, true)) return
        val dequeued = AtomicBoolean(false)
        val job = launchGuarded {
            mutex.withLock {
                dequeued.set(true)
                refreshQueued.set(false)
                refreshLocked(scheduleRetry = true)
            }
        }
        // Also when cancelled before it ran (logout): later triggers must not be swallowed.
        job.invokeOnCompletion { if (!dequeued.get()) refreshQueued.set(false) }
    }

    /** Launches a triggered pass; an unexpected error is logged, never crashes the app. */
    private fun launchGuarded(block: suspend () -> Unit): Job = scope.launch(triggered) {
        try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("push registration pass failed: ${e.javaClass.simpleName}")
        }
    }

    /**
     * One full pass: permission, server config, transport choice, endpoint,
     * registration. A network failure schedules [PushRegistrationWorker]
     * when [scheduleRetry] is set (the worker itself passes false).
     */
    suspend fun refresh(scheduleRetry: Boolean = true): RefreshOutcome =
        mutex.withLock { refreshLocked(scheduleRetry) }

    /** A transport delivered [endpoint] outside a pass (token rotation, UnifiedPush). */
    suspend fun onEndpoint(kind: TransportKind, endpoint: String): RefreshOutcome =
        mutex.withLock {
            val account = currentAccount() ?: return@withLock RefreshOutcome.NOT_LOGGED_IN
            registerLocked(kind, endpoint, distributor = null, account, epoch.get(), scheduleRetry = true)
        }

    /**
     * First logout step, while the session is still valid: removes this
     * device's server row (bounded by [LOGOUT_LOCK_WAIT_MS] +
     * [LOGOUT_DELETE_TIMEOUT_MS]), forgets the local registration and gives
     * up the transport endpoint in the background. Best effort: never throws
     * except for cancellation, and the local cleanup always runs.
     */
    suspend fun unregisterForLogout() {
        epoch.incrementAndGet()
        triggered.cancelChildren()
        scheduler.cancel()
        val previous = lastKind ?: store.load()?.let { kindOf(it.provider) }
        var locked = false
        try {
            locked = withTimeoutOrNull(LOGOUT_LOCK_WAIT_MS) {
                mutex.lock()
                true
            } ?: false
            if (currentAccount() != null) {
                val done = withTimeoutOrNull(LOGOUT_DELETE_TIMEOUT_MS) {
                    pushApi.unregisterDevice(deviceIdStore.id())
                    true
                }
                logger.info(if (done == true) "push device unregistered" else "push device unregister timed out")
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            logger.info("push device unregister failed: status=${e.status.value} code=${e.code}")
        } catch (e: Exception) {
            logger.info("push device unregister failed: ${e.javaClass.simpleName}")
        } finally {
            if (locked) mutex.unlock()
            store.clear()
            lastKind = null
            _status.value = PushStatus.Unknown
            previous?.let(::transportFor)?.let { transport ->
                scope.launch { unregisterQuietly(transport) }
            }
        }
    }

    private suspend fun refreshLocked(scheduleRetry: Boolean): RefreshOutcome {
        val account = currentAccount() ?: return RefreshOutcome.NOT_LOGGED_IN
        val startEpoch = epoch.get()

        if (!platform.notificationsAllowed()) {
            _status.value = PushStatus.Off(PushOffReason.PERMISSION_DENIED)
            return RefreshOutcome.OFF
        }

        val config = try {
            pushApi.getConfig()
        } catch (e: CancellationException) {
            throw e
        } catch (e: IOException) {
            return networkFailure("config", e, scheduleRetry)
        } catch (e: ApiException) {
            logger.info("push config failed: status=${e.status.value} code=${e.code}")
            return when {
                // A server without push support.
                e.status == HttpStatusCode.NotFound -> off(PushOffReason.NO_TRANSPORT)
                isRetryable(e.status) -> retry(scheduleRetry)
                else -> failed()
            }
        } catch (e: Exception) {
            logger.info("push config failed: ${e.javaClass.simpleName}")
            return failed()
        }

        val choice = PushTransportSelector.select(
            PushEnvironment(
                gmsAvailable = platform.gmsAvailable(),
                firebaseProjectId = platform.firebaseProjectId(),
                server = config,
                distributors = platform.distributors(),
                preferUnifiedPush = platform.preferUnifiedPush(),
                savedDistributor = platform.savedDistributor(),
            )
        )
        val (kind: TransportKind?, distributor: String?) = when (choice) {
            TransportChoice.Fcm -> TransportKind.FCM to null
            is TransportChoice.UnifiedPush -> TransportKind.UNIFIED_PUSH to choice.distributor
            is TransportChoice.Off -> null to null
        }

        val previous = lastKind ?: store.load()?.let { kindOf(it.provider) }
        if (previous != null && previous != kind) {
            logger.info("push transport changed: from=${previous.wire} to=${kind?.wire ?: "off"}")
            transportFor(previous)?.let { unregisterQuietly(it) }
            store.clear()
            lastKind = null
        }

        if (choice is TransportChoice.Off) return off(choice.reason)
        val transport = kind?.let(::transportFor) ?: return off(PushOffReason.NO_TRANSPORT)
        lastKind = kind

        var outcome = RefreshOutcome.PENDING
        return try {
            transport.requestEndpoint { endpoint ->
                // Already under the lock: register directly (onEndpoint would deadlock).
                outcome = registerLocked(transport.kind, endpoint, distributor, account, startEpoch, scheduleRetry)
            }
            outcome
        } catch (e: CancellationException) {
            throw e
        } catch (e: IOException) {
            networkFailure("endpoint", e, scheduleRetry)
        } catch (e: Exception) {
            logger.info("push endpoint failed: provider=${transport.kind.wire} ${e.javaClass.simpleName}")
            failed()
        }
    }

    private suspend fun registerLocked(
        kind: TransportKind,
        endpoint: String,
        distributor: String?,
        account: Account,
        startEpoch: Long,
        scheduleRetry: Boolean,
    ): RefreshOutcome {
        if (epoch.get() != startEpoch || currentAccount() != account) return RefreshOutcome.NOT_LOGGED_IN

        val hash = PushRegistrationRecord.hash(endpoint)
        val record = store.load()
        val age = record?.let { now() - it.registeredAtMs }
        if (record != null && age != null &&
            record.userId == account.userId &&
            record.serverUrl == account.serverUrl &&
            record.provider == kind.wire &&
            record.endpointHash == hash &&
            age in 0 until STALE_AFTER_MS
        ) {
            lastKind = kind
            _status.value = PushStatus.Active(kind, distributor)
            return RefreshOutcome.UNCHANGED
        }

        val registration = PushRegistration(
            provider = kind.wire,
            endpoint = endpoint,
            deviceId = deviceIdStore.id(),
            deviceLabel = platform.deviceLabel(),
        )
        try {
            pushApi.register(registration)
        } catch (e: CancellationException) {
            throw e
        } catch (e: IOException) {
            return networkFailure("register", e, scheduleRetry)
        } catch (e: ApiException) {
            // A 400 (e.g. VALIDATION_ERROR for an over-long endpoint) is final: no retry loop.
            logger.info("push register failed: provider=${kind.wire} status=${e.status.value} code=${e.code}")
            return if (isRetryable(e.status)) retry(scheduleRetry) else failed()
        } catch (e: Exception) {
            logger.info("push register failed: provider=${kind.wire} ${e.javaClass.simpleName}")
            return failed()
        }

        // Logged out while the request ran: the logout DELETE owns the row now.
        if (epoch.get() != startEpoch) return RefreshOutcome.NOT_LOGGED_IN
        store.save(PushRegistrationRecord(account.userId, account.serverUrl, kind.wire, hash, now()))
        lastKind = kind
        _status.value = PushStatus.Active(kind, distributor)
        logger.info("push registered: provider=${kind.wire}")
        return RefreshOutcome.REGISTERED
    }

    private fun currentAccount(): Account? {
        val userId = tokenStorage.getUserId()?.takeIf { it.isNotBlank() } ?: return null
        if (tokenStorage.getAccessToken() == null && tokenStorage.getRefreshToken() == null) return null
        return Account(userId, tokenStorage.getServerUrl())
    }

    private fun transportFor(kind: TransportKind): PushTransport? = transports.firstOrNull { it.kind == kind }

    private fun kindOf(wire: String): TransportKind? = TransportKind.entries.firstOrNull { it.wire == wire }

    private fun isRetryable(status: HttpStatusCode): Boolean =
        status.value >= 500 || status == HttpStatusCode.TooManyRequests

    private fun off(reason: PushOffReason): RefreshOutcome {
        _status.value = PushStatus.Off(reason)
        return RefreshOutcome.OFF
    }

    private fun failed(): RefreshOutcome {
        _status.value = PushStatus.Off(PushOffReason.ERROR)
        return RefreshOutcome.FAILED
    }

    private fun networkFailure(step: String, e: IOException, scheduleRetry: Boolean): RefreshOutcome {
        logger.info("push $step failed: ${e.javaClass.simpleName}")
        return retry(scheduleRetry)
    }

    private fun retry(scheduleRetry: Boolean): RefreshOutcome {
        if (scheduleRetry) scheduler.schedule()
        return RefreshOutcome.RETRY
    }

    private suspend fun unregisterQuietly(transport: PushTransport) {
        try {
            transport.unregister()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("push transport unregister failed: provider=${transport.kind.wire} ${e.javaClass.simpleName}")
        }
    }
}
