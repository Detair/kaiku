package io.wolftown.kaiku.ui.settings

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.push.PushOffReason
import io.wolftown.kaiku.push.PushPlatform
import io.wolftown.kaiku.push.PushPreferences
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.push.PushStatus
import io.wolftown.kaiku.push.RefreshOutcome
import io.wolftown.kaiku.push.TransportKind
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SettingsViewModelTest {

    private val status = MutableStateFlow<PushStatus>(PushStatus.Unknown)
    private val registrar: PushRegistrar = mockk {
        every { this@mockk.status } returns this@SettingsViewModelTest.status
        coEvery { refresh(any()) } returns RefreshOutcome.OFF
    }
    private var installed = emptyList<String>()
    private var gms = true
    private val platform: PushPlatform = mockk {
        every { distributors() } answers { installed }
        every { gmsAvailable() } answers { gms }
    }
    private var prefer = false
    private var picked: String? = null
    private val preferences: PushPreferences = mockk {
        every { preferUnifiedPush } answers { prefer }
        every { preferUnifiedPush = any() } answers { prefer = firstArg() }
        every { distributor } answers { picked }
        every { distributor = any() } answers { picked = firstArg() }
    }

    @Before
    fun setUp() {
        Dispatchers.setMain(UnconfinedTestDispatcher())
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    private fun viewModel() = SettingsViewModel(
        authRepository = mockk(), // unstubbed: loading the profile fails and is handled
        tokenStorage = mockk<TokenStorage>(relaxed = true),
        pushRegistrar = registrar,
        pushPlatform = platform,
        pushPreferences = preferences,
    )

    @Test
    fun `settings shows unified push switch only with distributor and gms`() {
        installed = emptyList()
        assertFalse(viewModel().notificationState.value.showUnifiedPushSwitch)

        installed = listOf("io.heckel.ntfy")
        gms = false
        assertFalse(viewModel().notificationState.value.showUnifiedPushSwitch)

        gms = true
        val state = viewModel().notificationState.value
        assertTrue(state.showUnifiedPushSwitch)
        assertFalse(state.showDistributorPicker)
    }

    @Test
    fun `picker shows with more than one distributor`() {
        installed = listOf("io.heckel.ntfy", "org.example.other")
        assertTrue(viewModel().notificationState.value.showDistributorPicker)
    }

    @Test
    fun `status follows the registrar`() {
        val vm = viewModel()
        status.value = PushStatus.Active(TransportKind.FCM, null)
        assertEquals(PushStatus.Active(TransportKind.FCM, null), vm.notificationState.value.status)
        status.value = PushStatus.Off(PushOffReason.PERMISSION_DENIED)
        assertEquals(PushStatus.Off(PushOffReason.PERMISSION_DENIED), vm.notificationState.value.status)
    }

    @Test
    fun `toggling unified push re-runs registration`() {
        installed = listOf("io.heckel.ntfy")
        val vm = viewModel()

        vm.setPreferUnifiedPush(true)

        assertTrue(prefer)
        assertTrue(vm.notificationState.value.preferUnifiedPush)
        coVerify(exactly = 1) { registrar.refresh(any()) }
    }

    @Test
    fun `picking a distributor stores it and re-runs registration`() {
        installed = listOf("io.heckel.ntfy", "org.example.other")
        val vm = viewModel()

        vm.pickDistributor("org.example.other")

        assertEquals("org.example.other", picked)
        assertEquals("org.example.other", vm.notificationState.value.selectedDistributor)
        coVerify(exactly = 1) { registrar.refresh(any()) }
    }
}
