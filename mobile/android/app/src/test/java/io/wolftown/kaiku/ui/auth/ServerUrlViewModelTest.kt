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

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        tokenStorage = mockk(relaxed = true)
        every { tokenStorage.getServerUrl() } returns null
        authRepository = mockk(relaxed = true)
        serverProbe = mockk()
        viewModel = ServerUrlViewModel(tokenStorage, authRepository, serverProbe)
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

        val vm = ServerUrlViewModel(tokenStorage, authRepository, serverProbe)

        assertEquals("https://other.example", vm.uiState.value.url)
    }

    @Test
    fun `back is offered only when a server is already saved`() {
        assertFalse(viewModel.canGoBack)
        every { tokenStorage.getServerUrl() } returns "https://other.example"

        assertTrue(ServerUrlViewModel(tokenStorage, authRepository, serverProbe).canGoBack)
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
            tokenStorage.clearOidcPkceState()
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
}
