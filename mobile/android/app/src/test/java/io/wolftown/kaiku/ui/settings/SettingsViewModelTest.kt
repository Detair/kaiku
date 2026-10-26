package io.wolftown.kaiku.ui.settings

import io.mockk.Runs
import io.mockk.coEvery
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.UserStatus
import io.mockk.just
import io.mockk.verify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.push.PushOffReason
import io.wolftown.kaiku.push.PushPlatform
import io.wolftown.kaiku.push.PushPreferences
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.push.PushStatus
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
        every { refreshAsync() } just Runs
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
    fun `picker shows with more than one distributor and unifiedpush in use`() {
        installed = listOf("io.heckel.ntfy", "org.example.other")
        assertFalse(viewModel().notificationState.value.showDistributorPicker)

        prefer = true
        assertTrue(viewModel().notificationState.value.showDistributorPicker)

        prefer = false
        gms = false
        assertTrue(viewModel().notificationState.value.showDistributorPicker)
    }

    @Test
    fun `opening settings refreshes only when push is not active or distributor vanished`() {
        installed = listOf("io.heckel.ntfy")
        picked = "io.heckel.ntfy"
        status.value = PushStatus.Active(TransportKind.UNIFIED_PUSH, "io.heckel.ntfy")
        val vm = viewModel()

        vm.reloadPushState()
        verify(exactly = 0) { registrar.refreshAsync() }

        installed = emptyList()
        vm.reloadPushState()
        verify(exactly = 1) { registrar.refreshAsync() }
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
        verify(exactly = 1) { registrar.refreshAsync() }
    }

    @Test
    fun `picking a distributor stores it and re-runs registration`() {
        installed = listOf("io.heckel.ntfy", "org.example.other")
        val vm = viewModel()

        vm.pickDistributor("org.example.other")

        assertEquals("org.example.other", picked)
        assertEquals("org.example.other", vm.notificationState.value.selectedDistributor)
        verify(exactly = 1) { registrar.refreshAsync() }
    }

    @Test
    fun `load failure exposes a string resource`() {
        val repo: AuthRepository = mockk()
        coEvery { repo.getCurrentUser() } returns Result.failure(IllegalStateException("raw server text"))
        val vm = SettingsViewModel(
            authRepository = repo,
            tokenStorage = mockk<TokenStorage>(relaxed = true),
            pushRegistrar = registrar,
            pushPlatform = platform,
            pushPreferences = preferences,
        )
        assertEquals(R.string.settings_profile_load_failed, vm.error.value)
    }

    @Test
    fun `an unexpected exception while loading also exposes the string resource`() {
        val vm = viewModel() // unstubbed repository throws
        assertEquals(R.string.settings_profile_load_failed, vm.error.value)
    }

    @Test
    fun `status maps to a label resource for every enum value`() {
        assertEquals(R.string.home_status_online, statusLabelRes(UserStatus.ONLINE))
        assertEquals(R.string.home_status_idle, statusLabelRes(UserStatus.IDLE))
        assertEquals(R.string.home_status_dnd, statusLabelRes(UserStatus.DND))
        assertEquals(R.string.settings_status_offline, statusLabelRes(UserStatus.OFFLINE))
        assertEquals(R.string.settings_status_unknown, statusLabelRes(UserStatus.UNKNOWN))
        assertEquals(5, UserStatus.entries.size) // a new value needs a mapping above
    }
}
