package io.wolftown.kaiku.ui.auth

import io.mockk.*
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.api.OidcProvider
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.auth.PasskeyAuth
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * The login form (Task B14). Password, phone and SSO sign-ins run in
 * [LoginAttemptViewModel] (LoginAttemptViewModelTest, OidcHandlerTest); LP's
 * passkey sign-in has LoginViewModelPasskeyTest.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class LoginViewModelTest {

    private lateinit var authRepository: AuthRepository
    private lateinit var authApi: AuthApi
    private lateinit var tokenStorage: TokenStorage
    private lateinit var serverProbe: ServerProbe
    private lateinit var passkeyAuth: PasskeyAuth
    private lateinit var viewModel: LoginViewModel

    private val testDispatcher = StandardTestDispatcher()

    private fun newViewModel() = LoginViewModel(authRepository, authApi, tokenStorage, serverProbe, passkeyAuth)

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        authRepository = mockk(relaxed = true)
        authApi = mockk(relaxed = true)
        coEvery { authApi.getOidcProviders() } returns emptyList()
        tokenStorage = mockk(relaxed = true)
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
        serverProbe = mockk()
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Ok(registrationOpen = false)
        passkeyAuth = mockk(relaxed = true)
        viewModel = newViewModel()
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `initial state has empty fields and no errors`() = runTest {
        val state = viewModel.uiState.value
        assertEquals("", state.username)
        assertEquals("", state.password)
        assertFalse(state.isLoading)
        assertNull(state.errorRes)
        assertFalse(state.loginSuccess)
    }

    @Test
    fun `onUsernameChanged updates username in state`() = runTest {
        viewModel.onUsernameChanged("testuser")
        assertEquals("testuser", viewModel.uiState.value.username)
    }

    @Test
    fun `onPasswordChanged updates password in state`() = runTest {
        viewModel.onPasswordChanged("secret123")
        assertEquals("secret123", viewModel.uiState.value.password)
    }

    @Test
    fun `ui state toString hides the password`() {
        val text = LoginUiState(username = "anna", password = "temp-pass-1").toString()

        assertTrue(text.contains("anna"))
        assertFalse(text.contains("temp-pass-1"))
    }

    @Test
    fun `sso providers come from the server`() = runTest {
        coEvery { authApi.getOidcProviders() } returns listOf(OidcProvider("google", "Google"))
        val vm = newViewModel()
        advanceUntilIdle()
        assertEquals(listOf("google"), vm.uiState.value.oidcProviders.map { it.slug })

        // A server that cannot say shows no SSO buttons; the other sign-ins stay.
        coEvery { authApi.getOidcProviders() } throws java.io.IOException("offline")
        val offline = newViewModel()
        advanceUntilIdle()
        assertTrue(offline.uiState.value.oidcProviders.isEmpty())
    }

    @Test
    fun `lost phone and secure my account open the server's own pages`() {
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com/"
        assertEquals("https://kaiku.example.com/recover", viewModel.serverPageUrl("/recover"))
        assertEquals("https://kaiku.example.com/secure-account", viewModel.serverPageUrl("/secure-account"))
        every { tokenStorage.getServerUrl() } returns null
        assertNull(viewModel.serverPageUrl("/recover"))
    }

    // ========================================================================
    // First run: Register only when open, change server
    // ========================================================================

    @Test
    fun `register hidden when registration is closed`() = runTest {
        coEvery { serverProbe.probe("https://kaiku.example.com") } returns
            ProbeResult.Ok(registrationOpen = false)

        val vm = newViewModel()
        advanceUntilIdle()

        assertFalse(vm.uiState.value.registrationOpen)
        assertEquals("kaiku.example.com", vm.uiState.value.serverHost)
    }

    @Test
    fun `register shown when registration is open`() = runTest {
        coEvery { serverProbe.probe("https://kaiku.example.com") } returns
            ProbeResult.Ok(registrationOpen = true)

        val vm = newViewModel()
        advanceUntilIdle()

        assertTrue(vm.uiState.value.registrationOpen)
    }

    @Test
    fun `register stays hidden when the server cannot be checked`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Unreachable

        val vm = newViewModel()
        advanceUntilIdle()

        assertFalse(vm.uiState.value.registrationOpen)
    }

    @Test
    fun `retry finds Register after an offline first check`() = runTest {
        coEvery { serverProbe.probe(any()) } returnsMany listOf(
            ProbeResult.Unreachable,
            ProbeResult.Ok(registrationOpen = true)
        )
        val vm = newViewModel()
        advanceUntilIdle()
        assertTrue(vm.uiState.value.registrationCheckFailed)
        assertFalse(vm.uiState.value.registrationOpen)

        vm.onRetryRegistrationCheck()
        advanceUntilIdle()

        assertFalse(vm.uiState.value.registrationCheckFailed)
        assertTrue(vm.uiState.value.registrationOpen)
    }

    @Test
    fun `change server cancels a sign-in in flight`() = runTest {
        coEvery { passkeyAuth.loginWithPasskey(any(), any()) } coAnswers {
            kotlinx.coroutines.delay(10_000)
            Result.success(User(id = "u", username = "anna", displayName = "Anna"))
        }
        viewModel.onPasskeySignIn(mockk(relaxed = true))
        advanceTimeBy(100)

        viewModel.onChangeServerClicked()
        advanceUntilIdle()

        assertFalse(viewModel.uiState.value.loginSuccess)
        assertFalse(viewModel.uiState.value.isLoading)
    }

    @Test
    fun `change server returns to server screen keeping nothing stale`() = runTest {
        coEvery { serverProbe.probe(any()) } returns ProbeResult.Ok(registrationOpen = true)
        coEvery { authApi.getOidcProviders() } returns listOf(OidcProvider("google", "Google"))
        val vm = newViewModel()
        advanceUntilIdle()
        vm.onUsernameChanged("anna")
        vm.onPasswordChanged("secret")
        assertTrue(vm.uiState.value.registrationOpen)

        vm.onChangeServerClicked()
        advanceUntilIdle()

        val state = vm.uiState.value
        assertTrue(state.changeServerRequested)
        assertEquals("", state.username)
        assertEquals("", state.password)
        assertFalse(state.registrationOpen)
        assertTrue(state.oidcProviders.isEmpty())
        assertEquals("", state.serverHost)
        assertNull(state.errorRes)
        // The pending password change and any sign-in in flight belong to the old server.
        verify { authRepository.cancelPasswordChange() }

        vm.onChangeServerNavigated()
        assertFalse(vm.uiState.value.changeServerRequested)
    }

    @Test
    fun `the typed password leaves the form once its sign-in is done`() = runTest {
        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("secret")
        viewModel.clearPassword()
        assertEquals("", viewModel.uiState.value.password)
        assertEquals("anna", viewModel.uiState.value.username)
    }
}
