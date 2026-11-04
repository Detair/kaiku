package io.wolftown.kaiku.ui.settings

import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.auth.DeviceStepUpProvider
import io.wolftown.kaiku.data.auth.PasskeyStepUp
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.domain.model.Passkey
import io.wolftown.kaiku.domain.model.PasskeyList
import io.wolftown.kaiku.domain.model.SessionInfo
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import java.util.Optional
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Fable m5: with L2's provider bound, LP's "Add a passkey" shows only where a
 * step-up can work: a full session with the key on this phone or a passkey.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PasskeysAddAvailabilityTest {
    private val api: PasskeyApi = mockk(relaxed = true)
    private val authApi: AuthApi = mockk()
    private val active = Passkey("p-1", "Pixel 8", "active", "2026-11-01T10:00:00Z")

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm(assurance: String, keyPhone: Boolean, passkeys: List<Passkey>): PasskeysViewModel {
        val authState = AuthState(backgroundScope).apply { setLoggedIn("u1") }
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), SoftwareApprovalKeyStore(), authState, backgroundScope)
        if (keyPhone) {
            repo.saveClaimed(LocalKeyDevice("kd1", "u1", "https://chat.example.com", UvClass.PER_USE, "e1", null))
            repo.markActive("u1", "kd1")
        }
        coEvery { authApi.getMe() } returns User("u1", "anna", "Anna", session = SessionInfo("s1", assurance))
        coEvery { api.list() } returns PasskeyList("chat.example.com", "dQ", passkeys)
        val passkeyStepUp = PasskeyStepUp(api, mockk(relaxed = true))
        val device = DeviceStepUp(FakeApprovalApi(), mockk(), repo, { "d1" }, authApi)
        return PasskeysViewModel(api, mockk(relaxed = true), passkeyStepUp, Optional.of(DeviceStepUpProvider(device, passkeyStepUp)))
    }

    @Test fun `a probation session on a phone without a key offers no add`() = runTest {
        assertFalse(vm("probation", keyPhone = false, passkeys = emptyList()).state.value.canAdd)
        assertFalse(vm("probation", keyPhone = false, passkeys = listOf(active)).state.value.canAdd)
    }

    @Test fun `a full session without key or passkey offers no add`() = runTest {
        assertFalse(vm("full", keyPhone = false, passkeys = emptyList()).state.value.canAdd)
    }

    @Test fun `the key phone or a passkey in a full session offers add`() = runTest {
        assertTrue(vm("full", keyPhone = true, passkeys = emptyList()).state.value.canAdd)
        assertTrue(vm("full", keyPhone = false, passkeys = listOf(active)).state.value.canAdd)
    }
}
