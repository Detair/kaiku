package io.wolftown.kaiku.ui.auth

import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.mockk
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
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class RegisterViewModelTest {

    private lateinit var authRepository: AuthRepository
    private lateinit var viewModel: RegisterViewModel

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        authRepository = mockk(relaxed = true)
        viewModel = RegisterViewModel(authRepository)
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
    fun `successful registration sets the success flag`() = runTest {
        coEvery { authRepository.register("anna", "pw", null, null) } returns
            Result.success(User(id = "u", username = "anna", displayName = "Anna"))

        viewModel.onUsernameChanged("anna")
        viewModel.onPasswordChanged("pw")
        viewModel.onRegisterClicked()
        advanceUntilIdle()

        assertTrue(viewModel.uiState.value.registerSuccess)
        assertNull(viewModel.uiState.value.errorRes)
    }
}
