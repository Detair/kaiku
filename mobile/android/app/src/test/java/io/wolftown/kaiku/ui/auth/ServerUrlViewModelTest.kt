package io.wolftown.kaiku.ui.auth

import io.mockk.coEvery
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.mockk.verifyOrder
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ServerUrlViewModelTest {

    private lateinit var tokenStorage: TokenStorage
    private lateinit var authRepository: AuthRepository
    private lateinit var serverProbe: ServerProbe
    private lateinit var viewModel: ServerUrlViewModel
    private val keyStore = io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore()
    private lateinit var keyDevices: io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
    /** Whether the setup of this phone holds an attempt (as the setup owner reports it). */
    private var setupHolds = false
    private val serverAddress by lazy { io.wolftown.kaiku.ui.auth.setup.ServerAddressRule(keyDevices, { null }) { setupHolds } }

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        tokenStorage = mockk(relaxed = true)
        every { tokenStorage.getServerUrl() } returns null
        authRepository = mockk(relaxed = true)
        serverProbe = mockk()
        keyDevices = io.wolftown.kaiku.data.keydevice.KeyDeviceRepository(
            keyStore, io.wolftown.kaiku.approval.SoftwareApprovalKeyStore(),
            io.wolftown.kaiku.data.local.AuthState(kotlinx.coroutines.CoroutineScope(Dispatchers.Unconfined)),
            kotlinx.coroutines.CoroutineScope(Dispatchers.Unconfined),
        )
        viewModel = ServerUrlViewModel(tokenStorage, authRepository, serverProbe, serverAddress)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `starts with the default server filled in`() {
        assertTrue(viewModel.uiState.value.url.startsWith("https://"))
    }

    @Test
    fun `starts with the stored server when there is one`() {
        every { tokenStorage.getServerUrl() } returns "https://other.example"

        val vm = ServerUrlViewModel(tokenStorage, authRepository, serverProbe, serverAddress)

        assertEquals("https://other.example", vm.uiState.value.url)
    }

    @Test
    fun `back is offered only when a server is already saved`() {
        assertFalse(viewModel.canGoBack)
        every { tokenStorage.getServerUrl() } returns "https://other.example"

        assertTrue(ServerUrlViewModel(tokenStorage, authRepository, serverProbe, serverAddress).canGoBack)
    }

    @Test
    fun `plain address is probed and saved normalized`() = runTest {
        coEvery { serverProbe.probe("https://kaiku.example.com") } returns ProbeResult.Ok(true)

        viewModel.onUrlChanged("Kaiku.Example.com/")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        verifyOrder {
            authRepository.cancelPasswordChange()
            tokenStorage.clear()
            tokenStorage.saveServerUrl("https://kaiku.example.com")
        }
        assertTrue(viewModel.uiState.value.connectSuccess)
        assertFalse(viewModel.uiState.value.isChecking)
    }

    @Test
    fun `http address is rejected without a request`() = runTest {
        viewModel.onUrlChanged("http://kaiku.example.com")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_not_secure, viewModel.uiState.value.error)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
    }

    @Test
    fun `blank address shows an error`() = runTest {
        viewModel.onUrlChanged("  ")
        viewModel.onConnectClicked()

        assertEquals(R.string.auth_server_invalid_address, viewModel.uiState.value.error)
    }

    @Test
    fun `not a Kaiku server is not saved`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.NotKaiku

        viewModel.onUrlChanged("example.com")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_not_kaiku, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.connectSuccess)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        verify(exactly = 0) { authRepository.cancelPasswordChange() }
    }

    @Test
    fun `unreachable server is not saved`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Unreachable

        viewModel.onUrlChanged("example.com")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_unreachable, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.isChecking)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
    }

    private fun claimedRecord() = io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
        io.wolftown.kaiku.data.keydevice.LocalKeyDevice(null, "u1", "https://chat.example.com", io.wolftown.kaiku.approval.UvClass.PER_USE, "e1", "s"),
        io.wolftown.kaiku.data.keydevice.KeyDevicePhase.CLAIMED,
        keyId = "k1",
    )

    /** A key, or a setup that is not finished, ties the phone to its server: the address is not changed under it. */
    @Test
    fun `the server is not changed while this phone holds a key record`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Ok(true)
        keyStore.stored = claimedRecord()
        keyDevices.reload()

        viewModel.onUrlChanged("other.example.org")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_tied_to_key, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.connectSuccess)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        io.mockk.coVerify(exactly = 0) { serverProbe.probe(any()) }
    }

    @Test
    fun `nor while what it has saved about its key cannot be read`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Ok(true)
        keyStore.unreadable = true
        keyDevices.reload()

        viewModel.onUrlChanged("other.example.org")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_tied_to_key, viewModel.uiState.value.error)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
    }

    @Test
    fun `a key record that came while the server was being checked keeps the address too`() = runTest {
        coEvery { serverProbe.probe(any()) } coAnswers {
            keyStore.stored = claimedRecord()
            keyDevices.reload()
            ProbeResult.Ok(true)
        }

        viewModel.onUrlChanged("other.example.org")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_tied_to_key, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.isChecking)
        assertFalse(viewModel.uiState.value.connectSuccess)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
    }

    /**
     * Connect is tapped and the server is probed; meanwhile a setup link opens a setup. When the
     * probe answers, nothing is written (not the address, and the stored session is not cleared),
     * and the screen says why.
     */
    @Test
    fun `a probe that answers after a setup opened writes nothing`() = runTest {
        coEvery { serverProbe.probe(any()) } coAnswers {
            setupHolds = true
            ProbeResult.Ok(true)
        }

        viewModel.onUrlChanged("other.example.org")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_tied_to_key, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.isChecking)
        assertFalse(viewModel.uiState.value.connectSuccess)
        verify(exactly = 0) { tokenStorage.saveServerUrl(any()) }
        verify(exactly = 0) { tokenStorage.clear() }
        verify(exactly = 0) { authRepository.cancelPasswordChange() }
    }

    @Test
    fun `nor while a setup holds an attempt`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Ok(true)
        setupHolds = true

        viewModel.onUrlChanged("other.example.org")
        viewModel.onConnectClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_server_tied_to_key, viewModel.uiState.value.error)
        io.mockk.coVerify(exactly = 0) { serverProbe.probe(any()) }
    }

    /**
     * One rule, asked by both writers of the address: the server-address screen and a setup adopting
     * its link's server. Neither writes the address under a condition of its own.
     */
    @Test
    fun `the server-address screen and the setup ask the one rule`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val screen = java.io.File(root, "ui/auth/ServerUrlScreen.kt").readText()
            .substringAfter("class ServerUrlViewModel").substringBefore("@Composable")
        val setup = java.io.File(root, "ui/auth/setup/SetupFlowViewModel.kt").readText()
            .substringAfter("override fun adoptServer(").substringBefore("override fun savedServer")
        assertEquals(2, Regex("""serverAddress\.mayChange\(\)""").findAll(screen).count())
        assertTrue(setup.contains("serverAddress.mayChange(forOwnSetup = true)"))
        for (src in listOf(screen, setup)) {
            for (own in listOf("record.value", "unreadable()", "pinned()", "holdsAttempt()")) assertFalse(own, src.contains(own))
        }
    }

    /** What the rule refuses for: each condition alone. */
    @Test
    fun `the rule refuses for each of its conditions alone`() {
        var pinned: String? = null
        val rule = io.wolftown.kaiku.ui.auth.setup.ServerAddressRule(keyDevices, { pinned }) { setupHolds }
        assertTrue(rule.mayChange())
        pinned = "https://chat.example.com"
        assertFalse(rule.mayChange())
        assertFalse(rule.mayChange(forOwnSetup = true))
        pinned = null
        setupHolds = true
        assertFalse(rule.mayChange())
        assertTrue(rule.mayChange(forOwnSetup = true))
        setupHolds = false
        keyStore.stored = claimedRecord()
        keyDevices.reload()
        assertFalse(rule.mayChange())
        assertFalse(rule.mayChange(forOwnSetup = true))
        keyStore.stored = null
        keyStore.unreadable = true
        keyDevices.reload()
        assertFalse(rule.mayChange())
        assertFalse(rule.mayChange(forOwnSetup = true))
    }
}
