package io.wolftown.kaiku.ui.auth

import app.cash.turbine.test
import io.ktor.http.HttpStatusCode
import io.mockk.*
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LoginViewModelTest {

    private lateinit var authRepository: AuthRepository
    private lateinit var oidcHandler: OidcHandler
    private lateinit var viewModel: LoginViewModel

    private val testDispatcher = StandardTestDispatcher()

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        authRepository = mockk(relaxed = true)
        oidcHandler = mockk(relaxed = true)
        viewModel = LoginViewModel(authRepository, oidcHandler)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    // ========================================================================
    // 1. Initial state has empty fields and no errors
    // ========================================================================

    @Test
    fun `initial state has empty fields and no errors`() = runTest {
        val state = viewModel.uiState.value
        assertEquals("", state.username)
        assertEquals("", state.password)
        assertEquals("", state.mfaCode)
        assertFalse(state.isLoading)
        assertNull(state.error)
        assertFalse(state.mfaRequired)
        assertFalse(state.loginSuccess)
    }

    // ========================================================================
    // 2. onUsernameChanged / onPasswordChanged update state
    // ========================================================================

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
    fun `ui state toString hides password and mfa code`() {
        val text = LoginUiState(username = "anna", password = "temp-pass-1", mfaCode = "123456").toString()

        assertTrue(text.contains("anna"))
        assertFalse(text.contains("temp-pass-1"))
        assertFalse(text.contains("123456"))
    }

    @Test
    fun `onMfaCodeChanged updates mfaCode in state`() = runTest {
        viewModel.onMfaCodeChanged("123456")
        assertEquals("123456", viewModel.uiState.value.mfaCode)
    }

    // ========================================================================
    // 3. Successful login sets loginSuccess = true
    // ========================================================================

    @Test
    fun `successful login sets loginSuccess to true`() = runTest {
        val user = User(
            id = "user-1",
            username = "testuser",
            displayName = "Test User"
        )
        coEvery { authRepository.login("testuser", "pass", null) } returns Result.success(user)

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("pass")
        viewModel.onLoginClicked()

        // Advance coroutines to let login complete
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.loginSuccess)
        assertFalse(state.isLoading)
        assertNull(state.error)
    }

    // ========================================================================
    // 4. Failed login sets error message
    // ========================================================================

    @Test
    fun `failed login sets error message`() = runTest {
        coEvery {
            authRepository.login("testuser", "wrong", null)
        } returns Result.failure(ApiException(HttpStatusCode.Unauthorized, "server text", code = "INVALID_CREDENTIALS"))

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("wrong")
        viewModel.onLoginClicked()

        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertFalse(state.loginSuccess)
        assertFalse(state.isLoading)
        // C16: copy comes from resources by status/code, never the server message.
        assertEquals(R.string.auth_invalid_credentials, state.error)
    }

    @Test
    fun `network failure shows generic login error`() = runTest {
        coEvery {
            authRepository.login("testuser", "pass", null)
        } returns Result.failure(java.io.IOException("timeout"))

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("pass")
        viewModel.onLoginClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_login_failed, viewModel.uiState.value.error)
    }

    // ========================================================================
    // C4: forced password change
    // ========================================================================

    @Test
    fun `login view model flags password change`() = runTest {
        coEvery {
            authRepository.login("anna", "temp-pass-1", null)
        } returns Result.failure(PasswordChangeRequiredException())

        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("temp-pass-1")
        viewModel.onLoginClicked()
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.passwordChangeRequired)
        assertFalse(state.isLoading)
        assertFalse(state.loginSuccess)
        assertNull(state.error)
        // The temporary password is not kept in the login form.
        assertEquals("", state.password)

        viewModel.onPasswordChangeNavigated()
        assertFalse(viewModel.uiState.value.passwordChangeRequired)
    }

    // ========================================================================
    // 5. MFA required shows mfaRequired state
    // ========================================================================

    @Test
    fun `mfa required sets mfaRequired state`() = runTest {
        coEvery {
            authRepository.login("testuser", "pass", null)
        } returns Result.failure(MfaRequiredException())

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("pass")
        viewModel.onLoginClicked()

        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.mfaRequired)
        assertFalse(state.isLoading)
        assertNull(state.error)
    }

    // ========================================================================
    // 6. Login with MFA code succeeds
    // ========================================================================

    @Test
    fun `login with mfa code succeeds`() = runTest {
        val user = User(
            id = "user-1",
            username = "testuser",
            displayName = "Test User"
        )
        // First login attempt triggers MFA
        coEvery {
            authRepository.login("testuser", "pass", null)
        } returns Result.failure(MfaRequiredException())
        // Second login attempt with MFA code succeeds
        coEvery {
            authRepository.login("testuser", "pass", "123456")
        } returns Result.success(user)

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("pass")

        // Trigger MFA requirement
        viewModel.onLoginClicked()
        advanceUntilIdle()

        assertTrue(viewModel.uiState.value.mfaRequired)

        // Enter MFA code and login again
        viewModel.onMfaCodeChanged("123456")
        viewModel.onLoginClicked()
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.loginSuccess)
        assertFalse(state.isLoading)
    }

    // ========================================================================
    // 7. Loading state is true during login, false after
    // ========================================================================

    @Test
    fun `loading state is true during login and false after`() = runTest {
        val user = User(
            id = "user-1",
            username = "testuser",
            displayName = "Test User"
        )
        coEvery { authRepository.login("testuser", "pass", null) } returns Result.success(user)

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("pass")

        viewModel.uiState.test {
            awaitItem() // current state with username+password set

            viewModel.onLoginClicked()

            // Loading should be true immediately (synchronous update before launch body runs)
            val loadingState = awaitItem()
            assertTrue("Expected loading to be true", loadingState.isLoading)

            // Advance coroutines to let login complete
            testScheduler.advanceUntilIdle()

            // Loading should be false after completion
            val finalState = awaitItem()
            assertFalse("Expected loading to be false after login", finalState.isLoading)
            assertTrue(finalState.loginSuccess)
        }
    }

    // ========================================================================
    // clearError resets error to null
    // ========================================================================

    @Test
    fun `clearError resets error to null`() = runTest {
        coEvery {
            authRepository.login("testuser", "wrong", null)
        } returns Result.failure(Exception("boom"))

        viewModel.onUsernameChanged("testuser")
        viewModel.onPasswordChanged("wrong")
        viewModel.onLoginClicked()
        advanceUntilIdle()

        assertNotNull(viewModel.uiState.value.error)

        viewModel.clearError()

        assertNull(viewModel.uiState.value.error)
    }
}
