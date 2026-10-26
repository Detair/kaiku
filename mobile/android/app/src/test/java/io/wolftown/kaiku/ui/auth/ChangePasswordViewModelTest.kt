package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.User
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
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ChangePasswordViewModelTest {

    private val testDispatcher = StandardTestDispatcher()
    private lateinit var authRepository: AuthRepository
    private lateinit var viewModel: ChangePasswordViewModel

    private val user = User(id = "user-42", username = "anna", displayName = "Anna")

    @Before
    fun setUp() {
        Dispatchers.setMain(testDispatcher)
        authRepository = mockk(relaxed = true)
        viewModel = ChangePasswordViewModel(authRepository)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    private fun enter(newPassword: String, confirm: String = newPassword) {
        viewModel.onNewPasswordChanged(newPassword)
        viewModel.onConfirmChanged(confirm)
    }

    @Test
    fun `short password rejected`() = runTest {
        enter("seven77")

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_forced_change_length, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.done)
        coVerify(exactly = 0) { authRepository.completePasswordChange(any()) }
    }

    @Test
    fun `four emoji are too short because length counts code points`() = runTest {
        enter("\uD83D\uDE00".repeat(4)) // 4 code points, 8 UTF-16 units

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_forced_change_length, viewModel.uiState.value.error)
        coVerify(exactly = 0) { authRepository.completePasswordChange(any()) }
    }

    @Test
    fun `too long password rejected`() = runTest {
        enter("x".repeat(129))

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_forced_change_length, viewModel.uiState.value.error)
        coVerify(exactly = 0) { authRepository.completePasswordChange(any()) }
    }

    @Test
    fun `mismatched confirmation rejected`() = runTest {
        enter("my-own-password", confirm = "my-own-passw0rd")

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_forced_change_mismatch, viewModel.uiState.value.error)
        coVerify(exactly = 0) { authRepository.completePasswordChange(any()) }
    }

    @Test
    fun `valid password completes the change and sets done`() = runTest {
        coEvery { authRepository.completePasswordChange("my-own-password") } returns Result.success(user)
        enter("my-own-password")

        viewModel.onSubmit()
        advanceUntilIdle()

        val state = viewModel.uiState.value
        assertTrue(state.done)
        assertFalse(state.isLoading)
        assertNull(state.error)
        assertEquals("", state.newPassword)
        coVerify(exactly = 1) { authRepository.completePasswordChange("my-own-password") }
    }

    @Test
    fun `server validation error asks for a different password`() = runTest {
        coEvery { authRepository.completePasswordChange(any()) } returns Result.failure(
            ApiException(HttpStatusCode.BadRequest, "server text", code = "VALIDATION_ERROR")
        )
        enter("temp-pass-1")

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_forced_change_same_as_temporary, viewModel.uiState.value.error)
        assertFalse(viewModel.uiState.value.done)
    }

    @Test
    fun `missing pending change asks to sign in again`() = runTest {
        coEvery { authRepository.completePasswordChange(any()) } returns
            Result.failure(IllegalStateException("No password change pending"))
        enter("my-own-password")

        viewModel.onSubmit()
        advanceUntilIdle()

        assertEquals(R.string.auth_password_change_expired, viewModel.uiState.value.error)
    }

    @Test
    fun `cancel clears the pending change`() {
        viewModel.onCancel()

        verify(exactly = 1) { authRepository.cancelPasswordChange() }
    }
}
