package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.mockk.coVerify
import io.mockk.every
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.RegistrationTicket
import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class RegisterViewModelTest {

    private lateinit var authRepository: AuthRepository
    private lateinit var enrollmentApi: EnrollmentApi
    private lateinit var viewModel: RegisterViewModel

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        authRepository = mockk(relaxed = true)
        enrollmentApi = mockk()
        val tokenStorage: TokenStorage = mockk { every { getServerUrl() } returns "https://chat.example.com" }
        viewModel = RegisterViewModel(authRepository, enrollmentApi, tokenStorage)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test
    fun `taken username exposes a string resource, not the server message`() = runTest {
        coEvery { authRepository.register("anna", "pw", null, null) } returns
            Result.failure(ApiException(HttpStatusCode.Conflict, "Username already exists", "USER_EXISTS"))

        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertEquals(R.string.auth_register_username_taken, state.errorRes)
        assertTrue(!state.isLoading)
    }

    @Test
    fun `unknown failure shows the generic registration copy and clearError resets it`() = runTest {
        coEvery { authRepository.register("anna", "pw", null, null) } returns
            Result.failure(IllegalStateException("raw"))

        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        advanceUntilIdle()

        assertEquals(R.string.auth_register_failed, viewModel.uiState.value.errorRes)
        viewModel.clearError()
        assertNull(viewModel.uiState.value.errorRes)
    }

    @Test
    fun `register binds this phone with the ticket and no code`() = runTest {
        coEvery { authRepository.register("anna", "pw", null, null) } returns
            Result.success(RegistrationTicket("u", "anna", "2026-11-03T12:30:00Z", "ticket-secret"))
        coEvery { enrollmentApi.issueWithTicket("ticket-secret") } returns
            EnrollmentIssued("e1", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", "https://chat.example.com/setup#v=1&t=x", "t")

        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        advanceUntilIdle()

        coVerify { enrollmentApi.issueWithTicket("ticket-secret") }
        assertEquals(
            SetupEntry("https://chat.example.com", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", SetupEntry.Source.TICKET, ticket = "ticket-secret"),
            viewModel.uiState.value.setupEntry,
        )
        assertNull(viewModel.uiState.value.errorRes)
    }

    @Test
    fun `a failed issuance after the account was created retries with the same ticket`() = runTest {
        coEvery { authRepository.register("anna", "pw", null, null) } returns
            Result.success(io.wolftown.kaiku.domain.model.RegistrationTicket("u1", "anna", "2026-11-03T12:30:00Z", "ticket-1"))
        coEvery { enrollmentApi.issueWithTicket("ticket-1") } throws java.io.IOException("offline") andThen
            io.wolftown.kaiku.domain.model.EnrollmentIssued("e1", "setup-token", "https://chat.example.com/setup#v=1&t=x", "2026-11-03T12:10:00Z")
        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        advanceUntilIdle()
        assertTrue(viewModel.uiState.value.errorRes != null)

        viewModel.onRegisterClicked() // "Create account" again
        advanceUntilIdle()
        io.mockk.coVerify(exactly = 1) { authRepository.register(any(), any(), any(), any()) }
        assertEquals("ticket-1", viewModel.uiState.value.setupEntry?.ticket)
        viewModel.onSetupStarted()
        assertNull(viewModel.uiState.value.setupEntry)
    }

    @Test
    fun `a double tap registers once`() = runTest {
        coEvery { authRepository.register(any(), any(), any(), any()) } returns Result.failure(java.io.IOException("x"))
        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        viewModel.onRegisterClicked()
        advanceUntilIdle()
        io.mockk.coVerify(exactly = 1) { authRepository.register(any(), any(), any(), any()) }
    }
}
