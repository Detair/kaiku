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
import kotlinx.coroutines.ExperimentalCoroutinesApi
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

        override suspend fun requestEndpoint(onEndpoint: suspend (String) -> Unit) {
            requests++
            failWith?.let { throw it }
            onEndpoint(endpoint)
        }

        override suspend fun unregister() {
            unregisters++
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
        override fun savedDistributor(): String? = null
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
        override fun schedule() {
            scheduled++
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
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getUserId() } answers { userId }
        every { getAccessToken() } returns "access"
        every { getRefreshToken() } returns "refresh"
        every { getServerUrl() } returns SERVER
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
