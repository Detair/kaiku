package io.wolftown.kaiku.push

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.FcmPublicConfig
import io.wolftown.kaiku.data.api.PushApi
import io.wolftown.kaiku.data.api.PushConfig
import io.wolftown.kaiku.data.api.PushRegistration
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class PushRegistrarTest {

    private class FakeTransport(
        override val kind: TransportKind,
        var endpoint: String,
    ) : PushTransport {
        var requests = 0
        var unregisters = 0
        var failWith: Exception? = null
        var hang = false
        var silent = false
        var unregisterGate: CompletableDeferred<Unit>? = null
        val events = mutableListOf<String>()

        override suspend fun requestEndpoint(onEndpoint: suspend (String) -> Unit) {
            requests++
            events += "request"
            failWith?.let { throw it }
            if (hang) awaitCancellation()
            if (silent) return
            onEndpoint(endpoint)
        }

        override suspend fun unregister() {
            unregisterGate?.await()
            unregisters++
            events += "unregistered"
        }
    }

    private class FakePlatform : PushPlatform {
        var allowed = true
        var gms = true
        var project: String? = PROJECT
        var installed: List<String> = emptyList()

        override fun notificationsAllowed() = allowed
        override fun gmsAvailable() = gms
        override fun firebaseProjectId() = project
        override fun distributors() = installed
        override fun preferUnifiedPush() = false
        var saved: String? = null
        override fun savedDistributor(): String? = saved
        override fun saveDistributor(distributor: String) {
            saved = distributor
        }
        override fun deviceLabel() = LABEL
    }

    private class MemoryStore : PushRegistrationStore {
        var record: PushRegistrationRecord? = null
        override fun load() = record
        override fun save(record: PushRegistrationRecord) {
            this.record = record
        }
        override fun clear() {
            record = null
        }
    }

    private class FakeScheduler : PushRegistrationScheduler {
        var scheduled = 0
        var cancelled = 0
        val delays = mutableListOf<Long>()
        override fun schedule() {
            scheduled++
        }
        override fun scheduleDelayed(delaySeconds: Long) {
            delays += delaySeconds
        }
        override fun cancel() {
            cancelled++
        }
    }

    private val fcmConfig = PushConfig(listOf("fcm", "unifiedpush"), FcmPublicConfig(PROJECT))
    private val upOnlyConfig = PushConfig(listOf("unifiedpush"), null)

    private val api: PushApi = mockk(relaxed = true) {
        coEvery { getConfig() } returns fcmConfig
    }
    private var userId: String? = USER
    private var serverUrl: String? = SERVER
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getUserId() } answers { userId }
        every { getAccessToken() } returns "access"
        every { getRefreshToken() } returns "refresh"
        every { getServerUrl() } answers { serverUrl }
    }
    private val deviceIdStore: DeviceIdStore = mockk {
        every { id() } returns DEVICE_ID
    }
    private val platform = FakePlatform()
    private val fcm = FakeTransport(TransportKind.FCM, "fcm-token-1")
    private val up = FakeTransport(TransportKind.UNIFIED_PUSH, "https://push.example.com/up/abc")
    private val store = MemoryStore()
    private val scheduler = FakeScheduler()

    private fun TestScope.newRegistrar(authState: AuthState = AuthState(backgroundScope)) = PushRegistrar(
        pushApi = api,
        authState = authState,
        tokenStorage = tokenStorage,
        deviceIdStore = deviceIdStore,
        platform = platform,
        transports = setOf(fcm, up),
        store = store,
        scheduler = scheduler,
        scope = backgroundScope,
    )

    private fun fcmRegistration(token: String) = PushRegistration("fcm", token, DEVICE_ID, LABEL)

    @Test
    fun `login registers fcm token with device id`() = runTest {
        val authState = AuthState(backgroundScope)
        val registrar = newRegistrar(authState)
        registrar.start()

        authState.setLoggedIn(USER)
        runCurrent()

        coVerify(exactly = 1) { api.register(fcmRegistration("fcm-token-1")) }
        assertEquals(PushStatus.Active(TransportKind.FCM, null), registrar.status.value)
        assertEquals("fcm", store.record?.provider)
    }

    @Test
    fun `token rotation re-registers same device`() = runTest {
        val registrar = newRegistrar()
        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())

        assertEquals(RefreshOutcome.REGISTERED, registrar.onEndpoint(TransportKind.FCM, "fcm-token-2"))
        registrar.onNewFcmToken("fcm-token-3")
        runCurrent()

        coVerify(exactly = 1) { api.register(fcmRegistration("fcm-token-1")) }
        coVerify(exactly = 1) { api.register(fcmRegistration("fcm-token-2")) }
        coVerify(exactly = 1) { api.register(fcmRegistration("fcm-token-3")) }
    }

    @Test
    fun `token refresh before any pass schedules the worker`() = runTest {
        newRegistrar().onNewFcmToken("fcm-token-2")

        assertEquals(1, scheduler.scheduled)
        coVerify(exactly = 0) { api.register(any()) }
    }

    @Test
    fun `unchanged registration is not sent again`() = runTest {
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())
        assertEquals(RefreshOutcome.UNCHANGED, registrar.refresh())
        registrar.onPermissionChanged()
        runCurrent()

        coVerify(exactly = 1) { api.register(any()) }
    }

    @Test
    fun `stale registration is sent again`() = runTest {
        val registrar = newRegistrar()
        var now = 1_000L
        registrar.now = { now }
        registrar.refresh()

        now += PushRegistrar.STALE_AFTER_MS
        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())

        coVerify(exactly = 2) { api.register(fcmRegistration("fcm-token-1")) }
    }

    @Test
    fun `permission denied registers nothing and reports off`() = runTest {
        platform.allowed = false
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.OFF, registrar.refresh())

        coVerify(exactly = 0) { api.getConfig() }
        coVerify(exactly = 0) { api.register(any()) }
        assertEquals(0, fcm.requests)
        assertEquals(PushStatus.Off(PushOffReason.PERMISSION_DENIED), registrar.status.value)
    }

    @Test
    fun `permission granted later registers`() = runTest {
        platform.allowed = false
        val registrar = newRegistrar()
        registrar.refresh()

        platform.allowed = true
        registrar.onPermissionChanged()
        runCurrent()

        coVerify(exactly = 1) { api.register(fcmRegistration("fcm-token-1")) }
    }

    @Test
    fun `registration network failure schedules retry`() = runTest {
        coEvery { api.register(any()) } throws IOException("offline")
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.RETRY, registrar.refresh())

        assertEquals(1, scheduler.scheduled)
        assertNull(store.record)
    }

    @Test
    fun `config network failure schedules retry`() = runTest {
        coEvery { api.getConfig() } throws IOException("offline")

        assertEquals(RefreshOutcome.RETRY, newRegistrar().refresh())
        assertEquals(1, scheduler.scheduled)
    }

    @Test
    fun `worker pass does not schedule itself again`() = runTest {
        coEvery { api.register(any()) } throws IOException("offline")

        assertEquals(RefreshOutcome.RETRY, newRegistrar().refresh(scheduleRetry = false))
        assertEquals(0, scheduler.scheduled)
    }

    @Test
    fun `validation error on registration is not retried`() = runTest {
        coEvery { api.register(any()) } throws
            ApiException(HttpStatusCode.BadRequest, "endpoint is too long", "VALIDATION_ERROR")
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.FAILED, registrar.refresh())

        assertEquals(0, scheduler.scheduled)
        assertNull(store.record)
        assertEquals(PushStatus.Off(PushOffReason.ERROR), registrar.status.value)
    }

    @Test
    fun `server error on registration is retried`() = runTest {
        coEvery { api.register(any()) } throws
            ApiException(HttpStatusCode.ServiceUnavailable, "unavailable", null)

        assertEquals(RefreshOutcome.RETRY, newRegistrar().refresh())
        assertEquals(1, scheduler.scheduled)
    }

    @Test
    fun `apk without firebase config never asks fcm for a token`() = runTest {
        platform.project = null
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.OFF, registrar.refresh())

        assertEquals(0, fcm.requests)
        coVerify(exactly = 0) { api.register(any()) }
        assertEquals(PushStatus.Off(PushOffReason.NO_TRANSPORT), registrar.status.value)
    }

    @Test
    fun `transport switch unregisters previous transport`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        assertEquals(PushStatus.Active(TransportKind.FCM, null), registrar.status.value)

        coEvery { api.getConfig() } returns upOnlyConfig
        platform.installed = listOf(DISTRIBUTOR)
        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())

        runCurrent()
        assertEquals(1, fcm.unregisters)
        assertEquals(0, up.unregisters)
        coVerify(exactly = 1) {
            api.register(PushRegistration("unifiedpush", up.endpoint, DEVICE_ID, LABEL))
        }
        assertEquals(PushStatus.Active(TransportKind.UNIFIED_PUSH, DISTRIBUTOR), registrar.status.value)
    }

    @Test
    fun `server dropping fcm turns push off and gives up the token`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()

        coEvery { api.getConfig() } returns upOnlyConfig
        assertEquals(RefreshOutcome.OFF, registrar.refresh())
        runCurrent()

        assertEquals(1, fcm.unregisters)
        assertNull(store.record)
        assertEquals(PushStatus.Off(PushOffReason.NO_TRANSPORT), registrar.status.value)
    }

    @Test
    fun `signed out registers nothing`() = runTest {
        userId = null

        assertEquals(RefreshOutcome.NOT_LOGGED_IN, newRegistrar().refresh())
        coVerify(exactly = 0) { api.getConfig() }
    }

    @Test
    fun `logout deletes this device and forgets the registration`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        assertNotNull(store.record)

        registrar.unregisterForLogout()
        runCurrent()

        coVerify(exactly = 1) { api.unregisterDevice(DEVICE_ID) }
        assertNull(store.record)
        assertEquals(PushStatus.Unknown, registrar.status.value)
        assertEquals(1, fcm.unregisters)
        assertEquals(1, scheduler.cancelled)
    }

    @Test
    fun `logout failure still cleans up locally`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        coEvery { api.unregisterDevice(any()) } throws IOException("offline")

        registrar.unregisterForLogout()

        assertNull(store.record)
        assertEquals(PushStatus.Unknown, registrar.status.value)
    }

    @Test
    fun `logout is bounded when the server hangs`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        coEvery { api.unregisterDevice(any()) } coAnswers { awaitCancellation() }

        registrar.unregisterForLogout()

        assertTrue(currentTime <= PushRegistrar.LOGOUT_LOCK_WAIT_MS + PushRegistrar.LOGOUT_DELETE_TIMEOUT_MS)
        assertNull(store.record)
    }

    @Test
    fun `next login after logout registers again`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        registrar.unregisterForLogout()
        runCurrent()

        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())
        coVerify(exactly = 2) { api.register(fcmRegistration("fcm-token-1")) }
    }

    @Test
    fun `re-login waits for the logout token deletion before requesting a token`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        val gate = CompletableDeferred<Unit>()
        fcm.unregisterGate = gate
        registrar.unregisterForLogout()

        val pass = async { registrar.refresh() }
        runCurrent()
        assertEquals(1, fcm.requests) // only the first login's request so far

        gate.complete(Unit)
        runCurrent()

        assertEquals(RefreshOutcome.REGISTERED, pass.await())
        assertEquals(listOf("request", "unregistered", "request"), fcm.events)
    }

    @Test
    fun `stalled unregister of the old transport does not hold the lock`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh() // FCM
        fcm.unregisterGate = CompletableDeferred() // never completes
        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig

        // The switching pass itself returns at once (UnifiedPush) or after the join cap at most.
        val pass = async { registrar.refresh() }
        advanceTimeBy(PushRegistrar.UNREGISTER_JOIN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(RefreshOutcome.REGISTERED, pass.await())

        // A later pass and the logout are not blocked by the stalled task.
        val before = currentTime
        assertEquals(RefreshOutcome.UNCHANGED, registrar.refresh())
        registrar.unregisterForLogout()
        assertEquals(before, currentTime)
        coVerify(exactly = 1) { api.unregisterDevice(DEVICE_ID) }
    }

    @Test
    fun `waiting for the logout token deletion is bounded`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()
        fcm.unregisterGate = CompletableDeferred() // never completes
        registrar.unregisterForLogout()

        val pass = async { registrar.refresh() }
        advanceTimeBy(PushRegistrar.UNREGISTER_JOIN_TIMEOUT_MS + 1)
        runCurrent()

        assertEquals(RefreshOutcome.REGISTERED, pass.await())
        assertEquals(2, fcm.requests)
    }

    @Test
    fun `logout during the registration request rolls the row back`() = runTest {
        val registrar = newRegistrar()
        coEvery { api.register(any()) } coAnswers {
            backgroundScope.launch { registrar.unregisterForLogout() }
            // The logout times out on the lock, deletes and cleans up meanwhile.
            delay(PushRegistrar.LOGOUT_LOCK_WAIT_MS + 500)
        }

        assertEquals(RefreshOutcome.NOT_LOGGED_IN, registrar.refresh())

        assertNull(store.record)
        // The logout's DELETE plus the rollback after the late response.
        coVerify(exactly = 2) { api.unregisterDevice(DEVICE_ID) }
    }

    @Test
    fun `account change during the registration request saves nothing and rolls back`() = runTest {
        coEvery { api.register(any()) } coAnswers { userId = "user-2" }
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.NOT_LOGGED_IN, registrar.refresh())

        assertNull(store.record)
        coVerify(exactly = 1) { api.unregisterDevice(DEVICE_ID) }
        assertEquals(PushStatus.Unknown, registrar.status.value)
    }

    @Test
    fun `server change during the registration request saves nothing`() = runTest {
        coEvery { api.register(any()) } coAnswers { serverUrl = "https://other.example.com" }

        assertEquals(RefreshOutcome.NOT_LOGGED_IN, newRegistrar().refresh())

        assertNull(store.record)
        coVerify(exactly = 1) { api.unregisterDevice(DEVICE_ID) }
    }

    @Test
    fun `signed out by the time the response arrives sends no rollback`() = runTest {
        coEvery { api.register(any()) } coAnswers { userId = null }

        assertEquals(RefreshOutcome.NOT_LOGGED_IN, newRegistrar().refresh())

        coVerify(exactly = 0) { api.unregisterDevice(any()) }
    }

    @Test
    fun `stalled token request times out and releases the lock`() = runTest {
        fcm.hang = true
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.RETRY, registrar.refresh())
        assertEquals(PushRegistrar.ENDPOINT_TIMEOUT_MS, currentTime)
        assertEquals(1, scheduler.scheduled)

        fcm.hang = false
        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())
    }

    @Test
    fun `unifiedpush endpoint reports the distributor`() = runTest {
        platform.installed = listOf(DISTRIBUTOR)
        platform.saved = DISTRIBUTOR
        coEvery { api.getConfig() } returns upOnlyConfig
        val registrar = newRegistrar()

        assertEquals(
            RefreshOutcome.REGISTERED,
            registrar.onEndpoint(TransportKind.UNIFIED_PUSH, "https://push.example.com/up/xyz"),
        )

        assertEquals(PushStatus.Active(TransportKind.UNIFIED_PUSH, DISTRIBUTOR), registrar.status.value)
    }

    @Test
    fun `unifiedpush selection remembers the chosen distributor`() = runTest {
        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig
        val registrar = newRegistrar()

        registrar.refresh()

        assertEquals(DISTRIBUTOR, platform.saved)
    }

    @Test
    fun `non-https unifiedpush endpoint is rejected locally`() = runTest {
        platform.installed = listOf(DISTRIBUTOR)
        platform.saved = DISTRIBUTOR
        val registrar = newRegistrar()

        for (bad in listOf("http://push.example.com/up/abc", "not a url", "https:///nohost", "ftp://x.example/a")) {
            assertEquals(RefreshOutcome.FAILED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, bad))
        }

        coVerify(exactly = 0) { api.register(any()) }
        assertEquals(PushStatus.Off(PushOffReason.ERROR), registrar.status.value)
    }

    @Test
    fun `over-long unifiedpush endpoint is rejected locally`() = runTest {
        val registrar = newRegistrar()
        val tooLong = "https://push.example.com/" + "a".repeat(2048)

        assertEquals(RefreshOutcome.FAILED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, tooLong))

        coVerify(exactly = 0) { api.register(any()) }
    }

    private suspend fun TestScope.registrarOnUnifiedPush(): PushRegistrar {
        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig
        val registrar = newRegistrar()
        assertEquals(RefreshOutcome.REGISTERED, registrar.refresh())
        return registrar
    }

    @Test
    fun `distributor failures back off and stop after the limit`() = runTest {
        val registrar = registrarOnUnifiedPush()

        repeat(PushRegistrar.MAX_UP_FAILURES + 2) {
            registrar.onTransportRegistrationFailed()
            runCurrent()
        }

        assertEquals(listOf(30L, 60L, 120L, 240L, 480L), scheduler.delays)
        assertEquals(0, scheduler.scheduled)
        assertEquals(PushStatus.Off(PushOffReason.ERROR), registrar.status.value)
    }

    @Test
    fun `successful registration resets the failure count`() = runTest {
        val registrar = registrarOnUnifiedPush()
        registrar.onTransportRegistrationFailed()
        runCurrent()

        assertEquals(RefreshOutcome.REGISTERED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, "https://push.example.com/up/new"))
        registrar.onTransportRegistrationFailed()
        runCurrent()

        assertEquals(listOf(30L, 30L), scheduler.delays)
    }

    @Test
    fun `distributor failure is ignored on fcm and when logged out`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()

        registrar.onTransportRegistrationFailed()
        runCurrent()
        assertEquals(PushStatus.Active(TransportKind.FCM, null), registrar.status.value)
        assertTrue(scheduler.delays.isEmpty())

        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig
        registrar.refresh()
        userId = null
        registrar.onTransportRegistrationFailed()
        runCurrent()
        assertTrue(scheduler.delays.isEmpty())
    }

    @Test
    fun `endpoint that never arrives counts as a failure`() = runTest {
        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig
        up.silent = true
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.PENDING, registrar.refresh())
        advanceTimeBy(PushRegistrar.PENDING_ENDPOINT_TIMEOUT_MS - 1)
        runCurrent()
        assertTrue(scheduler.delays.isEmpty())
        advanceTimeBy(2)
        runCurrent()

        assertEquals(listOf(30L), scheduler.delays)
        assertEquals(PushStatus.Off(PushOffReason.ERROR), registrar.status.value)
    }

    @Test
    fun `late unifiedpush endpoint after switching to fcm registers nothing`() = runTest {
        val registrar = newRegistrar()
        registrar.refresh()

        registrar.onEndpointAsync(TransportKind.UNIFIED_PUSH, "https://push.example.com/up/late")
        runCurrent()

        coVerify(exactly = 0) { api.register(match { it.provider == "unifiedpush" }) }
        assertEquals(PushStatus.Active(TransportKind.FCM, null), registrar.status.value)
    }

    @Test
    fun `unifiedpush endpoint while unifiedpush is active registers`() = runTest {
        val registrar = registrarOnUnifiedPush()

        registrar.onEndpointAsync(TransportKind.UNIFIED_PUSH, "https://push.example.com/up/rotated")
        runCurrent()

        coVerify(exactly = 1) { api.register(PushRegistration("unifiedpush", "https://push.example.com/up/rotated", DEVICE_ID, LABEL)) }
    }

    @Test
    fun `transport without distributor turns push off instead of failing`() = runTest {
        platform.installed = listOf(DISTRIBUTOR)
        coEvery { api.getConfig() } returns upOnlyConfig
        up.failWith = NoDistributorException()
        val registrar = newRegistrar()

        assertEquals(RefreshOutcome.OFF, registrar.refresh())

        assertEquals(PushStatus.Off(PushOffReason.NO_DISTRIBUTOR), registrar.status.value)
    }

    @Test
    fun `endpoint size limit is 2048 bytes and underscore hosts are accepted`() = runTest {
        val registrar = newRegistrar()
        val prefix = "https://push.example.com/"

        assertEquals(RefreshOutcome.REGISTERED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, prefix + "a".repeat(2048 - prefix.length)))
        assertEquals(RefreshOutcome.FAILED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, prefix + "a".repeat(2049 - prefix.length)))
        assertEquals(RefreshOutcome.REGISTERED, registrar.onEndpoint(TransportKind.UNIFIED_PUSH, "https://my_ntfy.example.com/up/abc"))
    }

    @Test
    fun `registration toString hides the endpoint`() {
        val text = fcmRegistration("secret-fcm-token").toString()
        assertTrue(text, !text.contains("secret-fcm-token"))
    }

    private companion object {
        const val PROJECT = "kaiku-prod"
        const val USER = "user-1"
        const val SERVER = "https://kaiku.example.com"
        const val DEVICE_ID = "6f1c2b9e-3d4a-4e5f-8a7b-9c0d1e2f3a4b"
        const val LABEL = "Pixel 8"
        const val DISTRIBUTOR = "io.heckel.ntfy"
    }
}
