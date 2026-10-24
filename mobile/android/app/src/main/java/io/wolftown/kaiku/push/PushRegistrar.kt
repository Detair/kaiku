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
import kotlinx.coroutines.delay
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
import java.net.URI
import java.net.URISyntaxException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
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

        /** A pass waits at most this long for the logout's transport unregister to finish. */
        const val UNREGISTER_JOIN_TIMEOUT_MS = 5_000L

        /** Upper bound for obtaining an endpoint (and registering it) while holding the lock. */
        const val ENDPOINT_TIMEOUT_MS = 30_000L

        /** A registration that raced a session change is removed again within this bound. */
        const val ROLLBACK_DELETE_TIMEOUT_MS = 2_000L

        /** The server rejects longer UnifiedPush endpoints with a 400 (C19). */
        const val MAX_UP_ENDPOINT_BYTES = 2048

        /** The distributor has this long to deliver an endpoint after a registration request. */
        const val PENDING_ENDPOINT_TIMEOUT_MS = 60_000L

        /** Delay before the first retry after a distributor failure; doubles each time. */
        const val UP_RETRY_BASE_SECONDS = 30L

        /** After this many consecutive failures the status stays Off(ERROR) until the next trigger. */
        const val MAX_UP_FAILURES = 5
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

    /** Consecutive UnifiedPush failures since the last successful registration (this process). */
    private val upFailures = AtomicInteger(0)

    @Volatile
    private var pendingEndpointTimeout: Job? = null

    /** Parent of the passes launched by triggers, cancelled on logout. */
    private val triggered = SupervisorJob(scope.coroutineContext[Job])

    /** Transport of the last choice in this process. */
    @Volatile
    private var lastKind: TransportKind? = null

    /**
     * The logout's background transport unregister (e.g. FCM `deleteToken`).
     * Kept so the next pass waits for it: an immediate re-login must not
     * register the token that is being deleted.
     */
    @Volatile
    private var pendingTransportUnregister: Job? = null

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

    /**
     * UnifiedPush delivered an endpoint. Called from the connector service, so
     * it only hands off: no network and no disk access on the caller's thread.
     */
    fun onEndpointAsync(kind: TransportKind, endpoint: String) {
        launchGuarded {
            if (lastKind == kind && platform.notificationsAllowed()) {
                onEndpoint(kind, endpoint)
            } else {
                // Late endpoint of a transport we left, or no pass ran yet: select again.
                launchRefresh()
            }
        }
    }

    /** Sets the permission-free parts of a UnifiedPush failure and plans the bounded retry. */
    private fun handleUnifiedPushFailure() {
        if (lastKind != TransportKind.UNIFIED_PUSH || currentAccount() == null) return
        pendingEndpointTimeout?.cancel()
        _status.value = PushStatus.Off(PushOffReason.ERROR)
        val failures = upFailures.incrementAndGet()
        if (failures > MAX_UP_FAILURES) {
            logger.info("unifiedpush registration given up after $failures failures")
            return
        }
        scheduler.scheduleDelayed(UP_RETRY_BASE_SECONDS shl (failures - 1))
    }

    /** A pending distributor registration that never delivers an endpoint counts as a failure. */
    private fun armEndpointTimeout() {
        pendingEndpointTimeout?.cancel()
        pendingEndpointTimeout = scope.launch(triggered) {
            delay(PENDING_ENDPOINT_TIMEOUT_MS)
            logger.info("unifiedpush endpoint timed out")
            handleUnifiedPushFailure()
        }
    }

    /** The distributor dropped our registration: run the whole selection again. */
    fun onTransportUnregistered() {
        launchRefresh()
    }

    /**
     * The distributor could not register us. Ignored unless UnifiedPush is the active
     * transport of a logged-in account. Retried with a growing delay, [MAX_UP_FAILURES] times at most.
     */
    fun onTransportRegistrationFailed() {
        launchGuarded { handleUnifiedPushFailure() }
    }

    /** Re-runs the selection without blocking the caller (Settings changes). */
    fun refreshAsync() {
        launchRefresh()
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
            val distributor = if (kind == TransportKind.UNIFIED_PUSH) platform.savedDistributor() else null
            registerLocked(kind, endpoint, distributor, account, epoch.get(), scheduleRetry = true)
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
        upFailures.set(0)
        scheduler.cancel()
        val previous = lastKind ?: store.load()?.let { kindOf(it.provider) }
        var locked = false
        try {
            // A lock timeout still sends the DELETE: skipping it would leave this
            // device registered for the account that is signing out.
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
                // Background, so the logout cap holds; the next pass joins it.
                pendingTransportUnregister = scope.launch { unregisterQuietly(transport) }
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
        // The transport reads the distributor from the preferences; the selector may have fallen back to another.
        distributor?.let(platform::saveDistributor)

        awaitPendingTransportUnregister()

        var outcome = RefreshOutcome.PENDING
        return try {
            // Bounded: a stalled token request must not hold the lock forever.
            val finished = withTimeoutOrNull(ENDPOINT_TIMEOUT_MS) {
                transport.requestEndpoint { endpoint ->
                    // Already under the lock: register directly (onEndpoint would deadlock).
                    outcome = registerLocked(transport.kind, endpoint, distributor, account, startEpoch, scheduleRetry)
                }
                true
            }
            if (finished == null) {
                logger.info("push endpoint timed out: provider=${transport.kind.wire}")
                retry(scheduleRetry)
            } else {
                if (outcome == RefreshOutcome.PENDING && transport.kind == TransportKind.UNIFIED_PUSH) {
                    armEndpointTimeout()
                }
                outcome
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: NoDistributorException) {
            logger.info("push endpoint failed: no distributor")
            off(PushOffReason.NO_DISTRIBUTOR)
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

        if (kind == TransportKind.UNIFIED_PUSH) pendingEndpointTimeout?.cancel()

        // Endpoints are secrets and must reach the server over TLS only; a bad one is final.
        if (kind == TransportKind.UNIFIED_PUSH && !isAcceptableUnifiedPushEndpoint(endpoint)) {
            logger.info("push endpoint rejected: provider=${kind.wire}")
            return failed()
        }

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
            upFailures.set(0)
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

        // Logout or account/server change while the request ran: keep nothing
        // locally and remove the row again while tokens still authenticate it
        // (the response may even arrive after the logout DELETE). If the tokens
        // already belong to another account, that account's own pass follows and
        // re-registers this device for it.
        if (epoch.get() != startEpoch || currentAccount() != account) {
            rollbackRegistration(registration.deviceId)
            return RefreshOutcome.NOT_LOGGED_IN
        }
        store.save(PushRegistrationRecord(account.userId, account.serverUrl, kind.wire, hash, now()))
        lastKind = kind
        upFailures.set(0)
        _status.value = PushStatus.Active(kind, distributor)
        logger.info("push registered: provider=${kind.wire}")
        return RefreshOutcome.REGISTERED
    }

    private suspend fun awaitPendingTransportUnregister() {
        val pending = pendingTransportUnregister ?: return
        if (withTimeoutOrNull(UNREGISTER_JOIN_TIMEOUT_MS) { pending.join() } == null) {
            logger.info("push transport unregister still running; continuing")
        } else if (pendingTransportUnregister === pending) {
            pendingTransportUnregister = null
        }
    }

    private suspend fun rollbackRegistration(deviceId: String) {
        if (currentAccount() == null) return
        try {
            val done = withTimeoutOrNull(ROLLBACK_DELETE_TIMEOUT_MS) {
                pushApi.unregisterDevice(deviceId)
                true
            }
            logger.info(if (done == true) "push registration rolled back" else "push rollback timed out")
        } catch (e: CancellationException) {
            throw e
        } catch (e: ApiException) {
            logger.info("push rollback failed: status=${e.status.value} code=${e.code}")
        } catch (e: Exception) {
            logger.info("push rollback failed: ${e.javaClass.simpleName}")
        }
    }

    private fun currentAccount(): Account? {
        val userId = tokenStorage.getUserId()?.takeIf { it.isNotBlank() } ?: return null
        if (tokenStorage.getAccessToken() == null && tokenStorage.getRefreshToken() == null) return null
        return Account(userId, tokenStorage.getServerUrl())
    }

    /** https URL with a host that fits the server's 2048 byte limit (C19). */
    private fun isAcceptableUnifiedPushEndpoint(endpoint: String): Boolean {
        if (endpoint.toByteArray(Charsets.UTF_8).size > MAX_UP_ENDPOINT_BYTES) return false
        val uri = try {
            URI(endpoint)
        } catch (_: URISyntaxException) {
            return false
        }
        // URI.getHost() is null for names with underscores, which the server's URL parser accepts.
        val host = uri.host ?: uri.rawAuthority?.substringAfterLast('@')?.substringBefore(':')
        return uri.scheme.equals("https", ignoreCase = true) && !host.isNullOrBlank()
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
